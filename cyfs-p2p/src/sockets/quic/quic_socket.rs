use std::sync::Arc;
use std::time::Duration;
use bucky_objects::{DeviceId, Endpoint, Protocol};
use bucky_raw_codec::RawConvertTo;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::VarInt;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use rustls::version::TLS13;
use crate::error::{bdt_err, BdtErrorCode, BdtResult, into_bdt_err};
use crate::{LocalDeviceRef, runtime};

#[derive(Clone)]
pub struct QuicSocket {
    socket: quinn::Connection,
    remote_device_id: DeviceId,
    local_device_id: DeviceId,
    local: Endpoint,
    remote: Endpoint,
}

impl QuicSocket {
    pub fn new(
        socket: quinn::Connection,
        local_device_id: DeviceId,
        remote_device_id: DeviceId,
        local: Endpoint,
        remote: Endpoint,
    ) -> Self {
        Self {
            socket,
            local_device_id,
            remote_device_id,
            local,
            remote,
        }
    }

    pub async fn connect(local_device_ref: LocalDeviceRef, remote_device_id: DeviceId, remote: Endpoint,
                         timeout: Duration,) -> BdtResult<Self> {
        log::info!("quic to {} connect begin", remote);
        let client_key = local_device_ref.key().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let client_cert = local_device_ref.device().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(bucky_rustls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(bucky_rustls::BuckyServerCertVerifier{}))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(BdtErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(600).try_into().unwrap()));
        // transport_config.max_concurrent_bidi_streams(1_u8.into());
        // transport_config.max_concurrent_uni_streams(1_u8.into());
        client_config.transport_config(Arc::new(transport_config));
        let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(client_config);

        let conn = runtime::timeout(timeout, endpoint.connect(remote.addr().clone(),
                                    remote_device_id.object_id().to_base36().as_str()).unwrap()).await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_device_ref.device_id().clone(),
                     remote_device_id,
                     bucky_objects::Endpoint::from((Protocol::Udp, endpoint.local_addr().map_err(into_bdt_err!(BdtErrorCode::TlsError))?)),
                     remote))
    }

    pub async fn connect_with_ep(ep: quinn::Endpoint, local_device_ref: LocalDeviceRef, remote_device_id: DeviceId, remote: Endpoint,
                                 timeout: Duration,) -> BdtResult<Self> {
        log::info!("connect with ep remote = {}", remote);
        let client_key = local_device_ref.key().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let client_cert = local_device_ref.device().to_vec().map_err(into_bdt_err!(BdtErrorCode::RawCodecError))?;
        let mut config =
            rustls::ClientConfig::builder_with_provider(bucky_rustls::provider().into())
                .with_protocol_versions(&[&TLS13])
                .unwrap()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(bucky_rustls::BuckyServerCertVerifier{}))
                .with_client_auth_cert(vec![CertificateDer::from(client_cert)], PrivatePkcs8KeyDer::from(client_key).into())
                .map_err(into_bdt_err!(BdtErrorCode::TlsError))?;
        config.enable_early_data = true;

        let mut client_config =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(config).unwrap()));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(600).try_into().unwrap()));
        client_config.transport_config(Arc::new(transport_config));

        let conn = runtime::timeout(timeout, ep.connect_with(client_config,
                                   remote.addr().clone(),
                                   remote_device_id.object_id().to_base36().as_str()).unwrap()).await
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?
            .map_err(into_bdt_err!(BdtErrorCode::ConnectFailed, "quic to {} connect failed", remote))?;
        Ok(Self::new(conn,
                     local_device_ref.device_id().clone(),
                     remote_device_id,
                     bucky_objects::Endpoint::from((Protocol::Udp, ep.local_addr().map_err(into_bdt_err!(BdtErrorCode::TlsError))?)),
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

    pub fn local_device_id(&self) -> &DeviceId {
        &self.local_device_id
    }

    pub fn remote_device_id(&self) -> &DeviceId {
        &self.remote_device_id
    }

    pub async fn shutdown(&self) -> BdtResult<()> {
        self.socket.close(VarInt::from_u32(0), &[]);
        Ok(())
    }
}

