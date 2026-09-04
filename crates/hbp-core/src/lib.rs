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
mod vault;

pub use amount::{
    bond_minor, bond_warnings, equal_stage_count, fiat_minor_to_sats, minor_from_major,
    stage_bond_warnings, stages_equal_bond, suggest_equal_stage_minors, Unit, DEFAULT_BOND_BPS,
    MINOR_PER_MAJOR,
};
pub use contract::{
    canonical_json, contract_id, decode_compressed_pubkey, sha256_bytes, validate_fee_burn_deadlines,
    ArbiterNomination, ContractBody, ContractId, DisputePolicy, Network, Offer, PartidaQuote,
    PartidaSpec, Quote, Role, SignedContract, Terms, ARBITER_ENABLED, DEFAULT_ARBITER_WINDOW_SECS,
};
pub use error::Error;
pub use nonce::NonceJournal;
pub use state::{
    BondStatus, PartidaRuntime, PartidaStatus, Project, ProjectStatus, DEFAULT_CONFIRMATIONS,
};
pub use vault::{
    decrypt as vault_decrypt, encrypt as vault_encrypt, is_encrypted as vault_is_encrypted,
};

pub type Result<T> = std::result::Result<T, Error>;
