use super::server::{
    OwnerServingCommandCode, OwnerServingResponse, into_owner_serving_cmd_tunnel,
    owner_serving_purpose,
};
use super::{
    LocalSnDirectory, LocalSnDirectoryRef, OwnerDirectoryClientRef, OwnerMembership, ServingLease,
};
use crate::error::{P2pError, P2pErrorCode, P2pResult, into_p2p_err, p2p_err};
use crate::networks::{TunnelStreamRead, TunnelStreamWrite};
use crate::p2p_identity::P2pId;
use crate::sn::inter_sn::TtpInterSnClientRef;
use crate::sn::protocol::{SnPublishLease, SnQueryLease};
use crate::sn::types::{OwnerCmdPkgLen, SnTunnelRead, SnTunnelWrite};
use crate::ttp::{TtpConnector, TtpTarget};
use async_trait::async_trait;
use bucky_raw_codec::{RawConvertTo, RawDecode, RawFrom};
use log::warn;
use sfo_cmd_server::client::{CmdClient, CmdTunnelFactory};
use sfo_cmd_server::errors::{CmdErrorCode, CmdResult, cmd_err};
use sfo_cmd_server::{CmdBody, CmdTunnel};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

const OWNER_SERVING_CMD_VERSION: u8 = 0;
const OWNER_SERVING_CMD_TIMEOUT: Duration = Duration::from_secs(10);
const OWNER_SERVING_CMD_TUNNEL_COUNT: u16 = 4;

type OwnerServingCmdClient = sfo_cmd_server::client::DefaultCmdClient<
    (),
    SnTunnelRead,
    SnTunnelWrite,
    OwnerServingTunnelFactory,
    OwnerCmdPkgLen,
    OwnerServingCommandCode,
>;

#[async_trait]
pub trait OwnerDirectoryClient: Send + Sync + 'static {
    async fn publish_serving_lease(
        &self,
        local_sn_id: P2pId,
        peer_id: P2pId,
        sequence: u64,
    ) -> P2pResult<()>;

    async fn query_serving_leases(
        &self,
        local_sn_id: &P2pId,
        peer_id: &P2pId,
    ) -> P2pResult<Vec<ServingLease>>;
}

pub struct NoopOwnerDirectoryClient;

#[async_trait]
impl OwnerDirectoryClient for NoopOwnerDirectoryClient {
    async fn publish_serving_lease(
        &self,
        _local_sn_id: P2pId,
        _peer_id: P2pId,
        _sequence: u64,
    ) -> P2pResult<()> {
        Ok(())
    }

    async fn query_serving_leases(
        &self,
        _local_sn_id: &P2pId,
        _peer_id: &P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        Ok(Vec::new())
    }
}

pub fn noop_owner_directory_client() -> OwnerDirectoryClientRef {
    Arc::new(NoopOwnerDirectoryClient)
}

pub struct StaticOwnerDirectoryClient {
    directory: LocalSnDirectoryRef,
    serving_cmd_clients: Option<HashMap<P2pId, Arc<OwnerServingCmdClient>>>,
    inter_sn_client: Option<TtpInterSnClientRef>,
}

impl StaticOwnerDirectoryClient {
    pub fn new(
        membership: OwnerMembership,
        inter_sn_client: Option<TtpInterSnClientRef>,
    ) -> OwnerDirectoryClientRef {
        Self::new_with_serving_connector(membership, None, inter_sn_client)
    }

    pub fn new_with_serving_connector(
        membership: OwnerMembership,
        serving_connector: Option<Arc<dyn TtpConnector>>,
        inter_sn_client: Option<TtpInterSnClientRef>,
    ) -> OwnerDirectoryClientRef {
        let serving_cmd_clients = serving_connector.map(|connector| {
            owner_targets(&membership)
                .into_iter()
                .map(|(owner_sn_id, target)| {
                    let client = sfo_cmd_server::client::DefaultCmdClient::new(
                        OwnerServingTunnelFactory::new(connector.clone(), target),
                        OWNER_SERVING_CMD_TUNNEL_COUNT,
                    );
                    (owner_sn_id, client)
                })
                .collect()
        });
        Arc::new(Self {
            directory: LocalSnDirectory::new(Some(membership)),
            serving_cmd_clients,
            inter_sn_client,
        })
    }

