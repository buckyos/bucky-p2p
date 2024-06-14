use bucky_crypto::{AesKey, hash_data, KeyMixHash, PrivateKey, PublicKey};
use bucky_error::{BuckyError, BuckyErrorCode, BuckyResult};
use bucky_objects::{DeviceId, NamedObject, ObjectId};
use bucky_raw_codec::{RawDecode, RawDecodeWithContext, RawEncode, RawEncodePurpose, RawEncodeWithContext, RawFixedBytes};
use crate::error::{bdt_err, BdtError, BdtErrorCode, BdtResult, into_bdt_err};
use crate::history::keystore;
use super::{common::*, package::*, SnCall};

//TODO: Option<AesKey> 支持明文包
pub struct PackageBox {
    local: DeviceId,
    remote: DeviceId,
    key: MixAesKey,
    packages: Vec<DynamicPackage>,
}

impl std::fmt::Debug for PackageBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackageBox:{{remote:{},key:{},packages:", self.remote, self.key)?;
        for package in self.packages() {
            use crate::protocol;
            downcast_handle!(package, |p| {
                let _ = write!(f, "{:?};", p);
            });
        }
        write!(f, "}}")
    }
}

impl PackageBox {
    pub fn from_packages(local: DeviceId, remote: DeviceId, key: MixAesKey, packages: Vec<DynamicPackage>) -> Self {
        // session package 的数组，不合并
        let mut package_box = Self::encrypt_box(local, remote, key);
        package_box.append(packages);
        package_box
    }

    pub fn from_package(local: DeviceId, remote: DeviceId, key: MixAesKey, package: DynamicPackage) -> Self {
        let mut package_box = Self::encrypt_box(local, remote.clone(), key);
        package_box.packages.push(package);
        package_box
    }

    pub fn encrypt_box(local: DeviceId, remote: DeviceId, key: MixAesKey) -> Self {
        Self {
            local,
            remote,
            key,
            packages: vec![],
        }
    }

    pub fn append(&mut self, packages: Vec<DynamicPackage>) -> &mut Self {
        let mut packages = packages;
        self.packages.append(&mut packages);
        self
    }

    pub fn push<T: 'static + Package + Send + Sync>(&mut self, p: T) -> &mut Self {
        self.packages.push(DynamicPackage::from(p));
        self
    }

    pub fn pop(&mut self) -> Option<DynamicPackage> {
        if self.packages.is_empty() {
            None
        } else {
            Some(self.packages.remove(0))
        }
    }

    pub fn local(&self) -> &DeviceId {
        &self.local
    }

    pub fn remote(&self) -> &DeviceId {
        &self.remote
    }

    pub fn key(&self) -> &MixAesKey {
        &self.key
    }

    pub fn has_exchange(&self) -> bool {
        if self.packages.is_empty() {
            return false;
        }
        self.packages.get(0).unwrap().cmd_code().is_exchange()
    }

    pub fn is_sn(&self) -> bool {
        self.packages_no_exchange()
            .get(0)
            .unwrap()
            .cmd_code()
            .is_sn()
    }

    pub fn is_tunnel(&self) -> bool {
        self.packages_no_exchange()
            .get(0)
            .unwrap()
            .cmd_code()
            .is_tunnel()
    }

    pub fn is_tcp_stream(&self) -> bool {
        self.packages_no_exchange()
            .get(0)
            .unwrap()
            .cmd_code()
            .is_tcp_stream()
    }

    pub fn is_proxy(&self) -> bool {
        self.packages_no_exchange()
            .get(0)
            .unwrap()
            .cmd_code()
            .is_proxy()
    }

    pub fn packages(&self) -> &[DynamicPackage] {
        self.packages.as_ref()
    }

    pub fn mut_packages(&mut self) -> &mut [DynamicPackage] {
        self.packages.as_mut()
    }

    pub fn packages_no_exchange(&self) -> &[DynamicPackage] {
        if self.has_exchange() {
            &self.packages()[1..]
        } else {
            self.packages()
        }
    }

    pub fn mut_packages_no_exchage(&mut self) -> &mut [DynamicPackage] {
        if self.has_exchange() {
            &mut self.mut_packages()[1..]
        } else {
            self.mut_packages()
        }
    }
}

