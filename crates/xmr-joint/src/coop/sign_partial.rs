//! Partial CLSAG signing: mask assignment (KI order) + sign only local inputs.
//!
//! # Critical crypto rules
//!
//! 1. Inputs are ordered by **key-image sort** (oxide: reverse byte order).
//! 2. Pseudo-out masks: **random for every input except the last**; the last
//!    closes `sum_output_masks - sum(others)`.
//! 3. Output masks come from oxide wallet protocol (`outgoing_view_key` + layout)
//!    — exposed via [`SumOutputMasks`] (upstream must make `sum_output_masks` public).
//! 4. Each party signs **only their own** inputs with the agreed mask.
//! 5. Foreign partials are verified with [`Clsag::verify`] against `signature_hash`.
//!
//! # Upstream seam
//!
//! [`PartialClsagSigner::sign_input_with_mask`] must wrap a public form of
//! `Clsag::sign_core` (currently private in monero-clsag). Until that patch
//! lands, use [`UnsupportedPartialSigner`] (returns a clear error) or a
//! test-only same-process path via `SignableTransaction::sign_with_input_keys`
//! (not this module — that helper is **not** the distributed API).

use std::io::Cursor;

use monero_clsag::{Clsag, ClsagContext};
use monero_wallet::{
  ed25519::{CompressedPoint, Point, Scalar},
  ringct::{RctProofs, RctPrunable},
  transaction::{Input, Transaction},
  OutputWithDecoys,
};
use rand_core::{CryptoRng, RngCore};
use zeroize::Zeroizing;

use crate::coop::assemble::{OwnedInputIndex, SortedInput};
use crate::coop::messages::{PartialSignature, PartialSignatures, SessionId};
use crate::coop::{Error, Result};

/// Upstream monero-wallet must expose the output-mask sum for a signable intent.
///
/// Expected oxide signature (integrator patch):
/// ```ignore
/// impl SignableTransaction {
///     pub fn sum_output_masks(&self, key_images: &[CompressedPoint]) -> Scalar { ... }
/// }
/// ```
pub trait SumOutputMasks {
  /// Sum of output commitment masks for this intent given KI-sorted key images.
  fn sum_output_masks(&self, key_images: &[CompressedPoint]) -> Scalar;
}

/// Upstream monero-clsag must expose per-input CLSAG with a **fixed** mask.
///
/// Expected oxide signature (integrator patch):
/// ```ignore
/// impl Clsag {
///     pub fn sign_input_with_mask<R: RngCore + CryptoRng>(
///         rng: &mut R,
///         one_time_spend: &Zeroizing<Scalar>,
///         context: &ClsagContext,
///         mask: Scalar,
///         msg_hash: [u8; 32],
///     ) -> Result<(Clsag, Point), ClsagError>;
/// }
/// ```
/// (thin public wrapper around private `sign_core` + nonce response).
pub trait PartialClsagSigner {
  /// Sign a single input with a predetermined pseudo-out mask.
  fn sign_input_with_mask<R: RngCore + CryptoRng>(
    &self,
    rng: &mut R,
    one_time_spend: &Zeroizing<Scalar>,
    context: &ClsagContext,
    mask: Scalar,
    msg_hash: [u8; 32],
  ) -> Result<(Clsag, Point)>;
}

/// Default seam: documents that the oxide patch is missing.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedPartialSigner;

impl PartialClsagSigner for UnsupportedPartialSigner {
  fn sign_input_with_mask<R: RngCore + CryptoRng>(
    &self,
    _rng: &mut R,
    _one_time_spend: &Zeroizing<Scalar>,
    _context: &ClsagContext,
    _mask: Scalar,
    _msg_hash: [u8; 32],
  ) -> Result<(Clsag, Point)> {
    Err(Error::OxidePatchRequired(
      "monero-clsag: expose Clsag::sign_input_with_mask (public wrap of sign_core with fixed mask)",
    ))
  }
}

/// Assigned pseudo-out masks for the full KI-sorted input list.
#[derive(Clone, Debug)]
pub struct MaskAssignment {
  /// One mask per sorted input.
  pub masks: Vec<Scalar>,
  /// `sum_output_masks` used to close the last input.
  pub sum_output_masks: Scalar,
}

impl MaskAssignment {
  /// Encode masks as 32-byte arrays for the wire.
  pub fn to_wire_bytes(&self) -> (Vec<[u8; 32]>, [u8; 32]) {
    let masks = self.masks.iter().copied().map(Into::into).collect();
    (masks, <[u8; 32]>::from(self.sum_output_masks))
  }

  /// Decode from wire bytes.
  pub fn from_wire_bytes(masks: &[[u8; 32]], sum: &[u8; 32]) -> Result<Self> {
    let masks = masks
      .iter()
      .map(|b| Scalar::read(&mut b.as_slice()))
      .collect::<std::io::Result<Vec<_>>>()?;
    let sum_output_masks = Scalar::read(&mut sum.as_slice())?;
    Ok(Self { masks, sum_output_masks })
  }
}

