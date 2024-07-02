use std::sync::{Arc, RwLock};
use bucky_crypto::KeyMixHash;
use bucky_error::{BuckyError, BuckyErrorCode};
use bucky_objects::{DeviceId, Endpoint};
use bucky_raw_codec::{RawDecode, RawDecodeWithContext};
use crate::executor::Executor;
use crate::history::keystore;
use crate::history::keystore::{EncryptedKey, Keystore};
use crate::protocol::{Package, Exchange, merge_context, MTU_LARGE, PackageBox, PackageBoxDecodeContext, SnCall};
use crate::sockets::udp::udp_socket::{UDPSocket};
use super::super::UpdateOuterResult;
use crate::types::{LocalDeviceRef, MixAesKey};

pub struct UdpPackageBox {
    package_box: PackageBox,
    remote: Endpoint,
}

impl UdpPackageBox {
    pub fn new(package_box: PackageBox, remote: Endpoint) -> Self {
        Self {
            package_box,
            remote,
        }
    }

    pub fn remote(&self) -> &Endpoint {
        &self.remote
    }
}

impl Into<PackageBox> for UdpPackageBox {
    fn into(self) -> PackageBox {
        self.package_box
    }
}

impl AsRef<PackageBox> for UdpPackageBox {
    fn as_ref(&self) -> &PackageBox {
        &self.package_box
    }
}

#[async_trait::async_trait]
pub trait UDPListenerEventListener: 'static + Send + Sync {
    async fn on_udp_package_box(
        &self,
        socket: Arc<UDPSocket>,
        package_box: UdpPackageBox,
    );
    async fn on_udp_raw_data(&self, data: &[u8], context: (Arc<UDPSocket>, DeviceId, MixAesKey, Endpoint)) -> Result<(), BuckyError>;
}

struct UDPListenerState {
    outer: Option<Endpoint>,
    mapping_port: Option<u16>,
    socket: Option<Arc<UDPSocket>>,
    listener: Option<Arc<dyn UDPListenerEventListener>>,
}
pub struct UDPListener {
    key_store: Arc<Keystore>,
    sn_only: bool,
    state: RwLock<UDPListenerState>,
    buffer_size: usize,
}
pub type UDPListenerRef = Arc<UDPListener>;

impl UDPListener {
    pub fn new(
        key_store: Arc<Keystore>,
        sn_only: bool,
        buffer_size: usize,
    ) -> Arc<Self> {
        Arc::new(Self {
            key_store,
            sn_only,
            state: RwLock::new(UDPListenerState {
                outer: None,
                mapping_port: None,
                socket: None,
                listener: None,
            }),
            buffer_size,
        })
    }

    pub async fn bind(&self, local: &Endpoint, out: Option<Endpoint>, mapping_port: Option<u16>) -> Result<(), BuckyError> {
        {
            let mut state = self.state.write().unwrap();
            if state.socket.is_some() {
                return Err(BuckyError::new(BuckyErrorCode::ErrorState, "already bind"));
            }
        }

        let socket = UDPSocket::bind(local, self.buffer_size)?;
        let mut state = self.state.write().unwrap();
        state.socket = Some(Arc::new(socket));
        state.outer = out;
        state.mapping_port = mapping_port;
        Ok(())
    }

    pub fn set_listener(&self, listener: Arc<dyn UDPListenerEventListener>) {
        self.state.write().unwrap().listener = Some(listener);
    }

    pub fn local(&self) -> Endpoint {
        self.state.read().unwrap().socket.as_ref().unwrap().local().clone()
    }

    pub fn mapping_port(&self) -> Option<u16> {
        self.state.read().unwrap().mapping_port
    }

    pub fn outer(&self) -> Option<Endpoint> {
        self.state.read().unwrap().outer
    }

    pub fn socket(&self) -> Option<Arc<UDPSocket>> {
        self.state.read().unwrap().socket.clone()
    }

    pub fn update_outer(&self, outer: &Endpoint) -> UpdateOuterResult {
        let self_outer = &mut *self.state.write().unwrap();
        if let Some(outer_ep) = self_outer.outer.as_ref() {
            if *outer_ep != *outer {
                info!("{} reset outer to {}", self_outer.socket.as_ref().unwrap(), outer);
                self_outer.outer = Some(*outer);
                UpdateOuterResult::Reset
            } else {
                trace!("{} ignore update outer to {}", self_outer.socket.as_ref().unwrap(), outer);
                UpdateOuterResult::None
            }
        } else {
            info!("{} update outer to {}", self_outer.socket.as_ref().unwrap(), outer);
            self_outer.outer = Some(*outer);
            UpdateOuterResult::Update
        }
    }