impl Into<Vec<DynamicPackage>> for PackageBox {
    fn into(self) -> Vec<DynamicPackage> {
        self.packages
    }
}


pub(crate) struct PackageBoxEncodeContext {
    plaintext: bool,
    ignore_exchange: bool,
    fixed_values: merge_context::FixedValues,
    merged_values: Option<merge_context::ContextNames>,
}

impl PackageBoxEncodeContext {
    pub fn plaintext(&self) -> bool {
        self.plaintext
    }

    pub fn set_plaintext(&mut self, b: bool) {
        self.plaintext = b
    }

    pub fn set_ignore_exchange(&mut self, b: bool) {
        self.ignore_exchange = b
    }
}

// 编码SnCall::payload
impl From<&SnCall> for PackageBoxEncodeContext {
    fn from(sn_call: &SnCall) -> Self {
        let fixed_values: merge_context::FixedValues = sn_call.into();
        let merged_values = fixed_values.clone_merged();
        Self {
            plaintext: false,
            ignore_exchange: false,
            fixed_values,
            merged_values: Some(merged_values),
        }
    }
}

// impl From<(&DeviceDesc, Timestamp)> for PackageBoxEncodeContext {
//     fn from(params: (&DeviceDesc, Timestamp)) -> Self {
//         let mut fixed_values = merge_context::FixedValues::new();
//         fixed_values.insert("send_time", params.1);
//         Self {
//             ignore_exchange: false,
//             remote_const: Some(params.0.clone()),
//             fixed_values,
//             merged_values: None,
//         }
//     }
// }

impl Default for PackageBoxEncodeContext {
    fn default() -> Self {
        Self {
            plaintext: false,
            ignore_exchange: false,
            fixed_values: merge_context::FixedValues::new(),
            merged_values: None,
        }
    }
}

enum DecryptBuffer<'de> {
    Copy(&'de mut [u8]),
    Inplace(*mut u8, usize),
}

pub(crate) trait PackageBoxVersionGetter {
    fn version_of(&self, remote: &DeviceId) -> u8;
}

pub(crate) struct PackageBoxDecodeContext<'de> {
    decrypt_buf: DecryptBuffer<'de>,
    keystore: &'de keystore::Keystore,
}

impl<'de> PackageBoxDecodeContext<'de> {
    pub fn new_copy(
        decrypt_buf: &'de mut [u8],
        keystore: &'de keystore::Keystore,
    ) -> Self {
        Self {
            decrypt_buf: DecryptBuffer::Copy(decrypt_buf),
            keystore,
        }
    }

    pub fn new_inplace(
        ptr: *mut u8,
        len: usize,
        keystore: &'de keystore::Keystore,
    ) -> Self {
        Self {
            decrypt_buf: DecryptBuffer::Inplace(ptr, len),
            keystore,
        }
    }

    // 返回用于aes 解码的buffer
    pub unsafe fn decrypt_buf(self, data: &[u8]) -> &'de mut [u8] {
        use DecryptBuffer::*;
        match self.decrypt_buf {
            Copy(decrypt_buf) => {
                decrypt_buf[..data.len()].copy_from_slice(data);
                decrypt_buf
            }
            Inplace(ptr, len) => {
                std::slice::from_raw_parts_mut(ptr.offset((len - data.len()) as isize), data.len())
            }
        }
    }
    // 拿到local私钥
    pub fn local_secret(&self, device_id: &DeviceId) -> Option<PrivateKey> {
        self.keystore.private_key(device_id)
    }

    pub fn local_public_key(&self, device_id: &DeviceId) -> Option<PublicKey> {
        self.keystore.public_key(device_id)
    }

    pub fn key_from_mixhash(&self, mix_hash: &KeyMixHash) -> Option<(DeviceId, DeviceId, MixAesKey)> {
        self.keystore
            .get_key_by_mix_hash(mix_hash)
            .map(|k| (k.local_id, k.remote_id, k.key))
    }

    pub fn version_of(&self, _remote: &DeviceId) -> u8 {
        0
    }
}

