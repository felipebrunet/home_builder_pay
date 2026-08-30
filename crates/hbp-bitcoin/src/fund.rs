//! Unsigned funding PSBT: escrow outputs are exact; fee comes from change.

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
