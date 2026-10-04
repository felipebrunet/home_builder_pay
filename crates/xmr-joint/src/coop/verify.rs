//! Policy verification before Alice signs or broadcasts.
//!
//! Checks that the skeleton pays C as agreed, fee payer is Alice, amounts
//! match the proposal's own `pay_each`, and there are no unexpected destination
//! payments. Amounts are taken from the proposal — not a crate-wide constant.

use monero_wallet::{
  address::Network,
  ringct::RctProofs,
  transaction::Transaction,
};

use crate::coop::assemble::{key_image_sort_bytes, parse_address};
use crate::coop::messages::{Contribution, FeePolicy, Proposal, Skeleton};
use crate::coop::{Error, Result};

/// Result of a successful policy check.
#[derive(Clone, Debug)]
pub struct PolicyCheck {
  /// Fee observed on the unsigned TX (piconero).
  pub fee: u64,
  /// Alice claimed input sum.
  pub alice_in: u64,
  /// Bob claimed input sum.
  pub bob_in: u64,
  /// Per-party net to C (piconero), from the proposal.
  pub pay_each: u64,
}

/// Policy / skeleton mismatch.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
  /// Generic description.
  #[error("{0}")]
  Msg(String),
}

impl From<PolicyError> for Error {
  fn from(e: PolicyError) -> Self {
    Error::Protocol(e.to_string())
  }
}

/// Verify the proposal itself (Alice's side before send, Bob's side on receive).
pub fn verify_policy(proposal: &Proposal) -> Result<PolicyCheck> {
  proposal.validate().map_err(PolicyError::Msg)?;
  if !matches!(proposal.fee_policy, FeePolicy::AlicePays) {
    return Err(PolicyError::Msg("fee payer must be Alice".into()).into());
  }
  let pay_each = proposal.pay_each().map_err(PolicyError::Msg)?;
  let c_total: u64 = proposal.payments_to_c.iter().map(|p| p.amount).sum();
  if c_total != 2 * pay_each {
    return Err(PolicyError::Msg(format!(
      "C total {c_total} != 2 * pay_each ({})",
      2 * pay_each
    ))
    .into());
  }
  Ok(PolicyCheck {
    fee: 0,
    alice_in: proposal.alice.claimed_input_sum,
    bob_in: 0,
    pay_each,
  })
}

/// Verify Bob's skeleton against Alice's original proposal.
///
/// Call **before** Alice signs. Does not yet verify CLSAG crypto (see
/// [`crate::coop::sign_partial::verify_partial_signatures`]).
///
/// `network` selects address encoding (mainnet/regtest, stagenet, or testnet).
pub fn verify_skeleton_against_proposal(
  proposal: &Proposal,
  skeleton: &Skeleton,
  network: Network,
) -> Result<PolicyCheck> {
  let mut check = verify_policy(proposal)?;
  skeleton.validate().map_err(PolicyError::Msg)?;

  if skeleton.session_id != proposal.session_id {
    return Err(PolicyError::Msg("session_id mismatch".into()).into());
  }
  if skeleton.bob_partials.session_id != proposal.session_id {
    return Err(PolicyError::Msg("partials session_id mismatch".into()).into());
  }

  let pay_each = check.pay_each;
  if skeleton.bob.net_to_destination != pay_each {
    return Err(PolicyError::Msg(format!(
      "bob.net_to_destination {} != pay_each {pay_each}",
      skeleton.bob.net_to_destination
    ))
    .into());
  }
  if proposal.alice.net_to_destination != pay_each {
    return Err(PolicyError::Msg(format!(
      "alice.net_to_destination {} != pay_each {pay_each}",
      proposal.alice.net_to_destination
    ))
    .into());
  }

  let alice_in = proposal.alice.claimed_input_sum;
  let fee = skeleton.fee;
  let need = pay_each
    .checked_add(fee)
    .ok_or_else(|| PolicyError::Msg("fee overflow".into()))?;
  if alice_in < need {
    return Err(PolicyError::Msg(format!(
      "alice inputs {alice_in} < net_to_C + fee {fee}"
    ))
    .into());
  }

  let bob_in = skeleton.bob.claimed_input_sum;
  if bob_in < pay_each {
    return Err(PolicyError::Msg("bob inputs < net_to_destination".into()).into());
  }

  check_ki_partition(proposal, &skeleton.bob, skeleton)?;

  let tx = Transaction::read(&mut std::io::Cursor::new(skeleton.unsigned_tx.as_slice()))
    .map_err(|e| PolicyError::Msg(format!("unsigned_tx decode: {e}")))?;
  let tx_fee = match &tx {
    Transaction::V2 {
      proofs: Some(RctProofs { base, .. }),
      ..
    } => base.fee,
    _ => {
      return Err(PolicyError::Msg("unsigned TX missing V2 proofs".into()).into());
    }
  };
  if tx_fee != fee {
    return Err(PolicyError::Msg(format!("skeleton.fee {fee} != tx fee {tx_fee}")).into());
  }

  let recomputed = tx
    .signature_hash()
    .ok_or_else(|| PolicyError::Msg("no signature_hash".into()))?;
  if recomputed != skeleton.signature_hash {
    return Err(PolicyError::Msg("signature_hash mismatch vs unsigned_tx".into()).into());
  }

  let _ = parse_address(&proposal.destination_c, network)?;

  check.fee = fee;
  check.alice_in = alice_in;
  check.bob_in = bob_in;
  Ok(check)
}

fn check_ki_partition(proposal: &Proposal, bob: &Contribution, skeleton: &Skeleton) -> Result<()> {
  let mut all = Vec::new();
  all.extend_from_slice(&proposal.alice.key_images);
  all.extend_from_slice(&bob.key_images);
  all.sort_by(key_image_sort_bytes);
  if all != skeleton.key_images_sorted {
    return Err(PolicyError::Msg("key_images_sorted != sort(alice∪bob)".into()).into());
  }
  let mut uniq = all.clone();
  uniq.dedup();
  if uniq.len() != all.len() {
    return Err(PolicyError::Msg("duplicate key images".into()).into());
  }
  Ok(())
}

/// High-level Alice pre-sign gate: policy + Bob CLSAG verify flag.
///
/// `bob_clsags_ok` should be the result of
/// [`crate::coop::sign_partial::verify_partial_signatures`].
pub fn alice_pre_sign_checks(
  proposal: &Proposal,
  skeleton: &Skeleton,
  bob_clsags_ok: bool,
  network: Network,
) -> Result<PolicyCheck> {
  let check = verify_skeleton_against_proposal(proposal, skeleton, network)?;
  if !bob_clsags_ok {
    return Err(PolicyError::Msg("Bob CLSAG verification failed".into()).into());
  }
  for p in &skeleton.bob_partials.partials {
    if skeleton.alice_sorted_indices.contains(&p.sorted_index) {
      return Err(PolicyError::Msg(
        "Bob partial covers an Alice-owned sorted_index".into(),
      )
      .into());
    }
    if !skeleton.bob_sorted_indices.contains(&p.sorted_index) {
      return Err(PolicyError::Msg(
        "Bob partial sorted_index not in bob_sorted_indices".into(),
      )
      .into());
    }
  }
  Ok(check)
}
