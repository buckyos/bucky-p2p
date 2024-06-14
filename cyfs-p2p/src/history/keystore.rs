use std::collections::{HashMap, LinkedList};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::sync::{Arc, RwLock};
use std::cell::RefCell;
use std::rc::Rc;
use bucky_crypto::{AesKey, HashValue, KeyMixHash, PrivateKey, PublicKey};
use bucky_objects::{DeviceDesc, DeviceId, RsaCPUObjectSigner, SingleKeyObjectDesc};
use sha2::{Digest, Sha256};
use crate::{
    types::*
};
use crate::error::{BdtError, BdtErrorCode, BdtResult};


const MIX_HASH_LIVE_MINUTES: usize = 31;

pub struct Keystore {
    local_encryptor: RwLock<HashMap<DeviceId, (PrivateKey, DeviceDesc)>>,
    key_manager: KeyManager,
}

#[derive(Clone)]
pub enum EncryptedKey {
    None,
    Confirmed(Vec<u8>),
    Unconfirmed(Vec<u8>)
}

impl EncryptedKey {
    pub fn is_local(&self) -> bool {
        match self {
            Self::None => false,
            _ => true
        }
    }

    pub fn is_unconfirmed(&self) -> bool {
        match self {
            Self::Unconfirmed(_) => true,
            _ => false
        }
    }
}
pub struct FoundKey {
    pub local_id: DeviceId,
    pub remote_id: DeviceId,
    pub key: MixAesKey,
    pub encrypted: EncryptedKey,
}

#[derive(Clone)]
pub struct Config {
    pub active_time: Duration,
    pub key_expire: Duration,
    pub capacity: usize,
}

fn device_pair_hash(local_id: &DeviceId, remote_id: &DeviceId) -> HashValue {
    let mut sha = Sha256::new();
    sha.input(local_id.object_id().as_slice());
    sha.input(remote_id.object_id().as_slice());
    sha.result().into()
}

impl Keystore {
    pub fn new(config: Config) -> Self {
        Keystore {
            local_encryptor: RwLock::new(HashMap::new()),
            key_manager: KeyManager::new(config),
        }
    }

    pub fn add_local_key(&self, device_id: DeviceId, private_key: PrivateKey, device_info: DeviceDesc) {
        self.local_encryptor.write().unwrap().insert(device_id, (private_key, device_info));
    }

    pub fn private_key(&self, device_id: &DeviceId) -> Option<PrivateKey> {
        self.local_encryptor.read().unwrap().get(device_id).map(|v| v.0.clone())
    }

    pub fn signer(&self, device_id: &DeviceId) -> Option<RsaCPUObjectSigner> {
        self.local_encryptor.read().unwrap().get(device_id).map(|v| RsaCPUObjectSigner::new(v.1.public_key().clone(), v.0.clone()))
    }

    pub fn public_key(&self, device_id: &DeviceId) -> Option<PublicKey> {
        self.local_encryptor.read().unwrap().get(device_id).map(|v| v.1.public_key().clone())
    }

    pub fn get_key_by_remote(&self, local_id: &DeviceId, remote_id: &DeviceId) -> Option<FoundKey> {
        self.key_manager.find_by_peerid(&device_pair_hash(local_id, remote_id)).map(|found| found.found())
    }

    pub fn get_key_by_mix_hash(&self, mix_hash: &KeyMixHash) -> Option<FoundKey> {
        self.key_manager.find_by_mix_hash(mix_hash).map(|found| found.found())
    }

    pub fn create_key(&self, local_id: &DeviceId, peer_desc: &DeviceDesc) -> FoundKey {
        let pair_hash = device_pair_hash(local_id, &peer_desc.device_id());
        match self.key_manager.find_by_peerid(&pair_hash) {
            Some(found) => found.found(),
            None => {
                let (enc_key, encrypted) = peer_desc.public_key().gen_aeskey_and_encrypt().unwrap();
                let mix_key = AesKey::random();
                let encrypted = EncryptedKey::Unconfirmed(encrypted);
                let key = MixAesKey {
                    enc_key,
                    mix_key,
                };
                self.key_manager.add_key(&key, local_id, &peer_desc.device_id(), encrypted.clone());
                FoundKey {
                    local_id: local_id.clone(),
                    key,
                    remote_id: peer_desc.device_id(),
                    encrypted
                }
            }
        }
    }