impl RawEncodeWithContext<PackageBoxEncodeContext> for PackageBox {
    fn raw_measure_with_context(
        &self,
        _: &mut PackageBoxEncodeContext,
        _purpose: &Option<RawEncodePurpose>,
    ) -> Result<usize, BuckyError> {
        //TODO
        Ok(2048)
    }

    fn raw_encode_with_context<'a>(
        &self,
        buf: &'a mut [u8],
        context: &mut PackageBoxEncodeContext,
        purpose: &Option<RawEncodePurpose>,
    ) -> Result<&'a mut [u8], BuckyError> {
        let mut buf = buf;
        if self.has_exchange() && !context.ignore_exchange {
            let exchange: &Exchange = self.packages()[0].as_ref();
            if buf.len() < exchange.key_encrypted.len() {
                log::error!("try encode exchange without public-key");
                assert!(false);
                return Err(BuckyError::new(
                    BuckyErrorCode::Failed,
                    "try encode exchange without public-key",
                ));
            }
            let confusion = rand::random::<u16>();
            let mut hash = hash_data(confusion.to_le_bytes().as_slice()).as_slice().to_vec();
            hash.extend(&hash_data(hash.as_slice()).as_slice()[0..16]);
            let aes_key = AesKey::from(hash);


            let mut encrypted_device_id = vec![0u8; AesKey::padded_len(DeviceId::raw_bytes().unwrap())];
            aes_key.encrypt(exchange.to_device_id.as_ref().as_slice(), encrypted_device_id.as_mut_slice(), exchange.to_device_id.as_ref().as_slice().len())?;

            // 写入对端的device_id
            buf = confusion.raw_encode(buf, &None)?;
            buf[..encrypted_device_id.len()].copy_from_slice(encrypted_device_id.as_slice());
            buf = &mut buf[encrypted_device_id.len()..];
            // 首先用对端的const info加密aes key
            buf[..exchange.key_encrypted.len()].copy_from_slice(&exchange.key_encrypted[..]);
            buf = &mut buf[exchange.key_encrypted.len()..];
        }

        // 写入 key的mixhash
        let mixhash = self.key().mix_hash(self.remote(), self.local());
        let _ = mixhash.raw_encode(buf, purpose)?;
        if context.plaintext {
            buf[0] |= 0x80;
        }
        let buf = &mut buf[8..];

        let mut encrypt_in_len = buf.len();
        let to_encrypt_buf = buf;

        // 编码所有包
        let packages = if context.ignore_exchange {
            self.packages_no_exchange()
        } else {
            self.packages()
        };
        let (mut other_context, mut buf, packages) = match context.merged_values.as_ref() {
            Some(merged_values) => (
                merge_context::OtherEncode::new(merged_values.clone(), Some(&context.fixed_values)),
                &mut to_encrypt_buf[..],
                packages,
            ),
            None => {
                let mut first_context = merge_context::FirstEncode::from(&context.fixed_values); // merge_context::FirstEncode::new();
                let enc: &dyn RawEncodeWithContext<merge_context::FirstEncode> =
                    packages.get(0).unwrap().as_ref();
                let buf = enc.raw_encode_with_context(to_encrypt_buf, &mut first_context, purpose)?;
                (first_context.into(), buf, &packages[1..])
            }
        };
        for p in packages {
            let enc: &dyn RawEncodeWithContext<merge_context::OtherEncode> = p.as_ref();
            buf = enc.raw_encode_with_context(buf, &mut other_context, purpose)?;
        }
        //let buf_len = buf.len();
        encrypt_in_len -= buf.len();
        // 用aes 加密package的部分
        let len = if context.plaintext {
            encrypt_in_len
        } else {
            self.key().enc_key.inplace_encrypt(to_encrypt_buf, encrypt_in_len)?
        };

        //info!("package_box udp encode: encrypt_in_len={} len={} buf_len={} plaintext={}",
        //    encrypt_in_len, len, buf_len, context.plaintext);

        Ok(&mut to_encrypt_buf[len..])
    }
}

