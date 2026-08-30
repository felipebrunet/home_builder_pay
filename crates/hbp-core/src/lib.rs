//! Off-chain contract types and the project/partida state machine.
//!
//! Bitcoin script details live in `hbp-bitcoin`. This crate is the protocol
//! vocabulary: who locked what, which partida is live, and which transitions
//! are legal.

mod amount;
mod contract;
mod error;
mod nonce;
mod state;

pub use amount::{
    bond_minor, bond_warnings, fiat_minor_to_sats, minor_from_major, Unit, MINOR_PER_MAJOR,
};
pub use contract::{
    canonical_json, contract_id, decode_compressed_pubkey, sha256_bytes, ContractBody, ContractId,
    Network, Offer, PartidaQuote, PartidaSpec, Quote, Role, SignedContract, Terms,
};
pub use error::Error;
pub use nonce::NonceJournal;
pub use state::{
    BondStatus, PartidaRuntime, PartidaStatus, Project, ProjectStatus, DEFAULT_CONFIRMATIONS,
};

pub type Result<T> = std::result::Result<T, Error>;
