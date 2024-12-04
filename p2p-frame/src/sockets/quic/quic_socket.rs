use std::sync::Arc;
use std::time::Duration;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::VarInt;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::endpoint::{Endpoint, Protocol};
use crate::error::{BdtErrorCode, BdtResult, into_bdt_err};
use crate::p2p_identity::{P2pId, P2pIdentityRef, P2pIdentityCertFactoryRef};
use crate::runtime;

#[derive(Clone)]
pub struct QuicSocket {
    socket: quinn::Connection,
    remote_identity_id: P2pId,
    local_identity_id: P2pId,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicSocket {
    pub fn new(
        socket: quinn::Connection,
        local_identity_id: P2pId,
        remote_identity_id: P2pId,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            socket,
            local_identity_id,
            remote_identity_id,
            local,
            remote,
        }
    }

    pub async fn connect(local_identity_ref: P2pIdentityRef,
                         cert_factory: P2pIdentityCertFactoryRef,
                         remote_identity_id: P2pId,
                         remote: Endpoint,
                         timeout: Duration,
                         idle_timeout: Duration) -> BdtResult<Self> {
        log::info!("quic to {} connect begin", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory)))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(BdtErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(30) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(30)));
        }
        // transport_config.max_concurrent_bidi_streams(1_u8.into());
        // transport_config.max_concurrent_uni_streams(1_u8.into());
        client_config.transport_config(Arc::new(transport_config));
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);

        let conn = runtime::timeout(timeout, endpoint.connect(remote.addr().clone(),
                                    remote_identity_id.to_string().as_str()).unwrap()).await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_identity_ref.get_id(),
                     remote_identity_id,
                     Endpoint::from((Protocol::Udp, endpoint.local_addr().map_err(into_bdt_err!(BdtErrorCode::TlsError))?)),
                     remote))
    }

    pub async fn connect_with_ep(ep: quinn::Endpoint,
                                 local_identity_ref: P2pIdentityRef,
                                 cert_factory: P2pIdentityCertFactoryRef,
                                 remote_identity_id: P2pId,
                                 remote: Endpoint,
                                 timeout: Duration,
                                 idle_timeout: Duration) -> BdtResult<Self> {
        log::info!("connect with ep remote = {}", remote);
        let client_key = local_identity_ref.get_encoded_identity()?;
        let client_cert = local_identity_ref.get_identity_cert()?.get_encoded_cert()?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(crate::tls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::tls::TlsServerCertVerifier::new(cert_factory)))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(BdtErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(idle_timeout.try_into().unwrap()));
        if idle_timeout > Duration::from_secs(30) {
            transport_config.keep_alive_interval(Some(Duration::from_secs(30)));
        }
        client_config.transport_config(Arc::new(transport_config));

        let conn = runtime::timeout(timeout, ep.connect_with(client_config,
                                   remote.addr().clone(),
                                   remote_identity_id.to_string().as_str()).unwrap()).await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_identity_ref.get_id(),
                     remote_identity_id,
                     Endpoint::from((Protocol::Udp, ep.local_addr().map_err(into_bdt_err!(BdtErrorCode::TlsError))?)),
                     remote))
    }

    pub fn socket(&self) -> &quinn::Connection {
        &self.socket
    }

    pub fn local(&self) -> &Endpoint {
        &self.local
    }

    pub fn remote(&self) -> &Endpoint {
        &self.remote
    }

    pub fn local_identity_id(&self) -> &P2pId {
        &self.local_identity_id
    }

    pub fn remote_identity_id(&self) -> &P2pId {
        &self.remote_identity_id
    }

    pub async fn shutdown(&self) -> BdtResult<()> {
        self.socket.close(VarInt::from_u32(0), &[]);
        Ok(())
    }
}

