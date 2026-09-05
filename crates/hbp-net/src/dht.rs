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
    /// Mandante display name (“Don José”). Empty on old records.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub person_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub node_id: String,
    pub addr: PeerAddr,
}

pub fn dht_key(topic: &str) -> [u8; 32] {
    Sha256::digest(topic.as_bytes()).into()
}

/// Lowercase, collapse whitespace. Same string both sides of announce/lookup.
pub fn normalize_work_name(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn work_topic_key(work_name: &str) -> [u8; 32] {
    let n = normalize_work_name(work_name);
    dht_key(&format!("hbp-work:{n}"))
}

/// Directory key: contratista looks up the mandante by their display name.
pub fn person_topic_key(person_name: &str) -> [u8; 32] {
    let n = normalize_work_name(person_name);
    dht_key(&format!("hbp-person:{n}"))
}

pub fn xor_distance(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut d = [0u8; 32];
    for i in 0..32 {
        d[i] = a[i] ^ b[i];
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_key_ignores_case_and_spaces() {
        assert_eq!(
            work_topic_key("Casa Norte"),
            work_topic_key("  casa   NORTE ")
        );
        assert_ne!(work_topic_key("Casa Norte"), work_topic_key("Casa Sur"));
        assert_eq!(
            person_topic_key("Don José"),
            person_topic_key("  don   josé ")
        );
        assert_ne!(person_topic_key("Don José"), work_topic_key("Don José"));
        let old = r#"{"work_name":"Casa","onion":"a.onion","offer_id":null,"role":"mandante"}"#;
        let a: WorkAnnounce = serde_json::from_str(old).unwrap();
        assert!(a.person_name.is_empty());
    }
}

pub fn parse_node_id(hex_str: &str) -> crate::Result<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim())?;
    bytes
        .try_into()
        .map_err(|_| crate::Error::msg("node_id must be 32 bytes hex"))
}
