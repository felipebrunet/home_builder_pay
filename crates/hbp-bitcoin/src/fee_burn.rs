//! Dual-deadline miner-fee burn (product no-agreement path).
//!
//! # Why this shape (and not NUMS / anyone-can-spend)
//!
//! Bitcoin Script cannot constrain *outputs* without a covenant (CTV / APO are
//! not active). That rules out an `after(t)` leaf that “must” pay miners:
//!
//! - **NUMS / unspendable key** (legacy MAD): the coins sit in the UTXO set
//!   forever. Miners never receive them. The product forbids this.
//! - **Anyone-can-spend after T**: a third party can sweep to themselves.
//!   That is a bounty, not a miner-fee burn.
//!
//! The only enforceable fee-burn without covenants is a **MuSig2 key-path
//! transaction that both parties sign after the funding outpoint is known**,
//! with `SIGHASH_DEFAULT` binding the exact outputs. Either party (or anyone
//! holding the signed hex) can broadcast after `nLockTime`.
//!
//! Cooperative close stays the same key-path: a *different* signed spend of
//! the same UTXO, broadcast before the burns if both agree.
//!
//! # Exact transaction shapes
//!
//! Funding UTXO (bond **or** active partida), `N` sats, key-path-only:
//!
//! ```text
//! tr(musig(M, C))    // no unwind leaf — burn is the only no-agreement path
//! ```
//!
//! **t1** (`nLockTime = t1`, `nSequence = ENABLE_LOCKTIME_NO_RBF`):
//!
//! ```text
//! vin[0]  = funding outpoint (N)
//! vout[0] = continuation, same tr(musig(M,C)), floor(N/2)   // must be ≥ dust
//! fee     = N − floor(N/2)                                  // the burned half
//! ```
//!
//! The continuation is required: a single tx cannot both give 50% to miners
//! and leave 50% locked until `t2` without an output. SegWit txid excludes
//! the witness, so t2 can be built against t1's txid before signatures exist.
//!
//! **t2** (`nLockTime = t2`, `nSequence = ENABLE_LOCKTIME_NO_RBF`):
//!
//! ```text
//! vin[0]  = t1 vout[0] (floor(N/2))
//! vout[0] = 0-value OP_RETURN "hbp-feeburn"   // consensus needs ≥1 output
//! fee     = floor(N/2)                         // remaining half → miners
//! ```
//!
//! Both bond and partida use this shape independently (same t1/t2).
//!
//! # Arming
//!
//! After funding confirms, both parties MuSig2-sign t1 and t2 and exchange
//! the hex (`06-feeburn.json`). Until those signatures exist the UTXO can
//! only move by a later cooperative close — do not start work until armed.

use bitcoin::absolute::LockTime;
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::{Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut};
use serde::{Deserialize, Serialize};

use crate::taproot::Escrow;
use crate::Error;

/// Bitcoin dust threshold used for the t1 continuation.
pub const DUST_SATS: u64 = 546;

/// Marker in the t2 OP_RETURN so explorers can identify a fee-burn.
pub const T2_OP_RETURN: [u8; 11] = *b"hbp-feeburn";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeBurnSplit {
    pub input_sats: u64,
    /// `floor(input / 2)` — continuation at t1 / input of t2.
    pub continuation_sats: u64,
    /// `input - continuation` — miner fee at t1.
    pub fee_sats: u64,
}

pub fn fee_burn_split(input_sats: u64) -> Result<FeeBurnSplit, Error> {
    if input_sats < 2 * DUST_SATS {
        return Err(Error::msg(format!(
            "fee-burn input {input_sats} sats is below 2×dust ({})",
            2 * DUST_SATS
        )));
    }
    let continuation_sats = input_sats / 2;
    if continuation_sats < DUST_SATS {
        return Err(Error::msg("fee-burn continuation would be dust"));
    }
    Ok(FeeBurnSplit {
        input_sats,
        continuation_sats,
        fee_sats: input_sats - continuation_sats,
    })
}

fn locktime_input(outpoint: OutPoint) -> TxIn {
    TxIn {
        previous_output: outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
        witness: bitcoin::Witness::new(),
    }
}

/// Unsigned t1: half continues, half is the miner fee. `nLockTime = t1`.
pub fn build_fee_burn_t1_tx(
    outpoint: OutPoint,
    input_sats: u64,
    continuation_spk: ScriptBuf,
    t1: u32,
) -> Result<Transaction, Error> {
    let split = fee_burn_split(input_sats)?;
    Ok(Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::from_consensus(t1),
        input: vec![locktime_input(outpoint)],
        output: vec![TxOut {
            value: Amount::from_sat(split.continuation_sats),
            script_pubkey: continuation_spk,
        }],
    })
}