impl<'de> RawDecodeWithContext<'de, PackageBoxDecodeContext<'de>> for PackageBox {
    fn raw_decode_with_context(
        buf: &'de [u8],
        context: PackageBoxDecodeContext<'de>,
    ) -> Result<(Self, &'de [u8]), BuckyError> {
        Self::raw_decode_with_context(buf, (context, None))
    }
}

impl<'de>
RawDecodeWithContext<
    'de,
    (
        PackageBoxDecodeContext<'de>,
        Option<merge_context::OtherDecode>,
    ),
> for PackageBox
{
    fn raw_decode_with_context(
        buf: &'de [u8],
        c: (
            PackageBoxDecodeContext<'de>,
            Option<merge_context::OtherDecode>,
        ),
    ) -> Result<(Self, &'de [u8]), BuckyError> {
        let (context, merged_values) = c;
        let (mix_hash, hash_buf) = KeyMixHash::raw_decode(buf)?;

        enum KeyStub {
            Exist((DeviceId, DeviceId)),
            Exchange((DeviceId, Vec<u8>))
        }

        struct KeyInfo {
            enc_key: AesKey,
            mix_hash: KeyMixHash,
            stub: KeyStub
        }


        let mut mix_key = None;
        let (key_info, buf) = {
            match context.key_from_mixhash(&mix_hash) {
                Some((local, remote, key)) => {
                    mix_key = Some(key.mix_key);

                    (KeyInfo {
                        stub: KeyStub::Exist((local, remote)),
                        enc_key: key.enc_key,
                        mix_hash
                    }, hash_buf)
                },
                None => {
                    if buf.len() <= 50 {
                        return Err(BuckyError::new(BuckyErrorCode::InvalidData, "invalid data"));
                    }
                    let (confusion, buf) = u16::raw_decode(buf)?;
                    let mut hash = hash_data(confusion.to_le_bytes().as_slice()).as_slice().to_vec();
                    hash.extend(&hash_data(hash.as_slice()).as_slice()[0..16]);
                    let aes_key = AesKey::from(hash);

                    let padded_len = AesKey::padded_len(DeviceId::raw_bytes().unwrap());
                    let mut encrypted_device_id = vec![0u8; padded_len];
                    encrypted_device_id.copy_from_slice(&buf[..padded_len]);
                    let buf = &buf[padded_len..];
                    let len = aes_key.inplace_decrypt(encrypted_device_id.as_mut_slice(), padded_len)?;
                    assert_eq!(len, DeviceId::raw_bytes().unwrap());
                    let device_id = DeviceId::try_from(ObjectId::try_from(encrypted_device_id[0..DeviceId::raw_bytes().unwrap()].to_vec())?)?;

                    let device_secret = context.local_secret(&device_id);
                    if device_secret.is_none() {
                        error!("unkown device {}", device_id.to_string());
                        return Err(BuckyError::new(BuckyErrorCode::InvalidData, "unkown device"));
                    }
                    let device_secret = device_secret.unwrap();
                    let mut enc_key = AesKey::default();
                    let (remain, _) = device_secret.decrypt_aeskey(buf, enc_key.as_mut_slice()).map_err(|e|{
                        error!("decrypt aeskey err={}. (maybe: 1. local/remote device time is not correct 2. the packet is broken 3. the packet not contains Exchange info etc.. )", e);
                        e
                    })?;
                    let encrypted = Vec::from(&buf[..buf.len() - remain.len()]);
                    let (mix_hash, remain) = KeyMixHash::raw_decode(remain)?;
                    (KeyInfo {
                        stub: KeyStub::Exchange((device_id, encrypted)),
                        enc_key,
                        mix_hash,
                    }, remain)
                }
            }
        };

        let mut version = if let KeyStub::Exist((_, remote)) = &key_info.stub {
            context.version_of(remote)
        } else {
            0
        };
        // 把原数据拷贝到context 给的buffer上去
        let decrypt_buf = unsafe { context.decrypt_buf(buf) };
        // 用key 解密数据
        let decrypt_len =  key_info.enc_key.inplace_decrypt(decrypt_buf, buf.len())?;
        let remain_buf = &buf[buf.len()..];
        let decrypt_buf = &decrypt_buf[..decrypt_len];

        let mut packages = vec![];

        //解码所有package
        if decrypt_len != 0 {
            match merged_values {
                Some(mut merged) => {
                    let (package, buf) =
                        DynamicPackage::raw_decode_with_context(decrypt_buf, (&mut merged, &mut version))?;
                    packages.push(package);
                    let mut buf_ptr = buf;
                    while buf_ptr.len() > 0 {
                        match DynamicPackage::raw_decode_with_context(buf_ptr, (&mut merged, &mut version)) {
                            Ok((package, buf)) => {
                                buf_ptr = buf;
                                packages.push(package);
                            },
                            Err(err) => {
                                if err.code() == BuckyErrorCode::NotSupport {
                                    break;
                                } else {
                                    Err(err)?;
                                }
                            }
                        };
                    }
                }
                None => {
                    let mut context = merge_context::FirstDecode::new();
                    let (package, buf) = DynamicPackage::raw_decode_with_context(
                        decrypt_buf[0..decrypt_len].as_ref(),
                        (&mut context, &mut version)
                    )?;
                    packages.push(package);
                    let mut context: merge_context::OtherDecode = context.into();
                    let mut buf_ptr = buf;
                    while buf_ptr.len() > 0 {
                        match DynamicPackage::raw_decode_with_context(buf_ptr, (&mut context, &mut version)) {
                            Ok((package, buf)) => {
                                buf_ptr = buf;
                                packages.push(package);
                            },
                            Err(err) => {
                                if err.code() == BuckyErrorCode::NotSupport {
                                    break;
                                } else {
                                    Err(err)?;
                                }
                            }
                        };
                    }
                }
            }
        }

        if mix_key.is_none() {
            if packages.len() > 0 && packages[0].cmd_code().is_exchange() {
                let exchange: &Exchange = packages[0].as_ref();
                mix_key = Some(exchange.mix_key.clone());
            } else {
                return Err(BuckyError::new(BuckyErrorCode::ErrorState, "unkown mix_key"));
            }
        }

        let key = MixAesKey {
            enc_key: key_info.enc_key,
            mix_key: mix_key.unwrap()
        };
        match key_info.stub {
            KeyStub::Exist((local, remote)) => {
                let mut package_box = PackageBox::encrypt_box(local, remote,key );
                package_box.append(packages);
                Ok((package_box, remain_buf))
            }
            KeyStub::Exchange((local, encrypted)) => {
                if packages.len() > 0 && packages[0].cmd_code().is_exchange() {
                    let exchange: &mut Exchange = packages[0].as_mut();
                    exchange.key_encrypted = encrypted;

                    assert_eq!(exchange.to_device_id, local);

                    let mut package_box =
                        PackageBox::encrypt_box(local, exchange.from_device_desc.desc().device_id(), key);
                    package_box.append(packages);
                    Ok((package_box, remain_buf))
                } else {
                    Err(BuckyError::new(BuckyErrorCode::InvalidData, "unkown from"))
                }
            }
        }
    }
}

