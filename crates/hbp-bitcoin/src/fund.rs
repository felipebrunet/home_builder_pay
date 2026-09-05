//! Unsigned funding PSBT: escrow outputs are exact; fee comes from change.

use bitcoin::psbt::Psbt;
use bitcoin::{
    absolute::LockTime, Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut,
    Witness,
};

use hbp_core::Role;

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
    pub bond: Option<(ScriptBuf, u64)>,
    pub partida: (ScriptBuf, u64),
    pub mad: Option<(ScriptBuf, u64)>,
    pub fee: u64,
    pub mandante: FundingCoin,
    pub mandante_change: Address,
    pub contratista: Option<FundingCoin>,
    pub contratista_change: Option<Address>,
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
    let partida_sats = req.partida.1;
    let bond_sats = req.bond.as_ref().map(|b| b.1).unwrap_or(0);
    let mad_sats = req.mad.as_ref().map(|m| m.1).unwrap_or(0);
    if req.fee == 0 {
        return Err(Error::msg("fee must be > 0"));
    }

    let mut outputs = Vec::new();
    if let Some((script, sats)) = &req.bond {
        outputs.push(TxOut {
            value: Amount::from_sat(*sats),
            script_pubkey: script.clone(),
        });
    }
    outputs.push(TxOut {
        value: Amount::from_sat(partida_sats),
        script_pubkey: req.partida.0.clone(),
    });
    if let Some((script, sats)) = &req.mad {
        outputs.push(TxOut {
            value: Amount::from_sat(*sats),
            script_pubkey: script.clone(),
        });
    }

    let mad_m = mad_sats / 2;
    let mad_c = mad_sats - mad_m;
    let fee_m = if req.contratista.is_some() {
        req.fee / 2
    } else {
        req.fee
    };
    let fee_c = req.fee.saturating_sub(fee_m);

    let m_need = partida_sats
        .checked_add(mad_m)
        .and_then(|v| v.checked_add(fee_m))
        .ok_or_else(|| Error::msg("mandante funding overflow"))?;
    if req.mandante.sats < m_need {
        return Err(Error::msg(format!(
            "mandante input {} < need {m_need} (partida + mad share + fee)",
            req.mandante.sats
        )));
    }
    let m_change = req.mandante.sats - m_need;
    if m_change >= DUST {
        outputs.push(TxOut {
            value: Amount::from_sat(m_change),
            script_pubkey: req.mandante_change.script_pubkey(),
        });
    } else if m_change != 0 {
        return Err(Error::msg(
            "mandante change would be dust; use a larger input or pay extra fee",
        ));
    }

    let mut inputs = vec![TxIn {
        previous_output: req.mandante.outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    }];
    let mut witness_utxos = vec![TxOut {
        value: Amount::from_sat(req.mandante.sats),
        script_pubkey: req.mandante.script_pubkey.clone(),
    }];

    if let Some(c) = &req.contratista {
        let c_need = bond_sats
            .checked_add(mad_c)
            .and_then(|v| v.checked_add(fee_c))
            .ok_or_else(|| Error::msg("contratista funding overflow"))?;
        if c.sats < c_need {
            return Err(Error::msg(format!(
                "contratista input {} < need {c_need} (bond + mad share + fee)",
                c.sats
            )));
        }
        let c_change = c.sats - c_need;
        let chg = req
            .contratista_change
            .as_ref()
            .ok_or_else(|| Error::msg("contratista change address required"))?;
        if c_change >= DUST {
            outputs.push(TxOut {
                value: Amount::from_sat(c_change),
                script_pubkey: chg.script_pubkey(),
            });
        } else if c_change != 0 {
            return Err(Error::msg(
                "contratista change would be dust; use a larger input or pay extra fee",
            ));
        }
        inputs.push(TxIn {
            previous_output: c.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        });
        witness_utxos.push(TxOut {
            value: Amount::from_sat(c.sats),
            script_pubkey: c.script_pubkey.clone(),
        });
    } else if req.bond.is_some() {
        return Err(Error::msg(
            "bond output requires a contratista input (--c-outpoint)",
        ));
    }

    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: inputs,
        output: outputs,
    };
    Ok((tx, witness_utxos))
}

/// Attach the full previous transaction when we have it (helps Blue/Sparrow on P2WPKH).
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

/// Merge PSBTs that share the same unsigned tx (each Blue signs its own input).
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

/// Finalize singlesig inputs Blue can sign (P2WPKH / P2TR key-path) and extract the tx.
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
            "missing signature (Blue must sign this input and not broadcast yet)",
        ));
    }
    Err(Error::msg(
        "cannot finalize: more than one partial signature on a singlesig input",
    ))
}

