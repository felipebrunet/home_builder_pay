//! Public name discovery so two machines need not paste onions.
//!
//! Kademlia only works after at least one shared peer. This layer publishes
//! `{work name → onion}` to public HTTPS topic boards **through Tor SOCKS**
//! (ntfy.sh, then ntfy.envs.net). Anyone who knows the work name can poll
//! the same topic. Onion paste stays as an advanced fallback.
//!
//! Topics are a hash of the normalized name — not a secret. Do not put
//! private keys here.

use std::net::SocketAddr;

use sha2::{Digest, Sha256};

use crate::dht::{normalize_work_name, WorkAnnounce};
use crate::http::{get_text, put_text};

const NTFY_HOSTS: &[&str] = &["https://ntfy.sh", "https://ntfy.envs.net"];

pub fn rendezvous_topic(work_name: &str) -> String {
    let n = normalize_work_name(work_name);
    let h = Sha256::digest(format!("hbp-rendezvous-v1:{n}").as_bytes());
    format!("hbp{}", hex::encode(&h[..12]))
}

pub fn publish_announce(socks: Option<SocketAddr>, ann: &WorkAnnounce) -> crate::Result<String> {
    let body = serde_json::to_string(ann)?;
    let mut topics = vec![rendezvous_topic(&ann.work_name)];
    if !ann.person_name.trim().is_empty() {
        topics.push(rendezvous_topic(&ann.person_name));
    }
    let mut last = crate::Error::msg("no rendezvous host");
    let mut ok = None;
    for topic in topics {
        for host in NTFY_HOSTS {
            let url = format!("{host}/{topic}");
            match put_text(socks, &url, &body) {
                Ok(_) => {
                    ok = Some(format!("{host}/{topic}"));
                    break;
                }
                Err(e) => last = e,
            }
        }
    }
    ok.ok_or(last)
}

pub fn lookup_announce(
    socks: Option<SocketAddr>,
    work_name: &str,
) -> crate::Result<Option<WorkAnnounce>> {
    let topic = rendezvous_topic(work_name);
    let mut last = crate::Error::msg("no rendezvous host");
    for host in NTFY_HOSTS {
        let url = format!("{host}/{topic}/json?poll=1");
        match get_text(socks, &url) {
            Ok(raw) => {
                if let Some(ann) = latest_announce(&raw) {
                    return Ok(Some(ann));
                }
            }
            Err(e) => last = e,
        }
    }
    if last.to_string() == "no rendezvous host" {
        Ok(None)
    } else {
        // Offline / blocked: not a hard error — caller falls back to DHT / paste.
        Ok(None)
    }
}

/// ntfy poll is one JSON object per line (`event: message`).
pub fn latest_announce(raw: &str) -> Option<WorkAnnounce> {
    let mut found = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(ann) = serde_json::from_str::<WorkAnnounce>(line) {
            if !ann.onion.trim().is_empty() {
                found = Some(ann);
                continue;
            }
        }
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
        if let Ok(ann) = serde_json::from_str::<WorkAnnounce>(msg) {
            if !ann.onion.trim().is_empty() {
                found = Some(ann);
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_is_stable_and_normalized() {
        assert_eq!(
            rendezvous_topic("Casa Norte"),
            rendezvous_topic("  casa   NORTE ")
        );
        assert!(rendezvous_topic("Casa Norte").starts_with("hbp"));
        assert_ne!(rendezvous_topic("Casa Norte"), rendezvous_topic("Casa Sur"));
    }

    #[test]
    fn parses_ntfy_poll_and_bare_json() {
        let ann = WorkAnnounce {
            work_name: "Casa Norte".into(),
            onion: "abc.onion".into(),
            offer_id: None,
            role: "mandante".into(),
            person_name: "Don José".into(),
        };
        let bare = serde_json::to_string(&ann).unwrap();
        assert_eq!(latest_announce(&bare).unwrap().onion, "abc.onion");
        let wrapped = format!(
            "{{\"id\":\"1\",\"event\":\"message\",\"message\":{}}}",
            serde_json::to_string(&bare).unwrap()
        );
        assert_eq!(latest_announce(&wrapped).unwrap().onion, "abc.onion");
        assert!(latest_announce("").is_none());
    }
}