pub(crate) struct OtherBoxTcpEncodeContext {
    pub plaintext: bool,
}

impl Default for OtherBoxTcpEncodeContext {
    fn default() -> Self {
        Self {
            plaintext: false,
        }
    }
}
impl RawEncodeWithContext<OtherBoxTcpEncodeContext> for PackageBox {
    fn raw_measure_with_context(
        &self,
        _: &mut OtherBoxTcpEncodeContext,
        _purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<usize> {
        unimplemented!()
    }
    fn raw_encode_with_context<'a>(
        &self,
        buf: &'a mut [u8],
        _context: &mut OtherBoxTcpEncodeContext,
        purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<&'a mut [u8]> {
        let mut encrypt_in_len = buf.len();
        let to_encrypt_buf = buf;

        // 编码所有包
        let mut context = merge_context::FirstEncode::new();
        let packages = self.packages();
        let enc: &dyn RawEncodeWithContext<merge_context::FirstEncode> =
            packages.get(0).unwrap().as_ref();
        let mut buf = enc.raw_encode_with_context(to_encrypt_buf, &mut context, purpose)?;

        let mut context: merge_context::OtherEncode = context.into();
        for p in &packages[1..] {
            let enc: &dyn RawEncodeWithContext<merge_context::OtherEncode> = p.as_ref();
            buf = enc.raw_encode_with_context(buf, &mut context, purpose)?;
        }

        encrypt_in_len -= buf.len();
        // 用aes 加密package的部分
        let len = self.key().enc_key.inplace_encrypt(to_encrypt_buf, encrypt_in_len)?;

        Ok(&mut to_encrypt_buf[len..])
    }
}