fn add_scalars(a: Scalar, b: Scalar) -> Scalar {
  Scalar::from(a.into() + b.into())
}

fn sub_scalars(a: Scalar, b: Scalar) -> Scalar {
  Scalar::from(a.into() - b.into())
}

fn closing_mask(sum_output_masks: Scalar, sum_others: Scalar) -> Scalar {
  sub_scalars(sum_output_masks, sum_others)
}

/// Assign pseudo-out masks: random for all but last; last closes the sum.
///
/// Bob (assembler) typically calls this once and sends `all_masks` to Alice.
/// Openings/masks are shared by design (A/B privacy not required).
pub fn assign_pseudo_out_masks<R: RngCore + CryptoRng>(
  rng: &mut R,
  n_inputs: usize,
  sum_output_masks: Scalar,
) -> Result<MaskAssignment> {
  if n_inputs == 0 {
    return Err(Error::Protocol("no inputs for mask assignment".into()));
  }
  let mut masks = Vec::with_capacity(n_inputs);
  let mut sum_others = Scalar::from(curve25519_dalek::Scalar::ZERO);

  for i in 0..n_inputs {
    if i + 1 == n_inputs {
      masks.push(closing_mask(sum_output_masks, sum_others));
    } else {
      let m = Scalar::random(rng);
      sum_others = add_scalars(sum_others, m);
      masks.push(m);
    }
  }
  Ok(MaskAssignment { masks, sum_output_masks })
}

/// Build `ClsagContext` for a decoyed output.
pub fn context_for_output(output: &OutputWithDecoys) -> Result<ClsagContext> {
  Ok(ClsagContext::new(output.decoys().clone(), output.commitment().clone())?)
}

/// Sign every local input (by ownership) with the assigned masks.
///
/// `one_time_spends` must be parallel to the local owned inputs (in sorted-index
/// ascending order among locals) and equal to `spend_key + key_offset` each.
pub fn sign_local_inputs<R: RngCore + CryptoRng, S: PartialClsagSigner>(
  rng: &mut R,
  signer: &S,
  sorted: &[SortedInput],
  local_owner: OwnedInputIndex,
  one_time_spends: &[Zeroizing<Scalar>],
  masks: &MaskAssignment,
  msg_hash: [u8; 32],
) -> Result<Vec<PartialSignature>> {
  let local: Vec<&SortedInput> = sorted.iter().filter(|s| s.owner == local_owner).collect();
  if local.len() != one_time_spends.len() {
    return Err(Error::Protocol(format!(
      "one_time_spends ({}) != local inputs ({})",
      one_time_spends.len(),
      local.len()
    )));
  }
  if masks.masks.len() != sorted.len() {
    return Err(Error::Protocol("masks length != sorted inputs".into()));
  }

  let mut out = Vec::with_capacity(local.len());
  for (input, spend) in local.into_iter().zip(one_time_spends) {
    let ctx = context_for_output(&input.output)?;
    let mask = masks.masks[input.sorted_index];
    let (clsag, pseudo_out) =
      signer.sign_input_with_mask(rng, spend, &ctx, mask, msg_hash)?;
    let mut clsag_bytes = Vec::new();
    clsag.write(&mut clsag_bytes)?;
    out.push(PartialSignature {
      sorted_index: input.sorted_index,
      key_image: input.key_image.to_bytes(),
      clsag: clsag_bytes,
      pseudo_out: pseudo_out.compress().to_bytes(),
      mask: <[u8; 32]>::from(mask),
    });
  }
  Ok(out)
}

/// Verify foreign partial signatures bind to `signature_hash` and match the skeleton.
pub fn verify_partial_signatures(
  sorted: &[SortedInput],
  partials: &[PartialSignature],
  expected_owner: OwnedInputIndex,
  msg_hash: &[u8; 32],
  unsigned_tx: &Transaction,
) -> Result<()> {
  let expected_count = sorted.iter().filter(|s| s.owner == expected_owner).count();
  if partials.len() != expected_count {
    return Err(Error::Protocol(format!(
      "partial count {} != expected owned inputs {}",
      partials.len(),
      expected_count
    )));
  }

  let prefix = unsigned_tx.prefix();
  for partial in partials {
    if partial.sorted_index >= sorted.len() {
      return Err(Error::Protocol("partial sorted_index OOB".into()));
    }
    let input = &sorted[partial.sorted_index];
    if input.owner != expected_owner {
      return Err(Error::Protocol(format!(
        "partial sorted_index {} has wrong owner",
        partial.sorted_index
      )));
    }
    if input.key_image.to_bytes() != partial.key_image {
      return Err(Error::Protocol("partial key_image mismatch".into()));
    }

    let ring: Vec<[CompressedPoint; 2]> = input
      .output
      .decoys()
      .ring()
      .iter()
      .map(|r| [r[0].compress(), r[1].compress()])
      .collect();
    let ring_len = ring.len();
    let clsag = Clsag::read(ring_len, &mut Cursor::new(partial.clsag.as_slice()))?;
    let pseudo = CompressedPoint::read(&mut Cursor::new(partial.pseudo_out.as_slice()))?;
    let ki = CompressedPoint::read(&mut Cursor::new(partial.key_image.as_slice()))?;
    clsag.verify(ring, &ki, &pseudo, msg_hash)?;

    match &prefix.inputs[partial.sorted_index] {
      Input::ToKey { key_image: tx_ki, .. } => {
        if tx_ki.to_bytes() != partial.key_image {
          return Err(Error::Protocol("TX prefix key_image != partial".into()));
        }
      }
      _ => return Err(Error::Protocol("expected ToKey input".into())),
    }
  }
  Ok(())
}