    pub fn add_key(&self, key: &MixAesKey, local_id: &DeviceId, remote_id: &DeviceId, encrypted: EncryptedKey)  {
        self.key_manager.add_key(key, local_id, remote_id, encrypted)
    }

    pub fn reset_peer(&self, local_id: &DeviceId, device_id: &DeviceId) {
        info!("keystore reset local {} peer {}", local_id, device_id);
        self.key_manager.reset_peer(&device_pair_hash(local_id, device_id));
    }
}

struct KeyManager {
    // 按peer pair hash索引的key
    peerid_key_map: mini_moka::sync::Cache<HashValue, Arc<HashedKeyInfo>>,
    // 按hash索引的key
    mixhash_key_map: mini_moka::sync::Cache<KeyMixHash, Arc<HashedKeyInfo>>,

    config: Config,
}

impl KeyManager {
    fn new(config: Config) -> Self {
        KeyManager {
            peerid_key_map: mini_moka::sync::CacheBuilder::new(config.capacity as u64).time_to_live(config.active_time).build(),
            mixhash_key_map: mini_moka::sync::CacheBuilder::new(config.capacity as u64 * 20).time_to_idle(config.active_time).build(),
            config
        }
    }

    fn find_by_mix_hash(&self, mix_hash: &KeyMixHash) -> Option<Arc<HashedKeyInfo>> {
        let found_map = self.mixhash_key_map.get(mix_hash);
        found_map
    }

    fn find_by_peerid(&self, peerid: &HashValue) -> Option<Arc<HashedKeyInfo>> {
        let found_map = self.peerid_key_map.get(peerid);
        found_map
    }

    fn add_key(&self, key: &MixAesKey, local_id: &DeviceId, remote_id: &DeviceId, encrypted: EncryptedKey) {
        let pair_hash = device_pair_hash(local_id, remote_id);

        let new_key = KeyInfo {
            key: key.clone(),
            local_id: local_id.clone(),
            remote_id: remote_id.clone(),
            encrypted,
        };
        let new_info = Arc::new(HashedKeyInfo {
            info: new_key,
            original_hash: key.mix_key.mix_hash(None),
        });

        self.peerid_key_map.insert(pair_hash, new_info.clone());
        self.mixhash_key_map.insert(key.mix_hash(local_id, remote_id), new_info.clone());
    }

    fn remove_key(&self, key: &HashedKeyInfo) {
        let pair_hash = device_pair_hash(&key.info.local_id, &key.info.remote_id);
        self.peerid_key_map.invalidate(&pair_hash);
    }

    fn reset_peer(&self, device_id: &HashValue) {
        self.peerid_key_map.invalidate(device_id);
    }
}

struct HashedKeyInfo {
    info: KeyInfo,
    original_hash: KeyMixHash,
}

impl HashedKeyInfo {
    fn found(&self) -> FoundKey {
        FoundKey {
            local_id: self.info.local_id.clone(),
            key: self.info.key.clone(),
            remote_id: self.info.remote_id.clone(),
            encrypted: self.info.encrypted.clone(),
        }
    }
}

struct KeyInfo {
    key: MixAesKey,
    local_id: DeviceId,
    remote_id: DeviceId,
    encrypted: EncryptedKey,
}

#[derive(Clone)]
struct HashInfo {
    hash: KeyMixHash,
    minute_timestamp: u64,
}


// #[test]
// fn add_key() {
//     use std::time::Duration;
//     // 单一peer的key状态管理测试程序
//     let private_key = PrivateKey::generate_rsa(1024).unwrap();
//     let device = Device::new(
//         None,
//         UniqueId::default(),
//         vec![],
//         vec![],
//         vec![],
//         private_key.public(),
//         Area::default(),
//         DeviceCategory::PC
//     ).build();
//     let sim_device_id = device.desc().device_id();

