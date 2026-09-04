//! Shared DHT types. The live overlay lives in [`crate::overlay`].

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::wire::PeerAddr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhtRecord {
    pub key: String,
    pub value: Vec<u8>,
    pub publisher: Option<PeerAddr>,
    pub ttl_secs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkAnnounce {
    pub work_name: String,
    pub onion: String,
    pub offer_id: Option<String>,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: String,
    pub addr: PeerAddr,
}

pub fn dht_key(topic: &str) -> [u8; 32] {
    Sha256::digest(topic.as_bytes()).into()
}

pub fn work_topic_key(work_name: &str) -> [u8; 32] {
    dht_key(&format!("hbp-work:{work_name}"))
}

pub fn xor_distance(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut d = [0u8; 32];
    for i in 0..32 {
        d[i] = a[i] ^ b[i];
    }
    d
}

pub fn parse_node_id(hex_str: &str) -> crate::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim())?;
    bytes
        .try_into()
        .map_err(|_| crate::Error::msg("node_id must be 32 bytes hex"))
}
