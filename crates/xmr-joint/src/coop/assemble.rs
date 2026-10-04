//! Assemble a [`SignableTransaction`] from Alice + Bob contributions.
//!
//! Calls into monero-wallet APIs that **already exist**:
//! - [`OutputWithDecoys::read`]
//! - [`SignableTransaction::new`]
//! - [`SignableTransaction::unsigned_transaction`]
//! - [`SignableTransaction::necessary_fee`]
//!
//! Input order after assembly follows oxide's **key-image sort**
//! (descending byte order — see [`key_image_sort`]). Ownership maps are
//! returned so each party knows which sorted indices they must sign.

use std::cmp::Ordering;
use std::io::Cursor;

use monero_wallet::{
  address::{MoneroAddress, Network},
  ed25519::{CompressedPoint, Scalar},
  interface::FeeRate,
  ringct::RctType,
  send::{Change, SignableTransaction},
  transaction::Transaction,
  OutputWithDecoys, ViewPair,
};
use zeroize::Zeroizing;

use crate::coop::messages::{Contribution, PartialSignatures, Proposal, SessionId, Skeleton};
use crate::coop::sign_partial::fill_placeholder_clsags;
use crate::coop::{Error, Result};

/// Monero key-image ordering used by oxide (`pub(crate) key_image_sort`):
/// reverse lexicographic on compressed bytes.
pub fn key_image_sort(a: &CompressedPoint, b: &CompressedPoint) -> Ordering {
  a.cmp(b).reverse()
}

/// Same sort on raw 32-byte arrays.
pub fn key_image_sort_bytes(a: &[u8; 32], b: &[u8; 32]) -> Ordering {
  a.cmp(b).reverse()
}

/// Which party owns an input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OwnedInputIndex {
  /// Alice owns this sorted index.
  Alice,
  /// Bob owns this sorted index.
  Bob,
}

/// One input after KI sorting.
#[derive(Clone, Debug)]
pub struct SortedInput {
  /// Index in the KI-sorted list.
  pub sorted_index: usize,
  /// Owner.
  pub owner: OwnedInputIndex,
  /// Key image.
  pub key_image: CompressedPoint,
  /// Decoyed output.
  pub output: OutputWithDecoys,
}

/// Decode oxide `OutputWithDecoys` blobs from a contribution.
pub fn decode_outputs(contrib: &Contribution) -> Result<Vec<OutputWithDecoys>> {
  contrib.validate_structure().map_err(Error::Protocol)?;
  let mut out = Vec::with_capacity(contrib.outputs_with_decoys.len());
  for blob in &contrib.outputs_with_decoys {
    let o = OutputWithDecoys::read(&mut Cursor::new(blob.as_slice()))?;
    out.push(o);
  }
  Ok(out)
}

/// Decode compressed key images.
pub fn decode_key_images(contrib: &Contribution) -> Result<Vec<CompressedPoint>> {
  contrib.validate_structure().map_err(Error::Protocol)?;
  Ok(
    contrib
      .key_images
      .iter()
      .map(|ki| CompressedPoint::read(&mut Cursor::new(ki.as_slice())))
      .collect::<std::io::Result<Vec<_>>>()?,
  )
}

/// Build the KI-sorted ownership list from Alice + Bob contributions.
pub fn key_image_sort_order(
  alice: &Contribution,
  bob: &Contribution,
) -> Result<Vec<SortedInput>> {
  let a_outs = decode_outputs(alice)?;
  let b_outs = decode_outputs(bob)?;
  let a_kis = decode_key_images(alice)?;
  let b_kis = decode_key_images(bob)?;

  let mut paired: Vec<(OwnedInputIndex, OutputWithDecoys, CompressedPoint)> = Vec::new();
  for (o, ki) in a_outs.into_iter().zip(a_kis) {
    paired.push((OwnedInputIndex::Alice, o, ki));
  }
  for (o, ki) in b_outs.into_iter().zip(b_kis) {
    paired.push((OwnedInputIndex::Bob, o, ki));
  }

  paired.sort_by(|(_, _, a), (_, _, b)| key_image_sort(a, b));

  Ok(
    paired
      .into_iter()
      .enumerate()
      .map(|(sorted_index, (owner, output, key_image))| SortedInput {
        sorted_index,
        owner,
        key_image,
        output,
      })
      .collect(),
  )
}