pub(crate) type FirstBoxTcpEncodeContext = PackageBoxEncodeContext;
pub(crate) type FirstBoxTcpDecodeContext<'de> = PackageBoxDecodeContext<'de>;

pub(crate) struct OtherBoxTcpDecodeContext<'de> {
    decrypt_buf: DecryptBuffer<'de>,
    local: &'de DeviceId,
    remote: &'de DeviceId,
    key: &'de MixAesKey,
}

impl<'de> OtherBoxTcpDecodeContext<'de> {
    pub fn new_copy(decrypt_buf: &'de mut [u8], local: &'de DeviceId, remote: &'de DeviceId, key: &'de MixAesKey) -> Self {
        Self {
            decrypt_buf: DecryptBuffer::Copy(decrypt_buf),
            local,
            remote,
            key,
        }
    }

    pub fn new_inplace(ptr: *mut u8, len: usize, local: &'de DeviceId, remote: &'de DeviceId, key: &'de MixAesKey) -> Self {
        Self {
            decrypt_buf: DecryptBuffer::Inplace(ptr, len),
            local,
            remote,
            key,
        }
    }

    // 返回用于aes 解码的buffer
    pub unsafe fn decrypt_buf(self, data: &[u8]) -> &'de mut [u8] {
        use DecryptBuffer::*;
        match self.decrypt_buf {
            Copy(decrypt_buf) => {
                decrypt_buf[..data.len()].copy_from_slice(data);
                decrypt_buf
            }
            Inplace(ptr, len) => {
                std::slice::from_raw_parts_mut(ptr.offset((len - data.len()) as isize), data.len())
            }
        }
    }

    pub fn remote(&self) -> &DeviceId {
        self.remote
    }

    pub fn key(&self) -> &MixAesKey {
        self.key
    }

    pub fn local(&self) -> &DeviceId {
        self.local
    }
}

impl<'de> RawDecodeWithContext<'de, OtherBoxTcpDecodeContext<'de>> for PackageBox {
    fn raw_decode_with_context(
        buf: &'de [u8],
        context: OtherBoxTcpDecodeContext<'de>,
    ) -> BuckyResult<(Self, &'de [u8])> {
        let key = context.key().clone();

        let remote = context.remote().clone();
        let local = context.local().clone();

        let decrypt_buf = unsafe { context.decrypt_buf(buf) };
        // 用key 解密数据
        let decrypt_len = key.enc_key.inplace_decrypt(decrypt_buf, buf.len())?;
        let remain_buf = &buf[buf.len()..];
        let decrypt_buf = &decrypt_buf[..decrypt_len];
        let mut packages = vec![];

        {
            let mut context = merge_context::FirstDecode::new();
            let mut version = 0;
            let (package, buf) = DynamicPackage::raw_decode_with_context(
                decrypt_buf[0..decrypt_len].as_ref(),
                (&mut context, &mut version),
            )?;

            packages.push(package);
            let mut context: merge_context::OtherDecode = context.into();
            let mut buf_ptr = buf;
            while buf_ptr.len() > 0 {
                let (package, buf) =
                    DynamicPackage::raw_decode_with_context(buf_ptr, (&mut context, &mut version))?;
                buf_ptr = buf;
                packages.push(package);
            }
        }

        let mut package_box = PackageBox::encrypt_box(local, remote, key);
        package_box.append(packages);
        Ok((package_box, remain_buf))
    }
}

