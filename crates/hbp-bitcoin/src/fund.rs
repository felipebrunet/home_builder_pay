//! Unsigned funding PSBT: one P2WSH escrow output; fee from change.

use bitcoin::psbt::Psbt;
use bitcoin::{
    absolute::LockTime, Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness,
};

use crate::Error;

const DUST: u64 = 546;

#[derive(Debug, Clone)]
pub struct FundingCoin {
    pub outpoint: OutPoint,
    pub sats: u64,
    pub script_pubkey: ScriptBuf,
}

#[derive(Debug, Clone)]
pub struct FundingRequest {
    pub escrow: ScriptBuf,
    pub escrow_sats: u64,
    pub fee: u64,
    pub mandante: FundingCoin,
    pub mandante_change: Address,
    pub contratista: FundingCoin,
    pub contratista_change: Address,
}

pub fn build_funding_psbt(req: &FundingRequest) -> Result<Psbt, Error> {
    let (tx, witness_utxos) = funding_tx(req)?;
    let mut psbt = Psbt::from_unsigned_tx(tx).map_err(|e| Error::msg(e.to_string()))?;
    for (i, utxo) in witness_utxos.into_iter().enumerate() {
        psbt.inputs[i].witness_utxo = Some(utxo);
    }
    Ok(psbt)
}

pub fn funding_tx(req: &FundingRequest) -> Result<(Transaction, Vec<TxOut>), Error> {
    if req.fee == 0 {
        return Err(Error::msg("fee must be > 0"));
    }
    if req.escrow_sats < DUST * 2 {
        return Err(Error::msg("escrow too small"));
    }
    let each = req.escrow_sats / 2;
    if each * 2 != req.escrow_sats {
        return Err(Error::msg("escrow sats must be even (equal contributions)"));
    }
    let fee_m = req.fee / 2;
    let fee_c = req.fee - fee_m;
    let m_need = each
        .checked_add(fee_m)
        .ok_or_else(|| Error::msg("overflow"))?;
    let c_need = each
        .checked_add(fee_c)
        .ok_or_else(|| Error::msg("overflow"))?;
    if req.mandante.sats < m_need {
        return Err(Error::msg(format!(
            "mandante input {} < need {m_need} (escrow half + fee)",
            req.mandante.sats
        )));
    }
    if req.contratista.sats < c_need {
        return Err(Error::msg(format!(
            "contratista input {} < need {c_need} (escrow half + fee)",
            req.contratista.sats
        )));
    }

    let mut outputs = vec![TxOut {
        value: Amount::from_sat(req.escrow_sats),
        script_pubkey: req.escrow.clone(),
    }];
    push_change(
        &mut outputs,
        req.mandante.sats - m_need,
        &req.mandante_change,
        "mandante",
    )?;
    push_change(
        &mut outputs,
        req.contratista.sats - c_need,
        &req.contratista_change,
        "contratista",
    )?;

    let inputs = vec![
        txin(req.mandante.outpoint),
        txin(req.contratista.outpoint),
    ];
    let witness_utxos = vec![
        TxOut {
            value: Amount::from_sat(req.mandante.sats),
            script_pubkey: req.mandante.script_pubkey.clone(),
        },
        TxOut {
            value: Amount::from_sat(req.contratista.sats),
            script_pubkey: req.contratista.script_pubkey.clone(),
        },
    ];
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    };
    Ok((tx, witness_utxos))
}

fn txin(op: OutPoint) -> TxIn {
    TxIn {
        previous_output: op,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    }
}

fn push_change(
    outputs: &mut Vec<TxOut>,
    change: u64,
    addr: &Address,
    who: &str,
) -> Result<(), Error> {
    if change >= DUST {
        outputs.push(TxOut {
            value: Amount::from_sat(change),
            script_pubkey: addr.script_pubkey(),
        });
    } else if change != 0 {
        return Err(Error::msg(format!(
            "{who} change would be dust; use a larger input or pay extra fee"
        )));
    }
    Ok(())
}

pub fn attach_prev_tx(psbt: &mut Psbt, outpoint: OutPoint, prev: Transaction) -> Result<(), Error> {
    let want = prev.compute_txid();
    if want != outpoint.txid {
        return Err(Error::msg(format!(
            "prev tx {want} does not match outpoint {}",
            outpoint.txid
        )));
    }
    let pos = psbt
        .unsigned_tx
        .input
        .iter()
        .position(|i| i.previous_output == outpoint)
        .ok_or_else(|| Error::msg(format!("psbt has no input {outpoint}")))?;
    psbt.inputs[pos].non_witness_utxo = Some(prev);
    Ok(())
}

pub fn combine_psbts(parts: &[Psbt]) -> Result<Psbt, Error> {
    if parts.is_empty() {
        return Err(Error::msg("need at least one PSBT"));
    }
    let mut acc = parts[0].clone();
    for extra in &parts[1..] {
        acc.combine(extra.clone())
            .map_err(|e| Error::msg(e.to_string()))?;
    }
    Ok(acc)
}

/// Finalize singlesig inputs (P2WPKH / P2TR key-path) and extract.
pub fn extract_signed_funding_tx(mut psbt: Psbt) -> Result<Transaction, Error> {
    for (i, input) in psbt.inputs.iter_mut().enumerate() {
        finalize_singlesig_input(input).map_err(|e| Error::msg(format!("input {i}: {e}")))?;
    }
    psbt.extract_tx().map_err(|e| Error::msg(e.to_string()))
}

fn finalize_singlesig_input(input: &mut bitcoin::psbt::Input) -> Result<(), Error> {
    if input.final_script_witness.is_some() {
        return Ok(());
    }
    if let Some(sig) = input.tap_key_sig {
        input.final_script_witness = Some(Witness::p2tr_key_spend(&sig));
        input.tap_key_sig = None;
        return Ok(());
    }
    if input.partial_sigs.len() == 1 {
        let (pk, sig) = input.partial_sigs.iter().next().expect("len == 1");
        input.final_script_witness = Some(Witness::p2wpkh(sig, &pk.inner));
        input.partial_sigs.clear();
        return Ok(());
    }
    if input.partial_sigs.is_empty() && input.tap_key_sig.is_none() {
        return Err(Error::msg(
            "missing signature (wallet must sign this input and not broadcast yet)",
        ));
    }
    Err(Error::msg(
        "cannot finalize: more than one partial signature on a singlesig input",
    ))
}
