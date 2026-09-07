//! Spends of the P2WSH 2-of-2: cooperative close and nLockTime burn.

use bitcoin::blockdata::script::Builder;
use bitcoin::opcodes::all::OP_RETURN;
use bitcoin::psbt::Psbt;
use bitcoin::script::PushBytesBuf;
use bitcoin::secp256k1::PublicKey as SecpPk;
use bitcoin::{
    absolute::LockTime, Address, Amount, OutPoint, PublicKey, ScriptBuf, Sequence, Transaction,
    TxIn, TxOut, Witness,
};

use crate::p2wsh::Escrow;
use crate::Error;

const DUST: u64 = 546;
pub const BURN_TAG: &[u8] = b"hbp-burn";

#[derive(Debug, Clone)]
pub struct CoopOutput {
    pub address: Address,
    pub sats: u64,
}

pub fn build_burn_psbt(
    escrow: &Escrow,
    outpoint: OutPoint,
    sats: u64,
    t_unix: u32,
) -> Result<Psbt, Error> {
    if t_unix < 500_000_000 {
        return Err(Error::msg("burn locktime must be unix time"));
    }
    let mut data = PushBytesBuf::new();
    data.extend_from_slice(BURN_TAG)
        .map_err(|_| Error::msg("OP_RETURN payload"))?;
    let opreturn = Builder::new()
        .push_opcode(OP_RETURN)
        .push_slice(&data)
        .into_script();
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::from_time(t_unix).map_err(|e| Error::msg(e.to_string()))?,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(0),
            script_pubkey: opreturn,
        }],
    };
    // Entire value minus 0 = miner fee.
    let _ = sats;
    wsh_psbt(tx, escrow, sats)
}

pub fn build_coop_psbt(
    escrow: &Escrow,
    outpoint: OutPoint,
    sats: u64,
    outputs: &[CoopOutput],
    fee: u64,
) -> Result<Psbt, Error> {
    if fee == 0 {
        return Err(Error::msg("fee must be > 0"));
    }
    let out_sum: u64 = outputs.iter().map(|o| o.sats).sum();
    let need = out_sum
        .checked_add(fee)
        .ok_or_else(|| Error::msg("overflow"))?;
    if need != sats {
        return Err(Error::msg(format!(
            "outputs {out_sum} + fee {fee} must equal escrow {sats}"
        )));
    }
    for o in outputs {
        if o.sats < DUST {
            return Err(Error::msg("coop output below dust"));
        }
    }
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: outputs
            .iter()
            .map(|o| TxOut {
                value: Amount::from_sat(o.sats),
                script_pubkey: o.address.script_pubkey(),
            })
            .collect(),
    };
    wsh_psbt(tx, escrow, sats)
}

fn wsh_psbt(tx: Transaction, escrow: &Escrow, sats: u64) -> Result<Psbt, Error> {
    let mut psbt = Psbt::from_unsigned_tx(tx).map_err(|e| Error::msg(e.to_string()))?;
    psbt.inputs[0].witness_utxo = Some(TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey.clone(),
    });
    psbt.inputs[0].witness_script = Some(escrow.witness_script.clone());
    Ok(psbt)
}

pub fn extract_wsh_tx(mut psbt: Psbt) -> Result<Transaction, Error> {
    if psbt.inputs.len() != 1 {
        return Err(Error::msg("wsh spend must have exactly one input"));
    }
    finalize_wsh_input(&mut psbt.inputs[0])?;
    psbt.extract_tx().map_err(|e| Error::msg(e.to_string()))
}

fn finalize_wsh_input(input: &mut bitcoin::psbt::Input) -> Result<(), Error> {
    if input.final_script_witness.is_some() {
        return Ok(());
    }
    let ws = input
        .witness_script
        .as_ref()
        .ok_or_else(|| Error::msg("missing witness_script"))?;
    let pks = script_pubkeys(ws)?;
    let mut w = Witness::new();
    w.push([]); // CHECKMULTISIG dummy element
    for pk in &pks {
        let btc_pk = PublicKey::new(*pk);
        let sig = input
            .partial_sigs
            .get(&btc_pk)
            .ok_or_else(|| Error::msg(format!("missing signature for {pk}")))?;
        w.push(sig.to_vec());
    }
    w.push(ws.as_bytes());
    input.final_script_witness = Some(w);
    Ok(())
}

fn script_pubkeys(script: &ScriptBuf) -> Result<Vec<SecpPk>, Error> {
    let mut pks = Vec::new();
    for instr in script.instructions() {
        let instr = instr.map_err(|e| Error::msg(e.to_string()))?;
        if let bitcoin::blockdata::script::Instruction::PushBytes(b) = instr {
            if b.len() == 33 {
                pks.push(SecpPk::from_slice(b.as_bytes()).map_err(|e| Error::msg(e.to_string()))?);
            }
        }
    }
    if pks.len() != 2 {
        return Err(Error::msg("witness script is not 2-of-2"));
    }
    Ok(pks)
}
