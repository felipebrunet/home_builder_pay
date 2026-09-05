//! Tor point-to-point and a **TCP Kademlia DHT** for Windows v1.
//!
//! Same JSON the file protocol already speaks ([`NetMessage`]). Transport is
//! SOCKS5 to `.onion` (or direct TCP for localhost tests). There is no
//! application server.
//!
//! Mainline DHT is UDP and cannot ride Tor SOCKS. This crate speaks its own
//! Kademlia (`PING` / `FIND_NODE` / `FIND_VALUE` / `STORE` / `DELIVER`) on
//! TCP. Two machines that share a bootstrap address can find each other.
//! There is not yet a public HBP bootstrap cloud — see [docs/NETWORK.md].

mod bundle;
mod dht;
mod fx;
mod http;
mod message;
mod overlay;
mod rendezvous;
mod tor;
mod wire;

pub use bundle::{
    download_expert_bundle, expert_bundle_url, expert_bundle_url_for, extract_expert_bundle,
    find_tor_in_dir, tor_cache_dir, TOR_BUNDLE_VERSION,
};
pub use dht::{
    dht_key, normalize_work_name, person_topic_key, work_topic_key, DhtRecord, PeerInfo,
    WorkAnnounce,
};
pub use fx::{fiat_ticker, preview_sats, quote_btc, FxQuote};
pub use message::{NetMessage, FILE_FALLBACK};
pub use overlay::{OverlayConfig, OverlayHandle};
pub use rendezvous::{
    announce_matches_query, announce_topics, announces_from_ntfy, latest_announce, literal_topic,
    lookup_announce, lookup_topics, pick_announce, publish_announce, rendezvous_topic,
    DIRECTORY_TOPIC,
};
pub use tor::{
    bring_up_tor, bring_up_tor_with_hint, default_socks_addr, default_windows_tor_paths,
    discover_socks, find_tor_binary, probe_socks_port, read_onion_hostname, socks5_connect,
    socks_label, spawn_tor, tor_status, write_product_torrc, DiscoveredSocks, TorConfig,
    TorRuntime, TorStatus, SOCKS_CANDIDATE_PORTS,
};
pub use wire::{env_bootstrap_peers, parse_bootstrap_list, PeerAddr};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error(transparent)]
    Core(#[from] hbp_core::Error),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}
