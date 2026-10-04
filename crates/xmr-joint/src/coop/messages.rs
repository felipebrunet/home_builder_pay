//! Wire types for the Alice/Bob cooperative partial-signing protocol.
//!
//! # Serialization
//!
//! Oxide types (`OutputWithDecoys`, `Clsag`, `SignableTransaction`,
//! `CompressedPoint`) already provide binary `write`/`read`/`serialize`.
//! Messages wrap those as `Vec<u8>` (opaque blobs) plus small policy fields so
//! that **serde JSON** or **bincode** can carry the envelope without Serde
//! derives on oxide internals.
//!
//! Recommended:
//! - production wire: `bincode` (feature `serde-wire`) or raw length-prefixed
//!   oxide blobs inside a framing layer;
//! - debug / logs: `serde_json` with hex-encoded blobs.
//!
//! Do **not** put spend keys on the wire. Openings (amount + mask) inside
//! `OutputWithDecoys` **are** shared by design (privacy between A/B is not a
//! goal for this flow).

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// 1 XMR in piconero.
pub const XMR_ATOMIC: u64 = 1_000_000_000_000;

/// Convenience amount for local regtest PoCs with mined funds (10 XMR).
pub const DEFAULT_PAY_EACH_REGTEST: u64 = 10 * XMR_ATOMIC;

/// Convenience amount for stagenet PoCs with faucet balances (0.04 XMR).
pub const DEFAULT_PAY_EACH_STAGENET: u64 = XMR_ATOMIC / 25; // 0.04 XMR

/// Historical alias — prefer an explicit `pay_each` argument / CLI flag.
/// Equals [`DEFAULT_PAY_EACH_STAGENET`] (0.04 XMR); **not** the only supported mode.
pub const DEFAULT_PAY_EACH: u64 = DEFAULT_PAY_EACH_STAGENET;

/// Convert an XMR float (e.g. `0.04`, `10.0`) to piconero.
///
/// Panics if the value is negative or overflows `u64` after rounding.
pub fn pay_each_piconero(xmr: f64) -> u64 {
  assert!(xmr.is_finite() && xmr >= 0.0, "pay_each XMR must be finite and >= 0");
  let pico = (xmr * (XMR_ATOMIC as f64)).round();
  assert!(pico <= u64::MAX as f64, "pay_each overflows u64");
  pico as u64
}

/// Opaque 32-byte session identifier (random).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct SessionId(pub [u8; 32]);

impl SessionId {
  /// Create from raw bytes.
  pub fn from_bytes(bytes: [u8; 32]) -> Self {
    Self(bytes)
  }

  /// Hex encoding for logs.
  pub fn to_hex(&self) -> String {
    hex::encode(self.0)
  }
}

/// Who pays the network fee.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize, Default)]
pub enum FeePolicy {
  /// Alice pays the fee (design default — fee taken from Alice's change).
  #[default]
  AlicePays,
}

/// A payment the TX must include (plaintext amounts — A/B share openings).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PaymentSpec {
  /// Destination Monero address string (encoding must match the chosen network).
  pub address: String,
  /// Amount in piconero.
  pub amount: u64,
}

/// One party's contributed inputs for the cooperative TX.
///
/// `outputs_with_decoys` are oxide `OutputWithDecoys::serialize()` blobs.
/// `key_images` are 32-byte compressed key images (same order as the outputs).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Contribution {
  /// Party label for debugging (`"alice"` / `"bob"`).
  pub party: String,
  /// Serialized `OutputWithDecoys` (one per input).
  pub outputs_with_decoys: Vec<Vec<u8>>,
  /// Compressed key images, 32 bytes each, parallel to `outputs_with_decoys`.
  pub key_images: Vec<[u8; 32]>,
  /// Sum of plaintext input amounts (piconero) claimed by this party.
  pub claimed_input_sum: u64,
  /// Desired net contribution toward C (must match each payment-to-C amount).
  pub net_to_destination: u64,
  /// Optional change address string for this party (as a normal payment).
  /// Alice typically uses oxide `Change::new(alice_view)`; Bob uses an explicit payment.
  pub change_address: Option<String>,
}

impl Contribution {
  /// Validate structural consistency of a contribution.
  pub fn validate_structure(&self) -> Result<(), String> {
    if self.outputs_with_decoys.is_empty() {
      return Err(format!("{}: no inputs", self.party));
    }
    if self.outputs_with_decoys.len() != self.key_images.len() {
      return Err(format!(
        "{}: outputs ({}) != key_images ({})",
        self.party,
        self.outputs_with_decoys.len(),
        self.key_images.len()
      ));
    }
    Ok(())
  }
}