/// Fill empty CLSAG / pseudo-out slots with placeholders so the TX round-trips
/// through `serialize`/`read`. Placeholders do **not** affect `signature_hash`
/// (Monero's sig-hash transcripts only the bulletproof in the prunable part).
pub fn fill_placeholder_clsags(mut tx: Transaction, sorted: &[SortedInput]) -> Result<Transaction> {
  let sorted_len = sorted.len();
  let Transaction::V2 {
    proofs:
      Some(RctProofs {
        prunable: RctPrunable::Clsag { ref mut clsags, ref mut pseudo_outs, .. },
        ..
      }),
    ..
  } = tx
  else {
    return Err(Error::Protocol("TX is not V2 Clsag proofs".into()));
  };
  if !clsags.is_empty() || !pseudo_outs.is_empty() {
    if clsags.len() == sorted_len && pseudo_outs.len() == sorted_len {
      return Ok(tx);
    }
    return Err(Error::Protocol("partially-filled CLSAG slots".into()));
  }
  *clsags = Vec::with_capacity(sorted_len);
  *pseudo_outs = Vec::with_capacity(sorted_len);
  for s in sorted {
    let ring_len = s.output.decoys().ring().len();
    clsags.push(Clsag {
      D: CompressedPoint::G,
      s: vec![Scalar::ZERO; ring_len],
      c1: Scalar::ZERO,
    });
    pseudo_outs.push(CompressedPoint::G);
  }
  Ok(tx)
}

/// Insert all partial CLSAGs (Alice + Bob) into an unsigned TX in KI-sorted order.
pub fn fill_clsags_into_tx(
  mut tx: Transaction,
  sorted: &[SortedInput],
  all_partials: &[PartialSignature],
) -> Result<Transaction> {
  let sorted_len = sorted.len();
  if all_partials.len() != sorted_len {
    return Err(Error::Protocol("need a partial for every input before fill".into()));
  }
  let mut by_index: Vec<Option<&PartialSignature>> = vec![None; sorted_len];
  for p in all_partials {
    if p.sorted_index >= sorted_len {
      return Err(Error::Protocol("sorted_index OOB".into()));
    }
    if by_index[p.sorted_index].is_some() {
      return Err(Error::Protocol("duplicate sorted_index in partials".into()));
    }
    by_index[p.sorted_index] = Some(p);
  }

  let Transaction::V2 {
    proofs:
      Some(RctProofs {
        prunable: RctPrunable::Clsag { ref mut clsags, ref mut pseudo_outs, .. },
        ..
      }),
    ..
  } = tx
  else {
    return Err(Error::Protocol("TX is not V2 Clsag proofs".into()));
  };

  *clsags = Vec::with_capacity(sorted_len);
  *pseudo_outs = Vec::with_capacity(sorted_len);
  for (i, slot) in by_index.into_iter().enumerate() {
    let p = slot.ok_or_else(|| Error::Protocol(format!("missing partial for index {i}")))?;
    let ring_len = sorted[i].output.decoys().ring().len();
    let clsag = Clsag::read(ring_len, &mut Cursor::new(p.clsag.as_slice()))?;
    let pseudo = CompressedPoint::read(&mut Cursor::new(p.pseudo_out.as_slice()))?;
    clsags.push(clsag);
    pseudo_outs.push(pseudo);
  }
  Ok(tx)
}

/// Build Bob's `PartialSignatures` message body after signing.
pub fn bob_partials_message(
  session_id: SessionId,
  bob_partials: Vec<PartialSignature>,
  masks: &MaskAssignment,
) -> PartialSignatures {
  let (all_masks, sum_output_masks) = masks.to_wire_bytes();
  PartialSignatures {
    session_id,
    partials: bob_partials,
    all_masks,
    sum_output_masks,
  }
}

/// Sanity: masks must satisfy last = sum_out - sum(others).
pub fn verify_mask_closure(masks: &MaskAssignment) -> Result<()> {
  if masks.masks.is_empty() {
    return Err(Error::Protocol("empty masks".into()));
  }
  let mut sum_others = Scalar::from(curve25519_dalek::Scalar::ZERO);
  let last = *masks.masks.last().unwrap();
  for m in &masks.masks[..masks.masks.len() - 1] {
    sum_others = add_scalars(sum_others, *m);
  }
  let expected = closing_mask(masks.sum_output_masks, sum_others);
  if <[u8; 32]>::from(expected) != <[u8; 32]>::from(last) {
    return Err(Error::Protocol("mask closure check failed".into()));
  }
  Ok(())
}
