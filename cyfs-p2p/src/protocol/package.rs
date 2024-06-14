use bucky_error::{BuckyError, BuckyErrorCode};
use bucky_raw_codec::{RawDecode, RawDecodeWithContext, RawEncodeWithContext};
use super::{
    common::*,
    sn::*,
    v0
};

pub struct DynamicPackage {
    version: u8,
    cmd_code: PackageCmdCode,
    package: Box<dyn Any + Send + Sync>,
}

impl Clone for DynamicPackage {
    fn clone(&self) -> Self {
        match self.cmd_code {
            PackageCmdCode::Exchange => {
                let pkg = self.as_any().downcast_ref::<Exchange>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SynTunnel => {
                let pkg = self.as_any().downcast_ref::<SynTunnel>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::AckTunnel => {
                let pkg = self.as_any().downcast_ref::<AckTunnel>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::AckAckTunnel => {
                let pkg = self.as_any().downcast_ref::<v0::AckAckTunnel>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::PingTunnel => {
                let pkg = self.as_any().downcast_ref::<v0::PingTunnel>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::PingTunnelResp => {
                let pkg = self.as_any().downcast_ref::<v0::PingTunnelResp>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnCall => {
                let pkg = self.as_any().downcast_ref::<SnCall>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnCallResp => {
                let pkg = self.as_any().downcast_ref::<v0::SnCallResp>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnCalled => {
                let pkg = self.as_any().downcast_ref::<v0::SnCalled>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnCalledResp => {
                let pkg = self.as_any().downcast_ref::<v0::SnCalledResp>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnPing => {
                let pkg = self.as_any().downcast_ref::<SnPing>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SnPingResp => {
                let pkg = self.as_any().downcast_ref::<v0::SnPingResp>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::Datagram => {
                let pkg = self.as_any().downcast_ref::<v0::Datagram>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SessionData => {
                let pkg = self.as_any().downcast_ref::<v0::SessionData>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::TcpSynConnection => {
                let pkg = self.as_any().downcast_ref::<v0::TcpSynConnection>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::TcpAckConnection => {
                let pkg = self.as_any().downcast_ref::<v0::TcpAckConnection>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::TcpAckAckConnection => {
                let pkg = self.as_any().downcast_ref::<v0::TcpAckAckConnection>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::SynProxy => {
                let pkg = self.as_any().downcast_ref::<SynProxy>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::AckProxy => {
                let pkg = self.as_any().downcast_ref::<v0::AckProxy>().unwrap();
                DynamicPackage::from(pkg.clone())
            }
            PackageCmdCode::PieceData => {
                unreachable!()
            }
            PackageCmdCode::PieceControl => {
                unreachable!()
            }
            PackageCmdCode::ChannelEstimate => {
                unreachable!()
            }
        }
    }
}
impl<Context: merge_context::Encode> AsRef<dyn RawEncodeWithContext<Context>> for DynamicPackage {
    fn as_ref(&self) -> &(dyn RawEncodeWithContext<Context> + 'static) {
        use super::super::protocol;
        downcast_handle!(self)
    }
}

impl<T: 'static + Package + Send + Sync> AsRef<T> for DynamicPackage {
    fn as_ref(&self) -> &T {
        self.as_any().downcast_ref::<T>().unwrap()
    }
}

impl<T: 'static + Package + Send + Sync> AsMut<T> for DynamicPackage {
    fn as_mut(&mut self) -> &mut T {
        self.as_mut_any().downcast_mut::<T>().unwrap()
    }
}

impl<'de, Context: merge_context::Decode> RawDecodeWithContext<'de, (&mut Context, &mut u8)>
    for DynamicPackage
{
    fn raw_decode_with_context(
        buf: &'de [u8],
        context: (&mut Context, &mut u8),
    ) -> Result<(Self, &'de [u8]), BuckyError> {
        let (merge_context, version) = context;
        let (cmd_code, buf) = u8::raw_decode(buf)?;
        let cmd_code = PackageCmdCode::try_from(cmd_code)?;
        //TOFIX: may use macro
        match cmd_code {
            PackageCmdCode::Exchange => Exchange::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf)),
            PackageCmdCode::SynTunnel => SynTunnel::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| {
                    *version = pkg.version();
                    (DynamicPackage::from(pkg), buf)
                }),
            PackageCmdCode::AckTunnel => AckTunnel::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| {
                    *version = pkg.version();
                    (DynamicPackage::from(pkg), buf)
                }),
            PackageCmdCode::SnCall => SnCall::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| {
                    *version = pkg.version();
                    (DynamicPackage::from(pkg), buf)
                }),
            PackageCmdCode::SnPing => SnPing::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| {
                    *version = pkg.version();
                    (DynamicPackage::from(pkg), buf)
                }),
            PackageCmdCode::SynProxy => SynProxy::raw_decode_with_context(buf, merge_context)
                .map(|(pkg, buf)| {
                    *version = pkg.version();
                    (DynamicPackage::from(pkg), buf)
                }),

            PackageCmdCode::AckAckTunnel => {
                if *version == 0 {
                    v0::AckAckTunnel::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::PingTunnel => {
                if *version == 0 {
                    v0::PingTunnel::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::PingTunnelResp => {
                if *version == 0 {
                    v0::PingTunnelResp::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::SnCallResp => {
                if *version == 0 {
                    v0::SnCallResp::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::SnCalled => {
                if *version == 0 {
                    v0::SnCalled::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::SnCalledResp => {
                if *version == 0 {
                    v0::SnCalledResp::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::SnPingResp => {
                if *version == 0 {
                    v0::SnPingResp::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::Datagram => {
                if *version == 0 {
                    v0::Datagram::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::SessionData => {
                if *version == 0 {
                    v0::SessionData::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::TcpSynConnection => {
                if *version == 0 {
                    v0::TcpSynConnection::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::TcpAckConnection => {
                if *version == 0 {
                    v0::TcpAckConnection::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::TcpAckAckConnection => {
                if *version == 0 {
                    v0::TcpAckAckConnection::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            PackageCmdCode::AckProxy => {
                if *version == 0 {
                    v0::AckProxy::raw_decode_with_context(buf, merge_context)
                        .map(|(pkg, buf)| (DynamicPackage::from(pkg), buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::NotSupport, "greater protocol version"))
                }
            },
            _ => Err(BuckyError::new(
                BuckyErrorCode::InvalidData,
                "package cmd code ",
            )),
        }
    }
}

impl DynamicPackage {
    pub fn version(&self) -> u8 {
        self.version
    }

    pub fn cmd_code(&self) -> PackageCmdCode {
        self.cmd_code
    }

    pub fn as_any(&self) -> &dyn Any {
        self.package.as_ref()
    }

    pub fn as_mut_any(&mut self) -> &mut dyn Any {
        self.package.as_mut()
    }

    pub fn into_any(self) -> Box<dyn Any + Send + Sync> {
        self.package
    }
}


impl<T: 'static + Package + Send + Sync> From<T> for DynamicPackage {
    fn from(p: T) -> Self {
        Self {
            version: p.version(),
            cmd_code: T::cmd_code(),
            package: Box::new(p),
        }
    }
}





