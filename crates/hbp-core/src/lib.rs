//! Off-chain terms and project state. No keys, no signatures.
//!
//! Bitcoin script and PSBTs live in `hbp-bitcoin`. This crate is the protocol
//! vocabulary: hold vs burn, who locked what, which transitions are legal.

mod contract;
mod error;
mod state;

pub use contract::{
    canonical_json, contract_id, Mode, Network, Offer, Role, Terms, CONTRACT_ID_TAG,
};
pub use error::Error;
pub use state::{Project, ProjectStatus};

pub type Result<T> = std::result::Result<T, Error>;