/// Message 1 — Alice → Bob: unsigned contribution + policy.
///
/// Alice proposes her inputs and the TX policy. She does **not** sign yet.
/// Amounts are **parameters** on the proposal (`payments_to_c` / `net_to_destination`);
/// there is no single hardcoded pay amount in the protocol.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Proposal {
  /// Session id.
  #[zeroize(skip)]
  pub session_id: SessionId,
  /// Shared TX secret seeding oxide RNGs / output masks.
  /// Anyone with this can identify TX content — treat as joint secret.
  pub outgoing_view_key: [u8; 32],
  /// RCT type discriminant as `u8` (oxide `RctType`); design default = ClsagBulletproofPlus (6).
  #[zeroize(skip)]
  pub rct_type: u8,
  /// Fee policy (Alice pays).
  #[zeroize(skip)]
  pub fee_policy: FeePolicy,
  /// Fee rate: `(per_weight, mask)` for `FeeRate::new`.
  #[zeroize(skip)]
  pub fee_rate: (u64, u64),
  /// Destination C address string.
  #[zeroize(skip)]
  pub destination_c: String,
  /// Payments to C (design helper: two equal outs totaling `2 * pay_each`).
  #[zeroize(skip)]
  pub payments_to_c: Vec<PaymentSpec>,
  /// Alice's contribution.
  #[zeroize(skip)]
  pub alice: Contribution,
  /// Protocol version for future evolution.
  #[zeroize(skip)]
  pub protocol_version: u32,
}

impl Proposal {
  /// Construct a two-equal-outs-to-C proposal with an explicit `pay_each` (piconero).
  ///
  /// Layout: two payments of `pay_each` to `destination_c`, Alice pays fee.
  /// Callers must set `alice.net_to_destination == pay_each` (and Bob likewise).
  pub fn design_default(
    session_id: SessionId,
    outgoing_view_key: [u8; 32],
    destination_c: String,
    alice: Contribution,
    fee_rate: (u64, u64),
    pay_each: u64,
  ) -> Self {
    Self {
      session_id,
      outgoing_view_key,
      rct_type: 6, // RctType::ClsagBulletproofPlus
      fee_policy: FeePolicy::AlicePays,
      fee_rate,
      destination_c: destination_c.clone(),
      payments_to_c: vec![
        PaymentSpec {
          address: destination_c.clone(),
          amount: pay_each,
        },
        PaymentSpec {
          address: destination_c,
          amount: pay_each,
        },
      ],
      alice,
      protocol_version: 1,
    }
  }

  /// Per-output amount to C implied by this proposal (`payments_to_c[0].amount`).
  pub fn pay_each(&self) -> Result<u64, String> {
    let first = self
      .payments_to_c
      .first()
      .ok_or_else(|| "payments_to_c empty".to_string())?;
    Ok(first.amount)
  }

  /// Structural checks before Bob accepts.
  ///
  /// Requires exactly two equal positive payments to `destination_c`, matching
  /// `alice.net_to_destination`. Amounts are **not** compared to a crate-wide
  /// constant — they come from the proposal itself.
  pub fn validate(&self) -> Result<(), String> {
    if self.protocol_version != 1 {
      return Err(format!("unsupported protocol_version {}", self.protocol_version));
    }
    if self.rct_type != 6 {
      return Err(format!("expected ClsagBulletproofPlus (6), got {}", self.rct_type));
    }
    if !matches!(self.fee_policy, FeePolicy::AlicePays) {
      return Err("only FeePolicy::AlicePays is supported".into());
    }
    if self.payments_to_c.len() != 2 {
      return Err(format!(
        "design default requires 2 payments to C, got {}",
        self.payments_to_c.len()
      ));
    }
    let pay_each = self.payments_to_c[0].amount;
    if pay_each == 0 {
      return Err("pay_each (payment to C) must be > 0".into());
    }
    for p in &self.payments_to_c {
      if p.address != self.destination_c {
        return Err("payment address != destination_c".into());
      }
      if p.amount != pay_each {
        return Err(format!(
          "all payments to C must be equal (expected {pay_each}, got {})",
          p.amount
        ));
      }
    }
    if self.alice.net_to_destination != pay_each {
      return Err(format!(
        "alice.net_to_destination {} != pay_each {pay_each}",
        self.alice.net_to_destination
      ));
    }
    self.alice.validate_structure()?;
    if self.alice.party != "alice" {
      return Err("alice.party must be \"alice\"".into());
    }
    Ok(())
  }
}