/// What this party must cover on a 2-input boleta+P1 fund (fee split in half).
pub fn funding_share(
    bond_sats: u64,
    partida_sats: u64,
    fee: u64,
    role: Role,
) -> Result<u64, Error> {
    if fee == 0 {
        return Err(Error::msg("fee must be > 0"));
    }
    let fee_m = fee / 2;
    let fee_c = fee.saturating_sub(fee_m);
    match role {
        Role::Mandante => partida_sats
            .checked_add(fee_m)
            .ok_or_else(|| Error::msg("mandante funding overflow")),
        Role::Contratista => bond_sats
            .checked_add(fee_c)
            .ok_or_else(|| Error::msg("contratista funding overflow")),
    }
}

fn push_change(
    outputs: &mut Vec<TxOut>,
    coin: &FundingCoin,
    need: u64,
    change: &Address,
) -> Result<(), Error> {
    if coin.sats < need {
        return Err(Error::msg(format!(
            "input {} < need {need} (share + fee)",
            coin.sats
        )));
    }
    let leftover = coin.sats - need;
    if leftover >= DUST {
        outputs.push(TxOut {
            value: Amount::from_sat(leftover),
            script_pubkey: change.script_pubkey(),
        });
    } else if leftover != 0 {
        return Err(Error::msg(
            "change would be dust; pick a larger coin or a higher fee",
        ));
    }
    Ok(())
}

/// One-sided construction: escrow outputs (bond + P1) plus *this* party's input/change.
/// The unsigned tx does not balance until the peer adds their coin.
pub fn build_partial_funding_psbt(
    bond: (ScriptBuf, u64),
    partida: (ScriptBuf, u64),
    fee: u64,
    role: Role,
    coin: &FundingCoin,
    change: &Address,
) -> Result<Psbt, Error> {
    let need = funding_share(bond.1, partida.1, fee, role)?;
    let mut outputs = vec![
        TxOut {
            value: Amount::from_sat(bond.1),
            script_pubkey: bond.0,
        },
        TxOut {
            value: Amount::from_sat(partida.1),
            script_pubkey: partida.0,
        },
    ];
    push_change(&mut outputs, coin, need, change)?;
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: coin.outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: outputs,
    };
    let mut psbt = Psbt::from_unsigned_tx(tx).map_err(|e| Error::msg(e.to_string()))?;
    psbt.inputs[0].witness_utxo = Some(TxOut {
        value: Amount::from_sat(coin.sats),
        script_pubkey: coin.script_pubkey.clone(),
    });
    Ok(psbt)
}

/// Add the second party's input + change to a one-input partial. Escrow amounts stay exact.
pub fn complete_partial_funding_psbt(
    psbt: &mut Psbt,
    bond: (ScriptBuf, u64),
    partida: (ScriptBuf, u64),
    fee: u64,
    role: Role,
    coin: &FundingCoin,
    change: &Address,
) -> Result<(), Error> {
    if psbt.unsigned_tx.input.len() != 1 {
        return Err(Error::msg(format!(
            "expected a 1-input partial, this PSBT has {} inputs",
            psbt.unsigned_tx.input.len()
        )));
    }
    if psbt.unsigned_tx.input[0].previous_output == coin.outpoint {
        return Err(Error::msg("that coin is already in the PSBT"));
    }
    let tx = &psbt.unsigned_tx;
    let saw_bond = tx
        .output
        .iter()
        .any(|o| o.script_pubkey == bond.0 && o.value.to_sat() == bond.1);
    let saw_part = tx
        .output
        .iter()
        .any(|o| o.script_pubkey == partida.0 && o.value.to_sat() == partida.1);
    if !saw_bond || !saw_part {
        return Err(Error::msg(
            "partial PSBT is missing the exact boleta + partida 1 outputs",
        ));
    }
    let need = funding_share(bond.1, partida.1, fee, role)?;
    psbt.unsigned_tx.input.push(TxIn {
        previous_output: coin.outpoint,
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    });
    psbt.inputs.push(bitcoin::psbt::Input {
        witness_utxo: Some(TxOut {
            value: Amount::from_sat(coin.sats),
            script_pubkey: coin.script_pubkey.clone(),
        }),
        ..Default::default()
    });
    if coin.sats < need {
        return Err(Error::msg(format!(
            "input {} < need {need} (share + fee)",
            coin.sats
        )));
    }
    let leftover = coin.sats - need;
    if leftover >= DUST {
        psbt.unsigned_tx.output.push(TxOut {
            value: Amount::from_sat(leftover),
            script_pubkey: change.script_pubkey(),
        });
    } else if leftover != 0 {
        return Err(Error::msg(
            "change would be dust; pick a larger coin or a higher fee",
        ));
    }
    Ok(())
}

pub fn psbt_signed_input_count(psbt: &Psbt) -> usize {
    psbt.inputs
        .iter()
        .filter(|i| {
            i.final_script_witness.is_some()
                || i.tap_key_sig.is_some()
                || !i.partial_sigs.is_empty()
        })
        .count()
}