    async fn publish_to_owner(
        &self,
        owner_sn_id: &P2pId,
        local_sn_id: P2pId,
        lease: ServingLease,
    ) -> P2pResult<bool> {
        if let Some(clients) = &self.serving_cmd_clients {
            let client = clients.get(owner_sn_id).ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "owner serving target endpoint missing for {}",
                    owner_sn_id
                )
            })?;
            return send_owner_serving_publish(client, lease).await;
        }

        Err(p2p_err!(
            P2pErrorCode::NotFound,
            "owner serving transport client missing for {}",
            owner_sn_id
        ))
    }

    async fn query_owner(
        &self,
        owner_sn_id: &P2pId,
        local_sn_id: P2pId,
        peer_id: P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        if let Some(clients) = &self.serving_cmd_clients {
            let client = clients.get(owner_sn_id).ok_or_else(|| {
                p2p_err!(
                    P2pErrorCode::NotFound,
                    "owner serving target endpoint missing for {}",
                    owner_sn_id
                )
            })?;
            return send_owner_serving_query(client, peer_id).await;
        }

        Err(p2p_err!(
            P2pErrorCode::NotFound,
            "owner serving transport client missing for {}",
            owner_sn_id
        ))
    }
}

#[async_trait]
impl OwnerDirectoryClient for StaticOwnerDirectoryClient {
    async fn publish_serving_lease(
        &self,
        local_sn_id: P2pId,
        peer_id: P2pId,
        sequence: u64,
    ) -> P2pResult<()> {
        let lease = self
            .directory
            .new_lease(peer_id.clone(), local_sn_id.clone(), sequence);
        let mut accepted = false;

        for owner_sn_id in self.directory.owner_set(&peer_id, &local_sn_id) {
            match self
                .publish_to_owner(&owner_sn_id, local_sn_id.clone(), lease.clone())
                .await
            {
                Ok(written) => {
                    self.directory.refresh_owner_member(&owner_sn_id);
                    accepted |= written;
                }
                Err(err) => warn!(
                    "serving lease publish failed peer={} owner_sn={} err={:?}",
                    peer_id, owner_sn_id, err
                ),
            }
        }

        if accepted {
            Ok(())
        } else {
            Err(p2p_err!(
                P2pErrorCode::NotFound,
                "no owner accepted serving lease peer={}",
                peer_id
            ))
        }
    }

    async fn query_serving_leases(
        &self,
        local_sn_id: &P2pId,
        peer_id: &P2pId,
    ) -> P2pResult<Vec<ServingLease>> {
        let mut leases = Vec::new();
        for owner_sn_id in self.directory.owner_set(peer_id, local_sn_id) {
            match self
                .query_owner(&owner_sn_id, local_sn_id.clone(), peer_id.clone())
                .await
            {
                Ok(mut remote_leases) => {
                    self.directory.refresh_owner_member(&owner_sn_id);
                    leases.append(&mut remote_leases);
                }
                Err(err) => warn!(
                    "query serving leases failed peer={} owner_sn={} err={:?}",
                    peer_id, owner_sn_id, err
                ),
            }
        }
        leases.sort_by(|left, right| {
            right
                .sequence
                .cmp(&left.sequence)
                .then_with(|| left.serving_sn_id.cmp(&right.serving_sn_id))
        });
        leases.dedup_by(|left, right| left.serving_sn_id == right.serving_sn_id);
        Ok(leases)
    }
}

#[derive(Clone)]
struct OwnerServingTunnelFactory {
    connector: Arc<dyn TtpConnector>,
    target: TtpTarget,
}

impl OwnerServingTunnelFactory {
    fn new(connector: Arc<dyn TtpConnector>, target: TtpTarget) -> Self {
        Self { connector, target }
    }
}

#[async_trait]
impl CmdTunnelFactory<(), SnTunnelRead, SnTunnelWrite> for OwnerServingTunnelFactory {
    async fn create_tunnel(&self) -> CmdResult<CmdTunnel<SnTunnelRead, SnTunnelWrite>> {
        let accepted = self
            .connector
            .open_control_stream(
                &self.target,
                owner_serving_purpose().map_err(p2p_to_cmd_err)?,
            )
            .await
            .map_err(p2p_to_cmd_err)?;
        Ok(into_owner_serving_cmd_tunnel(accepted))
    }
}

fn owner_targets(membership: &OwnerMembership) -> HashMap<P2pId, TtpTarget> {
    let mut targets = HashMap::new();
    for member in membership.members() {
        if let Some(endpoint) = member.endpoints.first().copied() {
            targets.insert(
                member.sn_id.clone(),
                TtpTarget {
                    local_ep: None,
                    remote_ep: endpoint,
                    remote_id: member.sn_id.clone(),
                    remote_name: member.name.clone(),
                },
            );
        }
    }
    targets
}

async fn send_owner_serving_publish(
    client: &Arc<OwnerServingCmdClient>,
    lease: ServingLease,
) -> P2pResult<bool> {
    let body = SnPublishLease::from(lease)
        .to_vec()
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    match send_owner_serving_request(
        client,
        OwnerServingCommandCode::PublishLease,
        body.as_slice(),
    )
    .await?
    {
        OwnerServingResponse::Published(written) => Ok(written),
        OwnerServingResponse::Error(err) => Err(err.into()),
        _ => Err(p2p_err!(
            P2pErrorCode::InvalidData,
            "unexpected owner serving publish response"
        )),
    }
}

