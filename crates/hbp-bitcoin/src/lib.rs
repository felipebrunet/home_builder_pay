//! Taproot descriptors, MuSig2 cooperative spends, and unwind script-path spends.

mod convert;
mod error;
mod fee_burn;
mod fund;
mod identity;
mod musig;
mod sign_contract;
mod spend;
mod taproot;
mod validate;
mod watch;

pub use error::Error;
pub use fee_burn::{
    assert_fee_burn_t1_shape, assert_fee_burn_t2_shape, build_fee_burn_chain, build_fee_burn_t1_tx,
    build_fee_burn_t2_tx, fee_burn_plan, fee_burn_split, FeeBurnPlan, FeeBurnSplit, DUST_SATS,
    T2_OP_RETURN,
};
pub use fund::{
    attach_prev_tx, build_funding_psbt, build_partial_funding_psbt, combine_psbts,
    complete_partial_funding_psbt, extract_signed_funding_tx, funding_share, funding_tx,
    psbt_signed_input_count, FundingCoin, FundingRequest,
};
pub use identity::{generate_identity, identity_from_secret, Identity};
pub use musig::{
    agg_nonce, combine_partials, consume_nonce_seed, encode_partial, encode_pubnonce,
    finish_coop_signature, new_nonce_seed, our_partial_signature, parse_partial, parse_pubnonce,
    signer_index, start_round, verify_aggregated, CoopFile, CoopSession,
};
pub use sign_contract::{
    sign_arbiter, sign_body, sign_quote, verify_arbiter, verify_body, verify_quote,
};
pub use spend::{
    apply_key_spend_sig, build_key_spend_tx, build_script_path_tx, build_split_key_spend_tx,
    build_split_script_path_tx, build_unwind_tx, key_spend_sighash, sign_arbiter_leaf, sign_unwind,
    verify_key_spend_sig, verify_unwind_control_block, UnwindRole,
};
pub use taproot::{
    assert_output_key_matches, bond_address, bond_descriptor, bond_escrow_from_body,
    fee_burn_escrow, fee_burn_escrow_from_body, keys_from_body, mad_address, mad_escrow,
    mad_escrow_from_body, nums_xonly, partida_address, partida_descriptor,
    partida_escrow_from_body, to_btc_network, tweaked_key_agg, ArbiterWith, Escrow, EscrowKind,
};
pub use validate::{validate_funding_tx, ExpectedFunding, FundingIssue};
pub use watch::{
    address_at, default_esplora_url, default_esplora_urls, import_watch, scan_watch, script_at,
    slip132_to_xpub, OfferedCoin, WatchAccount, WatchKind, WatchScan, WatchedUtxo,
};

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