pub(crate) struct PackageBoxTcpEncodeContext<InnerContext>(pub(crate) InnerContext);
pub(crate) struct PackageBoxTcpDecodeContext<InnerContext>(pub(crate) InnerContext);

impl RawEncodeWithContext<PackageBoxTcpEncodeContext<FirstBoxTcpEncodeContext>> for PackageBox {
    fn raw_measure_with_context(
        &self,
        _: &mut PackageBoxTcpEncodeContext<FirstBoxTcpEncodeContext>,
        _purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<usize> {
        unimplemented!()
    }
    fn raw_encode_with_context<'a>(
        &self,
        buf: &'a mut [u8],
        context: &mut PackageBoxTcpEncodeContext<FirstBoxTcpEncodeContext>,
        purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<&'a mut [u8]> {
        let buf_len = buf.len();
        let box_header_len = u16::raw_bytes().unwrap();
        if buf_len < box_header_len {
            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                "buffer not enough".to_string(),
            ));
        }

        info!("packagebox FirstBoxEncodeContext buf_len={} box_header_len={}", buf.len(), box_header_len);

        let box_len = {
            let buf_ptr =
                self.raw_encode_with_context(&mut buf[box_header_len..], &mut context.0, purpose)?;
            buf_len - buf_ptr.len() - box_header_len
        };
        let buf = (box_len as u16).raw_encode(buf, purpose)?;
        Ok(&mut buf[box_len..])
    }
}

impl RawEncodeWithContext<PackageBoxTcpEncodeContext<OtherBoxTcpEncodeContext>> for PackageBox {
    fn raw_measure_with_context(
        &self,
        _: &mut PackageBoxTcpEncodeContext<OtherBoxTcpEncodeContext>,
        _purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<usize> {
        unimplemented!()
    }
    fn raw_encode_with_context<'a>(
        &self,
        buf: &'a mut [u8],
        context: &mut PackageBoxTcpEncodeContext<OtherBoxTcpEncodeContext>,
        purpose: &Option<RawEncodePurpose>,
    ) -> BuckyResult<&'a mut [u8]> {
        let buf_len = buf.len();
        let box_header_len = u16::raw_bytes().unwrap();
        if buf_len < box_header_len {
            return Err(BuckyError::new(
                BuckyErrorCode::OutOfLimit,
                "buffer not enough",
            ));
        }

        info!("packagebox FirstBoxEncodeContext buf_len={} box_header_len={}", buf.len(), box_header_len);

        let box_len = {
            let buf_ptr =
                self.raw_encode_with_context(&mut buf[box_header_len..], &mut context.0, purpose)?;
            buf_len - buf_ptr.len() - box_header_len
        };
        let buf = (box_len as u16).raw_encode(buf, purpose)?;
        Ok(&mut buf[box_len..])
    }
}

impl<'de> RawDecodeWithContext<'de, PackageBoxTcpDecodeContext<FirstBoxTcpDecodeContext<'de>>>
for PackageBox
{
    fn raw_decode_with_context(
        buf: &'de [u8],
        context: PackageBoxTcpDecodeContext<FirstBoxTcpDecodeContext<'de>>,
    ) -> BuckyResult<(Self, &'de [u8])> {
        let (_box_len, buf) = u16::raw_decode(buf)?;
        PackageBox::raw_decode_with_context(buf, context.0)
    }
}

impl<'de> RawDecodeWithContext<'de, PackageBoxTcpDecodeContext<OtherBoxTcpDecodeContext<'de>>>
for PackageBox
{
    fn raw_decode_with_context(
        buf: &'de [u8],
        context: PackageBoxTcpDecodeContext<OtherBoxTcpDecodeContext<'de>>,
    ) -> BuckyResult<(Self, &'de [u8])> {
        let (_box_len, buf) = u16::raw_decode(buf)?;
        PackageBox::raw_decode_with_context(buf, context.0)
    }
}