/// One CLSAG + pseudo-out for a single input index (in **key-image-sorted** order).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PartialSignature {
  /// Index into the KI-sorted input list.
  pub sorted_index: usize,
  /// Compressed key image this signature covers (must match skeleton).
  pub key_image: [u8; 32],
  /// Serialized `Clsag` (`Clsag::write`).
  pub clsag: Vec<u8>,
  /// Compressed pseudo-out commitment (32 bytes).
  pub pseudo_out: [u8; 32],
  /// Pseudo-out mask used (shared — A/B privacy not required). Scalar bytes.
  pub mask: [u8; 32],
}

/// Message 2 body — Bob's CLSAGs only (not Alice's).
///
/// **"Bob signs the complete TX"** means: Bob produces CLSAGs for **Bob's
/// inputs** on the **full agreed skeleton** (same `signature_hash`). It does
/// **not** mean Bob signs Alice's keys.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct PartialSignatures {
  /// Session id.
  pub session_id: SessionId,
  /// Sorted-order partials for every input Bob owns.
  pub partials: Vec<PartialSignature>,
  /// Full mask vector for **all** inputs in KI-sorted order (Bob assigns all
  /// non-last randomly; last = closing mask). Alice must use these exact masks
  /// when signing her inputs.
  pub all_masks: Vec<[u8; 32]>,
  /// `sum_output_masks` scalar bytes (from oxide; requires upstream visibility patch).
  pub sum_output_masks: [u8; 32],
}

/// Message 2 — Bob → Alice: assembled skeleton + Bob's partial signatures.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Skeleton {
  /// Session id.
  pub session_id: SessionId,
  /// Serialized `SignableTransaction` (oxide `serialize`) after assembling A+B inputs.
  pub signable_tx: Vec<u8>,
  /// All key images in **KI-sorted** order (32 bytes each).
  pub key_images_sorted: Vec<[u8; 32]>,
  /// Which sorted indices Bob owns.
  pub bob_sorted_indices: Vec<usize>,
  /// Which sorted indices Alice owns.
  pub alice_sorted_indices: Vec<usize>,
  /// Serialized unsigned `Transaction` (CLSAG slots empty / placeholders).
  pub unsigned_tx: Vec<u8>,
  /// `Transaction::signature_hash()` — all CLSAGs bind to this.
  pub signature_hash: [u8; 32],
  /// Fee actually used (piconero).
  pub fee: u64,
  /// Bob's contribution (echoed for Alice's verification).
  pub bob: Contribution,
  /// Bob's partial signatures + mask assignment.
  pub bob_partials: PartialSignatures,
}

impl Skeleton {
  /// Structural checks.
  pub fn validate(&self) -> Result<(), String> {
    self.bob.validate_structure()?;
    if self.bob.party != "bob" {
      return Err("bob.party must be \"bob\"".into());
    }
    if self.key_images_sorted.len()
      != self.alice_sorted_indices.len() + self.bob_sorted_indices.len()
    {
      return Err("sorted index partition != key_images_sorted len".into());
    }
    if self.bob_partials.all_masks.len() != self.key_images_sorted.len() {
      return Err("all_masks len != key_images_sorted len".into());
    }
    if self.bob_partials.partials.len() != self.bob_sorted_indices.len() {
      return Err("bob partials count != bob_sorted_indices".into());
    }
    if self.signature_hash == [0u8; 32] {
      return Err("signature_hash is zero".into());
    }
    Ok(())
  }
}

/// Optional Message 3 — Alice → Bob (or broadcast path): fully signed TX bytes.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct CompletedTransaction {
  /// Session id.
  pub session_id: SessionId,
  /// Fully signed oxide `Transaction::serialize()` bytes.
  pub signed_tx: Vec<u8>,
  /// Alice's partials (for audit).
  pub alice_partials: Vec<PartialSignature>,
}

#[cfg(feature = "serde-wire")]
mod wire {
  use super::*;

  /// Encode with bincode.
  pub fn encode_bincode<T: Serialize>(value: &T) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(value)
  }

  /// Decode with bincode.
  pub fn decode_bincode<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, bincode::Error> {
    bincode::deserialize(bytes)
  }

  /// Encode with JSON (debug).
  pub fn encode_json<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
  }
}

#[cfg(feature = "serde-wire")]
pub use wire::*;