//     let signer = RsaCPUObjectSigner::new(
//         private_key.public(),
//         private_key.clone(),
//     );

//     let key_store = Keystore::new(
//         private_key.clone(),
//         device.desc().clone(),
//         signer,
//         Config {
//             active_time: Duration::from_secs(1),
//             capacity: 5,
//         });

//     let key_for_id0_first = key_store.create_key(device.desc(), true);
//     assert!(key_for_id0_first.encrypted.is_unconfirmed());
//     assert_eq!(key_for_id0_first.peerid, sim_device_id);

//     fn found_key_is_same(left: &FoundKey, right: &FoundKey) -> bool {
//         left.enc_key == right.enc_key &&
//             left.peerid == right.peerid &&
//             left.hash == right.hash // <TODO>启用加盐hash后需要修改
//     }
//     assert!(found_key_is_same(&key_store.get_key_by_remote(&sim_device_id, true).unwrap(), &key_for_id0_first));
//     assert!(found_key_is_same(&key_store.get_key_by_mix_hash(&key_for_id0_first.hash, true, false).unwrap(), &key_for_id0_first));

//     let key_for_id0_twice = key_store.create_key(device.desc(), true); // 不重复构造key
//     assert!(key_for_id0_twice.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&key_for_id0_twice, &key_for_id0_first));

//     let found_by_hash = key_store.get_key_by_mix_hash(&key_for_id0_first.hash, true, true).unwrap(); // confirm: false->true
//     assert!(!found_by_hash.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&found_by_hash, &key_for_id0_first));

//     let found_by_hash = key_store.get_key_by_mix_hash(&key_for_id0_first.hash, true, false).unwrap(); // confirm不能从true->false
//     assert!(!found_by_hash.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&found_by_hash, &key_for_id0_first));

//     let (key_random, key_encrypted) = private_key.public().gen_aeskey_and_encrypt().unwrap();
//     let mix_key = AesKey::random();
//     let found_key_for_random = FoundKey {
//         enc_key: key_random.clone(),
//         hash: key_random.mix_hash(None),
//         peerid: sim_device_id.clone(),
//         encrypted: EncryptedKey::Unconfirmed(key_encrypted),
//         mix_key
//     };
//     key_store.add_key(&key_random, &sim_device_id);
//     let found_after_add = key_store.get_key_by_remote(&sim_device_id, true).unwrap();
//     assert!(found_key_is_same(&found_after_add, &key_for_id0_first) || found_key_is_same(&found_after_add, &found_key_for_random)); // 没有明显的时间先后，不能确定返回哪个
//     let found_by_hash_after_add = key_store.get_key_by_mix_hash(&found_key_for_random.hash, true, false).unwrap();
//     assert!(!found_by_hash_after_add.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&found_by_hash_after_add, &found_key_for_random));

//     key_store.add_key(&key_random, &sim_device_id); // confirm: false->true
//     let found_by_hash_after_add_with_confirm = key_store.get_key_by_mix_hash(&found_key_for_random.hash, true, false).unwrap();
//     assert!(!found_by_hash_after_add_with_confirm.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&found_by_hash_after_add_with_confirm, &found_key_for_random));

//     let (key_random2, key_encrypted2) = private_key.public().gen_aeskey_and_encrypt().unwrap();
//     let found_key_for_random2 = FoundKey {
//         enc_key: key_random2.clone(),
//         hash: key_random2.mix_hash(None),
//         peerid: sim_device_id.clone(),
//         encrypted: EncryptedKey::Unconfirmed(key_encrypted2),
//         mix_key: mix_key2,
//     };
//     key_store.add_key(&key_random2, &sim_device_id); // 直接在add里confirm
//     let found_by_hash_after_add2_with_confirm = key_store.get_key_by_mix_hash(&found_key_for_random2.hash, true, false).unwrap();
//     assert!(!found_by_hash_after_add2_with_confirm.encrypted.is_unconfirmed());
//     assert!(found_key_is_same(&found_by_hash_after_add2_with_confirm, &found_key_for_random2));
// }