fn rct_type_from_u8(v: u8) -> Result<RctType> {
  match v {
    6 => Ok(RctType::ClsagBulletproofPlus),
    other => Err(Error::Protocol(format!("unsupported rct_type {other}"))),
  }
}

/// Parse a CLI / config network name into oxide [`Network`].
///
/// Accepted: `mainnet`, `regtest` (alias → Mainnet encoding / local fakechain),
/// `stagenet`, `testnet`.
pub fn network_from_str(s: &str) -> Result<Network> {
  match s.trim().to_ascii_lowercase().as_str() {
    "mainnet" | "regtest" | "fakechain" => Ok(Network::Mainnet),
    "stagenet" => Ok(Network::Stagenet),
    "testnet" => Ok(Network::Testnet),
    other => Err(Error::Protocol(format!(
      "unknown network {other:?}; use mainnet|regtest|stagenet|testnet"
    ))),
  }
}

/// Human label for logs (`regtest` is preferred over `mainnet` when the caller
/// asked for regtest; pass the original CLI string separately if needed).
pub fn network_label(n: Network) -> &'static str {
  match n {
    Network::Mainnet => "mainnet",
    Network::Stagenet => "stagenet",
    Network::Testnet => "testnet",
  }
}

/// Parse a Monero address for an **explicit** network (preferred).
pub fn parse_address(s: &str, network: Network) -> Result<MoneroAddress> {
  MoneroAddress::from_str(network, s)
    .map_err(|e| Error::Protocol(format!("address parse ({network:?}): {e:?}")))
}

/// Best-effort parse trying Stagenet → Mainnet → Testnet (legacy / debug only).
/// Prefer [`parse_address`] with an explicit network.
pub fn parse_address_any(s: &str) -> Result<MoneroAddress> {
  MoneroAddress::from_str(Network::Stagenet, s)
    .or_else(|_| MoneroAddress::from_str(Network::Mainnet, s))
    .or_else(|_| MoneroAddress::from_str(Network::Testnet, s))
    .map_err(|e| Error::Protocol(format!("address parse (any network): {e:?}")))
}

/// Build payment list: two outs to C + Bob's change as an explicit payment.
///
/// Alice's change uses oxide [`Change::new`] (fee deducted from Alice).
pub fn build_payments_and_change(
  proposal: &Proposal,
  bob: &Contribution,
  alice_view: ViewPair,
  network: Network,
) -> Result<(Vec<(MoneroAddress, u64)>, Change)> {
  let mut payments = Vec::new();
  for p in &proposal.payments_to_c {
    payments.push((parse_address(&p.address, network)?, p.amount));
  }

  let bob_change = bob
    .claimed_input_sum
    .checked_sub(bob.net_to_destination)
    .ok_or_else(|| Error::Protocol("bob change underflow".into()))?;
  if bob_change > 0 {
    let bob_change_addr = bob
      .change_address
      .as_ref()
      .ok_or_else(|| Error::Protocol("bob change_address required when change > 0".into()))?;
    payments.push((parse_address(bob_change_addr, network)?, bob_change));
  }

  let change = Change::new(alice_view, None);
  Ok((payments, change))
}

/// Assemble [`SignableTransaction`] from proposal + Bob contribution + Alice view pair.
///
/// Inputs are passed in **contribution order** (Alice's then Bob's) — oxide
/// will re-sort by key image when signing / building the unsigned TX.
pub fn assemble_signable(
  proposal: &Proposal,
  bob: &Contribution,
  alice_view: ViewPair,
  network: Network,
) -> Result<SignableTransaction> {
  proposal.validate().map_err(Error::Protocol)?;
  bob.validate_structure().map_err(Error::Protocol)?;

  let mut inputs = decode_outputs(&proposal.alice)?;
  inputs.extend(decode_outputs(bob)?);

  let (payments, change) = build_payments_and_change(proposal, bob, alice_view, network)?;
  let fee_rate = FeeRate::new(proposal.fee_rate.0, proposal.fee_rate.1)
    .ok_or_else(|| Error::Protocol("invalid FeeRate".into()))?;
  let rct = rct_type_from_u8(proposal.rct_type)?;
  let ovk = Zeroizing::new(proposal.outgoing_view_key);

  Ok(SignableTransaction::new(rct, ovk, inputs, payments, change, vec![], fee_rate)?)
}