    pub async fn reset(self: &Arc<Self>, local: &Endpoint) -> Arc<Self> {
        let new = self.clone();
        let mut state = self.state.write().unwrap();
        info!("{} reset with {}", state.socket.as_ref().unwrap(), local);
        state.outer = None;
        self.clone()
    }

    pub fn start(self: &Arc<Self>) {
        let this = self.clone();
        let socket = {
            self.state.read().unwrap().socket.clone().unwrap()
        };
        std::thread::spawn(move || {
            Executor::block_on(async move {
                let mut recv_buf = [0u8; MTU_LARGE];
                loop {
                    let rr = socket.recv_from(&mut recv_buf).await;
                    if rr.is_ok() {
                        let (len, from) = rr.unwrap();
                        trace!("{} recv {} bytes from {}", socket, len, from);
                        let recv = &mut recv_buf[..len];
                        // FIXME: 分发到工作线程去
                        this.on_recv(recv, from).await;
                    } else {
                        let err = rr.err().unwrap();
                        if let Some(10054i32) = err.raw_os_error() {
                            // In Windows, if host A use UDP socket and call sendto() to send something to host B,
                            // but B doesn't bind any port so that B doesn't receive the message,
                            // and then host A call recvfrom() to receive some message,
                            // recvfrom() will failed, and WSAGetLastError() will return 10054.
                            // It's a bug of Windows.
                            trace!("{} socket recv failed for {}, ingore this error", socket, err);
                        } else {
                            info!("{} socket recv failed for {}, break recv loop", socket, err);
                            break;
                        }
                    }
                }
            });
        });
    }


    async fn on_recv(self: &Arc<Self>, recv: &mut [u8], from: Endpoint) {
        if recv.len() == 0 {
            return
        }

        if recv[0] & 0x80 != 0 {
            match KeyMixHash::raw_decode(recv) {
                Ok((mut mix_hash, raw_data)) => {
                    mix_hash.as_mut()[0] &= 0x7f;
                    if let Some(found_key) =
                        self.key_store.get_key_by_mix_hash(&mix_hash) {
                        if self.sn_only {
                            return;
                        }
                        let (listener, socket) = {
                            let state = self.state.read().unwrap();
                            (state.listener.clone(), state.socket.as_ref().unwrap().clone())
                        };

                        if listener.is_none() {
                            return;
                        }
                        let _ =
                            listener.as_ref().unwrap().on_udp_raw_data(raw_data, (socket, found_key.remote_id, found_key.key, from)).await;

                        return;
                    }
                }
                Err(err) => {
                    error!("{} decode failed, from={}, len={}, e={}", self.state.read().unwrap().socket.as_ref().unwrap(), from, recv.len(), &err);
                    return;
                }
            }
        }
        let ctx =
            PackageBoxDecodeContext::new_inplace(recv.as_mut_ptr(), recv.len(), self.key_store.as_ref());
        match PackageBox::raw_decode_with_context(recv, ctx) {
            Ok((package_box, _)) => {
                if self.sn_only && !package_box.is_sn() {
                    return;
                }
                let this = self.clone();
                if package_box.has_exchange() {
                    Executor::spawn_ok(async move {
                        let exchange: &Exchange = package_box.packages()[0].as_ref();
                        let (listener, socket) = {
                            let state = this.state.read().unwrap();
                            (state.listener.clone(), state.socket.as_ref().unwrap().clone())
                        };
                        if !exchange.verify(package_box.local()).await {
                            warn!("{} exchg verify failed, from {}.", socket, from);
                            return;
                        }
                        this.key_store.add_key(package_box.key(), package_box.local(), package_box.remote(), EncryptedKey::Unconfirmed(exchange.key_encrypted.clone()));
                        if listener.is_none() {
                            return;
                        }

                        listener.as_ref().unwrap().on_udp_package_box(this.socket().as_ref().unwrap().clone(),
                                                                      UdpPackageBox::new(
                                                                          package_box,
                                                                          from,
                                                                      )).await;
                    });
                } else {
                    let listener = {
                        let state = self.state.read().unwrap();
                        state.listener.clone()
                    };
                    if listener.is_none() {
                        return;
                    }
                    Executor::spawn_ok(async move {
                        let _ = listener.as_ref().unwrap().on_udp_package_box(this.socket().as_ref().unwrap().clone(),
                                                                              UdpPackageBox::new(
                                                                                  package_box,
                                                                                  from,
                                                                              )).await;
                    });
                }
            }
            Err(err) => {
                // do nothing
                error!("{} decode failed, from={}, len={}, e={}", self.state.read().unwrap().socket.as_ref().unwrap(), from, recv.len(), &err);
            }
        }
    }
}
