//! Taproot descriptors, MuSig2 cooperative spends, and unwind script-path spends.

mod convert;
mod error;
mod identity;
mod musig;
mod sign_contract;
mod spend;
mod taproot;
mod validate;

pub use error::Error;
pub use identity::{generate_identity, Identity};
pub use musig::{
    agg_nonce, consume_nonce_seed, encode_pubnonce, finish_coop_signature, new_nonce_seed,
    parse_pubnonce, signer_index, verify_aggregated, CoopSession,
};
pub use sign_contract::{sign_arbiter, sign_body, verify_arbiter, verify_body};
pub use spend::{
    apply_key_spend_sig, build_key_spend_tx, build_split_key_spend_tx, build_unwind_tx,
    key_spend_sighash, sign_unwind, verify_key_spend_sig, verify_unwind_control_block, UnwindRole,
};
pub use taproot::{
    assert_output_key_matches, bond_address, bond_descriptor, bond_escrow_from_body,
    keys_from_body, mad_address, mad_escrow, mad_escrow_from_body, nums_xonly, partida_address,
    partida_descriptor, partida_escrow_from_body, to_btc_network, tweaked_key_agg, Escrow,
    EscrowKind,
};
pub use validate::{validate_funding_tx, ExpectedFunding, FundingIssue};

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