/// Result of building the unsigned skeleton (before any CLSAG).
pub struct UnsignedSkeleton {
  /// Intent (re-read after `unsigned_transaction` consumes the original).
  pub signable: SignableTransaction,
  /// Serialized intent.
  pub signable_bytes: Vec<u8>,
  /// KI-sorted ownership.
  pub sorted: Vec<SortedInput>,
  /// Unsigned TX with empty CLSAG slots.
  pub unsigned_tx: Transaction,
  /// Binding hash for all CLSAGs.
  pub signature_hash: [u8; 32],
  /// Fee in piconero.
  pub fee: u64,
}

/// Build unsigned TX + signature hash from an assembled intent + all key images
/// (Alice∥Bob contribution order, matching `assemble_signable` input order).
pub fn build_unsigned_skeleton(
  signable: SignableTransaction,
  alice: &Contribution,
  bob: &Contribution,
) -> Result<UnsignedSkeleton> {
  let sorted = key_image_sort_order(alice, bob)?;
  let fee = signable.necessary_fee();
  let signable_bytes = signable.serialize();

  let mut kis_contrib_order = decode_key_images(alice)?;
  kis_contrib_order.extend(decode_key_images(bob)?);

  let unsigned_tx = signable
    .unsigned_transaction(kis_contrib_order)
    .ok_or_else(|| Error::Protocol("unsigned_transaction: key image count mismatch".into()))?;

  let signature_hash = unsigned_tx
    .signature_hash()
    .ok_or_else(|| Error::Protocol("signature_hash missing on unsigned TX".into()))?;

  let signable = SignableTransaction::read(&mut Cursor::new(signable_bytes.as_slice()))?;

  Ok(UnsignedSkeleton {
    signable,
    signable_bytes,
    sorted,
    unsigned_tx,
    signature_hash,
    fee,
  })
}

/// Helper: partition sorted indices by owner.
pub fn partition_owners(sorted: &[SortedInput]) -> (Vec<usize>, Vec<usize>) {
  let mut alice = Vec::new();
  let mut bob = Vec::new();
  for s in sorted {
    match s.owner {
      OwnedInputIndex::Alice => alice.push(s.sorted_index),
      OwnedInputIndex::Bob => bob.push(s.sorted_index),
    }
  }
  (alice, bob)
}

/// Encode key images from sorted list.
pub fn sorted_key_images_bytes(sorted: &[SortedInput]) -> Vec<[u8; 32]> {
  sorted.iter().map(|s| s.key_image.to_bytes()).collect()
}

/// Read a scalar from 32 bytes (canonical).
pub fn scalar_from_bytes(bytes: &[u8; 32]) -> Result<Scalar> {
  Ok(Scalar::read(&mut bytes.as_slice())?)
}

/// Build a Skeleton message once Bob has partials.
pub fn skeleton_message(
  session_id: SessionId,
  unsigned: &UnsignedSkeleton,
  bob: Contribution,
  bob_partials: PartialSignatures,
) -> Skeleton {
  let (alice_sorted_indices, bob_sorted_indices) = partition_owners(&unsigned.sorted);
  // Placeholders so unsigned_tx round-trips on the wire (empty CLSAG vecs do not).
  let wire_tx = fill_placeholder_clsags(unsigned.unsigned_tx.clone(), &unsigned.sorted)
    .expect("placeholder clsags");
  // signature_hash is independent of CLSAG placeholders (bulletproof-only transcript).
  debug_assert_eq!(
    wire_tx.signature_hash().expect("sig hash"),
    unsigned.signature_hash
  );
  Skeleton {
    session_id,
    signable_tx: unsigned.signable_bytes.clone(),
    key_images_sorted: sorted_key_images_bytes(&unsigned.sorted),
    bob_sorted_indices,
    alice_sorted_indices,
    unsigned_tx: wire_tx.serialize(),
    signature_hash: unsigned.signature_hash,
    fee: unsigned.fee,
    bob,
    bob_partials,
  }
}
