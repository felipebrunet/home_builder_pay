//! Cooperative Alice/Bob partial-signing for one Monero transaction.
//!
//! Brought into `xmr-joint` from the `partial-sign` lab. Alice is the mandante
//! (pays the fee). Bob is the contratista. Destination C is the obra's 2-of-2
//! address. monero-oxide is vendored under `third_party/monero-oxide` with
//! `sign_input_with_mask` and public `sum_output_masks`.
//!
//! # What this is
//!
//! Alice and Bob each contribute inputs to a single Monero TX that pays
//! destination C (helper: two equal outs of a caller-chosen `pay_each`). Fee is
//! paid by Alice. Both parties' CLSAGs are required for consensus acceptance →
//! on-chain atomicity. Amounts and network are **parameters**, not hardcoded.
//!
//! Preferred flow: Alice proposes → Bob assembles the skeleton, signs **Bob's**
//! CLSAGs, returns → Alice verifies, signs **Alice's** CLSAGs, broadcasts.
//!
//! # Oxide mapping
//!
//! | Protocol concept | Oxide type |
//! |------------------|------------|
//! | Contributed UTXO + ring | [`monero_wallet::OutputWithDecoys`] |
//! | Unsigned intent | [`monero_wallet::send::SignableTransaction`] |
//! | Skeleton TX | [`monero_wallet::transaction::Transaction`] via `unsigned_transaction` |
//! | Per-input proof | [`monero_clsag::Clsag`] + pseudo-out [`CompressedPoint`] |
//! | Signing context | [`monero_clsag::ClsagContext`] |
//!
//! # Missing upstream API
//!
//! Stock oxide has no public per-input CLSAG with a **fixed** pseudo-out mask
//! (`sign_core` is private) and `sum_output_masks` is `pub(crate)`. This crate
//! defines [`PartialClsagSigner`] / [`SumOutputMasks`] seams the integrator
//! must satisfy. Optional same-process helper `sign_with_input_keys` (already
//! patched in the local clone) is for tests only — **not** the distributed API.
//!
//! See `PROTOCOL.md` (Spanish) for the full protocol.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod assemble;
pub mod messages;
pub mod oxide_impl;
pub mod session;
pub mod sign_partial;
pub mod verify;

pub use assemble::{
  assemble_signable, build_unsigned_skeleton, key_image_sort_order, network_from_str,
  network_label, parse_address, parse_address_any, OwnedInputIndex, SortedInput, UnsignedSkeleton,
};
pub use messages::{
  pay_each_piconero, CompletedTransaction, Contribution, FeePolicy, PartialSignature,
  PartialSignatures, PaymentSpec, Proposal, SessionId, Skeleton, DEFAULT_PAY_EACH,
  DEFAULT_PAY_EACH_REGTEST, DEFAULT_PAY_EACH_STAGENET, XMR_ATOMIC,
};
#[cfg(feature = "serde-wire")]
pub use messages::{decode_bincode, encode_bincode, encode_json};
pub use session::{AbortReason, CoopSession, Role, SessionState};
pub use oxide_impl::{OxidePartialSigner, OxideSumMasks};
pub use sign_partial::{
  assign_pseudo_out_masks, fill_clsags_into_tx, sign_local_inputs, verify_mask_closure,
  verify_partial_signatures, MaskAssignment, PartialClsagSigner, SumOutputMasks,
  UnsupportedPartialSigner,
};
pub use verify::{
  alice_pre_sign_checks, verify_policy, verify_skeleton_against_proposal, PolicyCheck, PolicyError,
};

/// Crate-level errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  /// Protocol / validation failure.
  #[error("protocol: {0}")]
  Protocol(String),
  /// Oxide send construction error.
  #[error("send: {0}")]
  Send(#[from] monero_wallet::send::SendError),
  /// CLSAG crypto error.
  #[error("clsag: {0}")]
  Clsag(#[from] monero_clsag::ClsagError),
  /// I/O / serialization.
  #[error("io: {0}")]
  Io(#[from] std::io::Error),
  /// Session abort.
  #[error("abort: {0}")]
  Abort(#[from] AbortReason),
  /// Missing upstream oxide API (integrator must patch).
  #[error("oxide patch required: {0}")]
  OxidePatchRequired(&'static str),
}

/// Convenient result alias.
pub type Result<T> = std::result::Result<T, Error>;
