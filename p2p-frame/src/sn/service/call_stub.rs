use crate::p2p_identity::P2pId;
use crate::types::{Timestamp, TunnelId};
use bucky_time::bucky_time_now;
use std::sync::Mutex;
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RemoteSeq {
    remote: P2pId,
    seq: TunnelId,
}

struct StubImpl {
    last_recycle: Timestamp,
    stubs: BTreeMap<RemoteSeq, Timestamp>,
}
pub struct CallStub(Mutex<StubImpl>);

impl CallStub {
    pub fn new() -> Self {
        Self(Mutex::new(StubImpl {
            last_recycle: bucky_time_now(),
            stubs: Default::default(),
        }))
    }

    pub fn insert(&self, remote: &P2pId, seq: &TunnelId) -> bool {
        let remote_seq = RemoteSeq {
            remote: remote.clone(),
            seq: *seq,
        };
        let stubs = &mut self.0.lock().unwrap().stubs;
        stubs.insert(remote_seq, bucky_time_now()).is_none()
    }

    pub fn recycle(&self, now: Timestamp) {
        let mut stub = self.0.lock().unwrap();
        if now > stub.last_recycle
            && Duration::from_micros(now - stub.last_recycle) > Duration::from_secs(60)
        {
            let mut to_remove = vec![];
            for (key, when) in stub.stubs.iter() {
                if now > *when && Duration::from_micros(now - *when) > Duration::from_secs(60) {
                    to_remove.push(key.clone());
                }
            }
            for key in to_remove {
                stub.stubs.remove(&key);
            }
            stub.last_recycle = bucky_time_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_remote(seed: u8) -> P2pId {
        P2pId::from(vec![seed; 32])
    }

    #[test]
    fn test_insert_dedup_by_remote_and_seq() {
        let call_stub = CallStub::new();
        let remote = test_remote(1);
        let seq = TunnelId::from(100);

        assert!(call_stub.insert(&remote, &seq));
        assert!(!call_stub.insert(&remote, &seq));
    }

    #[test]
    fn test_recycle_removes_entries_older_than_60_seconds() {
        let call_stub = CallStub::new();
        let remote = test_remote(2);
        let seq = TunnelId::from(200);

        {
            let mut stub = call_stub.0.lock().unwrap();
            stub.last_recycle = 0;
            stub.stubs.insert(
                RemoteSeq {
                    remote: remote.clone(),
                    seq,
                },
                0,
            );
        }

        let now = Duration::from_secs(61).as_micros() as Timestamp;
        call_stub.recycle(now);

        assert!(call_stub.insert(&remote, &seq));
    }

    #[test]
    fn test_recycle_keeps_entries_at_60_seconds_boundary() {
        let call_stub = CallStub::new();
        let remote = test_remote(3);
        let seq = TunnelId::from(300);

        {
            let mut stub = call_stub.0.lock().unwrap();
            stub.last_recycle = 0;
            stub.stubs.insert(
                RemoteSeq {
                    remote: remote.clone(),
                    seq,
                },
                0,
            );
        }

        let now = Duration::from_secs(60).as_micros() as Timestamp;
        call_stub.recycle(now);

        assert!(!call_stub.insert(&remote, &seq));
    }
}