/// Unsigned t2: remaining half consumed as fees. One 0-value OP_RETURN.
pub fn build_fee_burn_t2_tx(
    outpoint: OutPoint,
    input_sats: u64,
    t2: u32,
) -> Result<Transaction, Error> {
    if input_sats < DUST_SATS {
        return Err(Error::msg(format!(
            "fee-burn t2 input {input_sats} is below dust"
        )));
    }
    Ok(Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::from_consensus(t2),
        input: vec![locktime_input(outpoint)],
        output: vec![TxOut {
            value: Amount::ZERO,
            script_pubkey: ScriptBuf::new_op_return(T2_OP_RETURN),
        }],
    })
}

/// Build the t1→t2 chain for one UTXO. Continuation uses the same scriptPubKey
/// as the funding escrow (key-path-only `tr(musig(M,C))`).
pub fn build_fee_burn_chain(
    funding: OutPoint,
    input_sats: u64,
    escrow: &Escrow,
    t1: u32,
    t2: u32,
) -> Result<(Transaction, Transaction, FeeBurnSplit), Error> {
    if !escrow.is_key_path_only() {
        return Err(Error::msg(
            "fee-burn chain requires a key-path-only escrow (no unwind leaf)",
        ));
    }
    let split = fee_burn_split(input_sats)?;
    let t1_tx = build_fee_burn_t1_tx(funding, input_sats, escrow.script_pubkey(), t1)?;
    let t2_tx = build_fee_burn_t2_tx(
        OutPoint {
            txid: t1_tx.compute_txid(),
            vout: 0,
        },
        split.continuation_sats,
        t2,
    )?;
    Ok((t1_tx, t2_tx, split))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeBurnPlan {
    pub kind: String,
    pub partida_id: Option<u32>,
    pub funding_outpoint: String,
    pub funding_sats: u64,
    pub t1: u32,
    pub t2: u32,
    pub continuation_sats: u64,
    pub t1_fee_sats: u64,
    pub t2_fee_sats: u64,
    pub continuation_spk: String,
    pub t1_txid: String,
    pub t1_tx_hex: String,
    pub t2_tx_hex: String,
}

pub fn fee_burn_plan(
    kind: &str,
    partida_id: Option<u32>,
    funding: OutPoint,
    input_sats: u64,
    escrow: &Escrow,
    t1: u32,
    t2: u32,
) -> Result<FeeBurnPlan, Error> {
    let (t1_tx, t2_tx, split) = build_fee_burn_chain(funding, input_sats, escrow, t1, t2)?;
    Ok(FeeBurnPlan {
        kind: kind.to_string(),
        partida_id,
        funding_outpoint: funding.to_string(),
        funding_sats: input_sats,
        t1,
        t2,
        continuation_sats: split.continuation_sats,
        t1_fee_sats: split.fee_sats,
        t2_fee_sats: split.continuation_sats,
        continuation_spk: hex::encode(escrow.script_pubkey().as_bytes()),
        t1_txid: t1_tx.compute_txid().to_string(),
        t1_tx_hex: serialize_hex(&t1_tx),
        t2_tx_hex: serialize_hex(&t2_tx),
    })
}

pub fn assert_fee_burn_t1_shape(
    tx: &Transaction,
    input_sats: u64,
    continuation_spk: &ScriptBuf,
    t1: u32,
) -> Result<FeeBurnSplit, Error> {
    let split = fee_burn_split(input_sats)?;
    if tx.lock_time.to_consensus_u32() != t1 {
        return Err(Error::msg("t1 locktime mismatch"));
    }
    if tx.input.len() != 1 || tx.output.len() != 1 {
        return Err(Error::msg("t1 must be 1-in 1-out (continuation)"));
    }
    if tx.input[0].sequence != Sequence::ENABLE_LOCKTIME_NO_RBF {
        return Err(Error::msg("t1 sequence must enable locktime"));
    }
    if tx.output[0].value.to_sat() != split.continuation_sats {
        return Err(Error::msg("t1 continuation amount mismatch"));
    }
    if tx.output[0].script_pubkey != *continuation_spk {
        return Err(Error::msg("t1 continuation script mismatch"));
    }
    let implied_fee = input_sats.saturating_sub(split.continuation_sats);
    if implied_fee != split.fee_sats {
        return Err(Error::msg("t1 fee is not the burned half"));
    }
    Ok(split)
}

pub fn assert_fee_burn_t2_shape(tx: &Transaction, input_sats: u64, t2: u32) -> Result<(), Error> {
    if tx.lock_time.to_consensus_u32() != t2 {
        return Err(Error::msg("t2 locktime mismatch"));
    }
    if tx.input.len() != 1 || tx.output.len() != 1 {
        return Err(Error::msg("t2 must be 1-in 1-out (OP_RETURN)"));
    }
    if tx.input[0].sequence != Sequence::ENABLE_LOCKTIME_NO_RBF {
        return Err(Error::msg("t2 sequence must enable locktime"));
    }
    if tx.output[0].value != Amount::ZERO {
        return Err(Error::msg(
            "t2 output must be 0 so the remainder is miner fee",
        ));
    }
    if !tx.output[0].script_pubkey.is_op_return() {
        return Err(Error::msg("t2 output must be OP_RETURN"));
    }
    let _ = input_sats;
    Ok(())
}
