//! Public name discovery so two machines need not paste onions.
//!
//! Kademlia only works after at least one shared peer. This layer publishes
//! `{person + obra → onion}` to public HTTPS topic boards **through Tor SOCKS**
//! (ntfy.sh, then ntfy.envs.net).
//!
//! Persona search is the product path. Topics:
//! - literal `hbpn-{normalized}` (e.g. `hbpn-felipe`) — primary, debuggable
//! - hashed `hbp` + SHA-256(`hbp-rendezvous-v1:{normalized}`)[:12]
//! - directory `hbpn-dir-v1` — payload scan by `person_name` or `work_name`
//!
//! Onion paste stays as an advanced fallback. Topics are not a secret.

use std::net::SocketAddr;

use sha2::{Digest, Sha256};

use crate::dht::{normalize_work_name, WorkAnnounce};
use crate::http::{get_text, put_text};

const NTFY_HOSTS: &[&str] = &["https://ntfy.sh", "https://ntfy.envs.net"];

/// Shared board: every announce JSON, looked up by payload (persona or obra).
pub const DIRECTORY_TOPIC: &str = "hbpn-dir-v1";

pub fn rendezvous_topic(name: &str) -> String {
    let n = normalize_work_name(name);
    let h = Sha256::digest(format!("hbp-rendezvous-v1:{n}").as_bytes());
    format!("hbp{}", hex::encode(&h[..12]))
}

/// Human-readable ntfy topic for a persona or obra. `Felipe` → `hbpn-felipe`.
pub fn literal_topic(name: &str) -> String {
    let n = normalize_work_name(name);
    let safe: String = n
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let safe = safe.trim_matches('-').to_string();
    format!("hbpn-{safe}")
}

pub fn announce_matches_query(ann: &WorkAnnounce, query: &str) -> bool {
    let q = normalize_work_name(query);
    if q.is_empty() {
        return false;
    }
    normalize_work_name(&ann.person_name) == q || normalize_work_name(&ann.work_name) == q
}

/// Topics we PUT. Persona channels first; directory last (payload fallback).
pub fn announce_topics(ann: &WorkAnnounce) -> Vec<String> {
    let mut topics = Vec::new();
    if !ann.person_name.trim().is_empty() {
        topics.push(literal_topic(&ann.person_name));
        topics.push(rendezvous_topic(&ann.person_name));
    }
    if !ann.work_name.trim().is_empty() {
        let lit = literal_topic(&ann.work_name);
        let hashed = rendezvous_topic(&ann.work_name);
        if !topics.contains(&lit) {
            topics.push(lit);
        }
        if !topics.contains(&hashed) {
            topics.push(hashed);
        }
    }
    topics.push(DIRECTORY_TOPIC.to_string());
    topics
}

/// Topics we GET for a Buscar query (persona or obra string).
pub fn lookup_topics(query: &str) -> Vec<String> {
    let mut topics = vec![literal_topic(query), rendezvous_topic(query)];
    topics.dedup();
    topics.push(DIRECTORY_TOPIC.to_string());
    topics
}

fn persona_channel_count(ann: &WorkAnnounce) -> usize {
    if ann.person_name.trim().is_empty() {
        0
    } else {
        2
    }
}

pub fn publish_announce(socks: Option<SocketAddr>, ann: &WorkAnnounce) -> crate::Result<String> {
    let body = serde_json::to_string(ann)?;
    let topics = announce_topics(ann);
    let persona_n = persona_channel_count(ann);
    let mut last = crate::Error::msg("no rendezvous host");
    let mut persona_ok = None;
    let mut directory_ok = None;
    let mut any_ok = None;
    for (i, topic) in topics.iter().enumerate() {
        let mut topic_ok = None;
        for host in NTFY_HOSTS {
            let url = format!("{host}/{topic}");
            match put_text(socks, &url, &body) {
                Ok(_) => {
                    topic_ok = Some(url);
                    break;
                }
                Err(e) => last = e,
            }
        }
        if let Some(url) = topic_ok {
            any_ok = Some(url.clone());
            if i < persona_n {
                if persona_ok.is_none() {
                    persona_ok = Some(url.clone());
                }
            }
            if topic == DIRECTORY_TOPIC {
                directory_ok = Some(url);
            }
        }
    }
    if persona_n > 0 {
        return persona_ok.or(directory_ok).ok_or(last);
    }
    any_ok.ok_or(last)
}

pub fn lookup_announce(
    socks: Option<SocketAddr>,
    query: &str,
) -> crate::Result<Option<WorkAnnounce>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    let mut last = crate::Error::msg("no rendezvous host");
    for topic in lookup_topics(query) {
        for host in NTFY_HOSTS {
            let url = format!("{host}/{topic}/json?poll=1");
            match get_text(socks, &url) {
                Ok(raw) => {
                    if let Some(ann) = pick_announce(&raw, query) {
                        return Ok(Some(ann));
                    }
                }
                Err(e) => last = e,
            }
        }
    }
    if last.to_string() == "no rendezvous host" {
        Ok(None)
    } else {
        Ok(None)
    }
}

