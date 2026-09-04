//! Transport for the same JSON the file protocol already speaks.
//!
//! Windows v1 needs **Tor** (point-to-point once the onion is known) and a
//! **DHT** (discovery). This crate is the shared message vocabulary plus
//! scaffolding: a real SOCKS5 client, a local Kademlia-shaped store, and
//! documented Windows Tor layout. It is **not** yet a production overlay —
//! no mainline UDP DHT, no onion service publisher inside the process.
//!
//! The contract bytes are [`hbp_core`] types (offer / accept / quote). Bitcoin
//! redeem artifacts (`04-coop.json`, `06-feeburn.json`) travel as tagged JSON
//! blobs so this crate does not depend on `hbp-bitcoin`.

mod dht;
mod message;
mod tor;

pub use dht::{dht_key, DhtNode, DhtRecord, WorkAnnounce};
pub use message::{NetMessage, FILE_FALLBACK};
pub use tor::{
    default_socks_addr, default_windows_tor_paths, socks5_connect, tor_status, TorConfig, TorStatus,
};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Core(#[from] hbp_core::Error),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}
