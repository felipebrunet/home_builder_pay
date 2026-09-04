//! Discovery DHT (scaffolding).
//!
//! Mainline BitTorrent DHT is UDP. Tor (as used here) is a TCP SOCKS proxy.
//! A production overlay must therefore be a **TCP Kademlia** (or onion
//! service directory) — not raw mainline. This module is that API:
//!
//! - [`dht_key`] = SHA-256 of a topic (`hbp-work:{name}` / offer id)
//! - [`DhtNode`] is an in-process routing table + store
//! - [`DhtNode::merge`] is how two laptops would sync over Tor later
//!
//! Ready: types, put/get, work announce, tests.
//! Not ready: WAN replication, peer ping, NAT, or a daemon.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhtRecord {
    pub key: String,
    pub value: Vec<u8>,
    pub publisher_onion: Option<String>,
    pub ttl_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAnnounce {
    pub work_name: String,
    pub onion: String,
    pub offer_id: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct DhtNode {
    pub node_id: [u8; 32],
    store: std::collections::BTreeMap<[u8; 32], DhtRecord>,
}

impl DhtNode {
    pub fn new(node_id: [u8; 32]) -> Self {
        Self {
            node_id,
            store: std::collections::BTreeMap::new(),
        }
    }

    pub fn put(&mut self, key: [u8; 32], record: DhtRecord) {
        self.store.insert(key, record);
    }

    pub fn get(&self, key: &[u8; 32]) -> Option<&DhtRecord> {
        self.store.get(key)
    }

    pub fn announce_work(&mut self, ann: &WorkAnnounce) -> crate::Result<[u8; 32]> {
        let key = dht_key(&format!("hbp-work:{}", ann.work_name));
        let value = serde_json::to_vec(ann)?;
        self.put(
            key,
            DhtRecord {
                key: hex::encode(key),
                value,
                publisher_onion: Some(ann.onion.clone()),
                ttl_secs: 3_600,
            },
        );
        Ok(key)
    }

    pub fn lookup_work(&self, work_name: &str) -> crate::Result<Option<WorkAnnounce>> {
        let key = dht_key(&format!("hbp-work:{work_name}"));
        match self.get(&key) {
            None => Ok(None),
            Some(rec) => Ok(Some(serde_json::from_slice(&rec.value)?)),
        }
    }

    pub fn announce_offer_blob(&mut self, offer_id: &str, blob: &[u8], onion: &str) -> [u8; 32] {
        let key = dht_key(&format!("hbp-offer:{offer_id}"));
        self.put(
            key,
            DhtRecord {
                key: hex::encode(key),
                value: blob.to_vec(),
                publisher_onion: Some(onion.to_string()),
                ttl_secs: 86_400,
            },
        );
        key
    }

    /// Merge another node's store (the future Tor sync primitive).
    pub fn merge(&mut self, other: &DhtNode) {
        for (k, v) in &other.store {
            self.store.insert(*k, v.clone());
        }
    }

    pub fn len(&self) -> usize {
        self.store.len()
    }
}

pub fn dht_key(topic: &str) -> [u8; 32] {
    Sha256::digest(topic.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announce_and_lookup_roundtrip() {
        let mut a = DhtNode::new([1u8; 32]);
        let mut b = DhtNode::new([2u8; 32]);
        let ann = WorkAnnounce {
            work_name: "Casa Norte".into(),
            onion: "abc.onion".into(),
            offer_id: Some("deadbeef".into()),
            role: "mandante".into(),
        };
        a.announce_work(&ann).unwrap();
        assert!(b.lookup_work("Casa Norte").unwrap().is_none());
        b.merge(&a);
        let found = b.lookup_work("Casa Norte").unwrap().unwrap();
        assert_eq!(found.onion, "abc.onion");
        assert_eq!(found.offer_id.as_deref(), Some("deadbeef"));
    }
}