/// Prefer a payload that matches the query (persona or obra). Else latest onion.
pub fn pick_announce(raw: &str, query: &str) -> Option<WorkAnnounce> {
    let all = announces_from_ntfy(raw);
    all.iter()
        .rev()
        .find(|a| announce_matches_query(a, query))
        .cloned()
        .or_else(|| all.into_iter().rev().find(|a| !a.onion.trim().is_empty()))
}

pub fn latest_announce(raw: &str) -> Option<WorkAnnounce> {
    announces_from_ntfy(raw).into_iter().rev().next()
}

/// ntfy poll is one JSON object per line (`event: open` then `event: message`).
/// A bad line must not drop later announces.
pub fn announces_from_ntfy(raw: &str) -> Vec<WorkAnnounce> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(ann) = serde_json::from_str::<WorkAnnounce>(line) {
            if !ann.onion.trim().is_empty() {
                out.push(ann);
                continue;
            }
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("");
            if let Ok(ann) = serde_json::from_str::<WorkAnnounce>(msg) {
                if !ann.onion.trim().is_empty() {
                    out.push(ann);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn casa2_felipe() -> WorkAnnounce {
        WorkAnnounce {
            work_name: "casa2".into(),
            onion: "abc.onion".into(),
            offer_id: None,
            role: "mandante".into(),
            person_name: "Felipe".into(),
        }
    }

    #[test]
    fn topic_is_stable_and_normalized() {
        assert_eq!(
            rendezvous_topic("Casa Norte"),
            rendezvous_topic("  casa   NORTE ")
        );
        assert!(rendezvous_topic("Casa Norte").starts_with("hbp"));
        assert_ne!(rendezvous_topic("Casa Norte"), rendezvous_topic("Casa Sur"));
        assert_ne!(rendezvous_topic("Felipe"), rendezvous_topic("casa2"));
    }

    #[test]
    fn literal_persona_topic_is_hbpn_felipe() {
        assert_eq!(literal_topic("Felipe"), "hbpn-felipe");
        assert_eq!(literal_topic("  FELIPE "), "hbpn-felipe");
        assert_eq!(literal_topic("casa2"), "hbpn-casa2");
        assert_ne!(literal_topic("Felipe"), literal_topic("casa2"));
        let topics = lookup_topics("Felipe");
        assert_eq!(topics[0], "hbpn-felipe");
        assert!(topics.contains(&DIRECTORY_TOPIC.to_string()));
    }

    #[test]
    fn announce_topics_put_persona_before_obra() {
        let topics = announce_topics(&casa2_felipe());
        assert_eq!(topics[0], "hbpn-felipe");
        assert_eq!(topics[1], rendezvous_topic("Felipe"));
        assert!(topics.contains(&literal_topic("casa2")));
        assert_eq!(topics.last().map(String::as_str), Some(DIRECTORY_TOPIC));
    }

    #[test]
    fn discover_felipe_matches_casa2_announce() {
        let ann = casa2_felipe();
        assert!(announce_matches_query(&ann, "Felipe"));
        assert!(announce_matches_query(&ann, "felipe"));
        assert!(announce_matches_query(&ann, "  FELIPE "));
        assert!(announce_matches_query(&ann, "casa2"));
        assert!(!announce_matches_query(&ann, "casa3"));
        let wrapped = format!(
            "{{\"id\":\"1\",\"event\":\"open\"}}\n{{\"id\":\"2\",\"event\":\"message\",\"message\":{}}}",
            serde_json::to_string(&serde_json::to_string(&ann).unwrap()).unwrap()
        );
        let found = pick_announce(&wrapped, "Felipe").expect("persona query");
        assert_eq!(found.work_name, "casa2");
        assert_eq!(found.person_name, "Felipe");
        assert_eq!(pick_announce(&wrapped, "casa2").unwrap().onion, "abc.onion");
    }

    #[test]
    fn parses_ntfy_poll_and_bare_json() {
        let ann = casa2_felipe();
        let bare = serde_json::to_string(&ann).unwrap();
        assert_eq!(latest_announce(&bare).unwrap().onion, "abc.onion");
        let wrapped = format!(
            "{{\"id\":\"1\",\"event\":\"message\",\"message\":{}}}",
            serde_json::to_string(&bare).unwrap()
        );
        assert_eq!(latest_announce(&wrapped).unwrap().onion, "abc.onion");
        assert!(latest_announce("").is_none());
        assert!(latest_announce("{\"event\":\"open\"}").is_none());
    }
}
