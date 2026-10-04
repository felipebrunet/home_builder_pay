//! Real [`PartialClsagSigner`] / [`SumOutputMasks`] backed by the patched oxide APIs.
//!
//! Requires:
//! - `Clsag::sign_input_with_mask` (monero-clsag)
//! - `SignableTransaction::sum_output_masks` (monero-wallet, public)

use monero_clsag::{Clsag, ClsagContext};
use monero_wallet::{
  ed25519::{CompressedPoint, Point, Scalar},
  send::SignableTransaction,
};
use rand_core::{CryptoRng, RngCore};
use zeroize::Zeroizing;

use crate::coop::sign_partial::{PartialClsagSigner, SumOutputMasks};
use crate::coop::Result;

/// Oxide-backed partial CLSAG signer (one input, fixed mask).
#[derive(Debug, Default, Clone, Copy)]
pub struct OxidePartialSigner;

impl PartialClsagSigner for OxidePartialSigner {
  fn sign_input_with_mask<R: RngCore + CryptoRng>(
    &self,
    rng: &mut R,
    one_time_spend: &Zeroizing<Scalar>,
    context: &ClsagContext,
    mask: Scalar,
    msg_hash: [u8; 32],
  ) -> Result<(Clsag, Point)> {
    Ok(Clsag::sign_input_with_mask(
      rng,
      one_time_spend,
      context,
      mask,
      msg_hash,
    )?)
  }
}

/// Oxide-backed sum of output masks.
#[derive(Debug, Clone)]
pub struct OxideSumMasks(pub SignableTransaction);

impl SumOutputMasks for OxideSumMasks {
  fn sum_output_masks(&self, key_images: &[CompressedPoint]) -> Scalar {
    self.0.sum_output_masks(key_images)
  }
}

impl SumOutputMasks for SignableTransaction {
  fn sum_output_masks(&self, key_images: &[CompressedPoint]) -> Scalar {
    SignableTransaction::sum_output_masks(self, key_images)
  }
}