async fn send_owner_serving_query(
    client: &Arc<OwnerServingCmdClient>,
    peer_id: P2pId,
) -> P2pResult<Vec<ServingLease>> {
    let body = SnQueryLease { peer_id }
        .to_vec()
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))?;
    match send_owner_serving_request(client, OwnerServingCommandCode::QueryLease, body.as_slice())
        .await?
    {
        OwnerServingResponse::Leases(resp) => {
            Ok(resp.leases.into_iter().map(ServingLease::from).collect())
        }
        OwnerServingResponse::Error(err) => Err(err.into()),
        _ => Err(p2p_err!(
            P2pErrorCode::InvalidData,
            "unexpected owner serving query response"
        )),
    }
}

async fn send_owner_serving_request(
    client: &Arc<OwnerServingCmdClient>,
    command: OwnerServingCommandCode,
    body: &[u8],
) -> P2pResult<OwnerServingResponse> {
    let response = client
        .send_with_resp(
            command,
            OWNER_SERVING_CMD_VERSION,
            body,
            OWNER_SERVING_CMD_TIMEOUT,
        )
        .await
        .map_err(cmd_to_p2p_err)?
        .into_bytes()
        .await
        .map_err(cmd_to_p2p_err)?;
    OwnerServingResponse::clone_from_slice(response.as_slice())
        .map_err(into_p2p_err!(P2pErrorCode::RawCodecError))
}

fn p2p_to_cmd_err(err: P2pError) -> sfo_cmd_server::errors::CmdError {
    cmd_err!(CmdErrorCode::Failed, "{:?}", err)
}

fn cmd_to_p2p_err(err: sfo_cmd_server::errors::CmdError) -> P2pError {
    P2pError::new(P2pErrorCode::Failed, err.msg().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::endpoint::{Endpoint, Protocol};
    use crate::networks::{TunnelDatagramWrite, TunnelPurpose};
    use crate::sn::directory::OwnerMember;
    use crate::ttp::{TtpDatagramMeta, TtpStreamMeta};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn test_id(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    struct FailingConnector {
        control_stream_calls: AtomicUsize,
        purposes: Mutex<Vec<TunnelPurpose>>,
    }

    impl FailingConnector {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                control_stream_calls: AtomicUsize::new(0),
                purposes: Mutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl TtpConnector for FailingConnector {
        async fn open_stream(
            &self,
            _target: &TtpTarget,
            _purpose: TunnelPurpose,
        ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
            Err(p2p_err!(P2pErrorCode::Failed, "unexpected open_stream"))
        }

        async fn open_control_stream(
            &self,
            _target: &TtpTarget,
            purpose: TunnelPurpose,
        ) -> P2pResult<(TtpStreamMeta, TunnelStreamRead, TunnelStreamWrite)> {
            self.control_stream_calls.fetch_add(1, Ordering::SeqCst);
            self.purposes.lock().unwrap().push(purpose);
            Err(p2p_err!(
                P2pErrorCode::Failed,
                "expected fake connector failure"
            ))
        }

        async fn open_datagram(
            &self,
            _target: &TtpTarget,
            _purpose: TunnelPurpose,
        ) -> P2pResult<(TtpDatagramMeta, TunnelDatagramWrite)> {
            Err(p2p_err!(P2pErrorCode::Failed, "unexpected open_datagram"))
        }
    }

    #[tokio::test]
    async fn owner_serving_listener_client_uses_serving_connector() {
        let owner_sn_id = test_id(10);
        let local_sn_id = test_id(11);
        let peer_id = test_id(12);
        let owner_ep = Endpoint::from((Protocol::Tcp, "127.0.0.1:19090".parse().unwrap()));
        let membership = OwnerMembership::with_members(
            vec![OwnerMember::with_endpoint(owner_sn_id, owner_ep)],
            1,
            Duration::from_secs(60),
        )
        .unwrap();
        let connector = FailingConnector::new();
        let client = StaticOwnerDirectoryClient::new_with_serving_connector(
            membership,
            Some(connector.clone() as Arc<dyn TtpConnector>),
            None,
        );

        let err = client
            .publish_serving_lease(local_sn_id, peer_id, 1)
            .await
            .unwrap_err();

        assert_eq!(err.code(), P2pErrorCode::NotFound);
        assert_eq!(connector.control_stream_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            connector.purposes.lock().unwrap().as_slice(),
            &[owner_serving_purpose().unwrap()]
        );
    }
}
