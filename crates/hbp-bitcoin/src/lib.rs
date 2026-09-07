//! P2WSH sortedmulti 2-of-2 and PSBT helpers. No keys live here.

mod convert;
mod error;
mod fund;
mod p2wsh;
mod spend;
mod watch;

pub use convert::to_btc_network;
pub use error::Error;
pub use fund::{
    attach_prev_tx, build_funding_psbt, combine_psbts, extract_signed_funding_tx, funding_tx,
    FundingCoin, FundingRequest,
};
pub use p2wsh::{escrow_at, normalize_cosigner_key, wsh_sortedmulti, Escrow};
pub use spend::{
    build_burn_psbt, build_coop_psbt, extract_wsh_tx, CoopOutput, BURN_TAG,
};
pub use watch::{
    address_at, default_esplora_url, default_esplora_urls, import_watch, scan_watch, script_at,
    slip132_to_xpub, OfferedCoin, WatchAccount, WatchKind, WatchScan, WatchedUtxo,
};

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
