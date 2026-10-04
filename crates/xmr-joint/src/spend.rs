//! Gasto FROST de la caja. Dos destinos: contratista y mandante.
//!
//! Con un porcentaje P el contratista cobra P del principal más la garantía entera.
//! El mandante cobra el resto del principal. El fee sale primero de ese resto.
//! Si un lado quedaría en cero, se le deja 1 piconero para que la transacción tenga dos salidas.

use std::collections::HashMap;
use std::io::Cursor;

use frost::sign::{PreprocessMachine, SignMachine, SignatureMachine, Writable};
use monero_wallet::{
    address::MoneroAddress,
    ringct::RctType,
    send::{
        Change, SignableTransaction, TransactionPreprocess, TransactionSignMachine,
        TransactionSignatureMachine,
    },
    transaction::Transaction,
    OutputWithDecoys,
};
use rand_core::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};

use crate::dkg::JointAccount;
use crate::{Error, Result};

/// Reparto en piconero. `contratista + mandante + fee` es el pote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub contratista: u64,
    pub mandante: u64,
    pub fee: u64,
}

/// `pot` tiene que ser `2 * capital` (las dos salidas del fondeo).
pub fn split_pot(pot: u64, capital: u64, pct: u32, fee: u64) -> Result<Split> {
    if pct > 100 {
        return Err(Error::Spend("el porcentaje pasa de 100".into()));
    }
    if capital == 0 || pot != capital.saturating_mul(2) {
        return Err(Error::Spend("el pote no es 2 × capital".into()));
    }
    if fee >= pot {
        return Err(Error::Spend("el fee se come el pote".into()));
    }
    let principal = capital * u64::from(pct) / 100;
    let mut to_contratista = capital + principal;
    let mut to_mandante = capital - principal;
    if to_mandante >= fee {
        to_mandante -= fee;
    } else {
        let short = fee - to_mandante;
        to_mandante = 0;
        if to_contratista < short {
            return Err(Error::Spend("no alcanza para el fee".into()));
        }
        to_contratista -= short;
    }
    if to_mandante == 0 {
        if to_contratista < 2 {
            return Err(Error::Spend("no queda dust para el mandante".into()));
        }
        to_contratista -= 1;
        to_mandante = 1;
    }
    if to_contratista == 0 {
        if to_mandante < 2 {
            return Err(Error::Spend("no queda dust para el contratista".into()));
        }
        to_mandante -= 1;
        to_contratista = 1;
    }
    if to_contratista + to_mandante + fee != pot {
        return Err(Error::Spend("el reparto no cierra".into()));
    }
    Ok(Split {
        contratista: to_contratista,
        mandante: to_mandante,
        fee,
    })
}

/// Propuesta que viaja al otro antes de firmar.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpendProposal {
    pub obra_id: String,
    pub capital: u64,
    pub pct: u32,
    pub contratista: String,
    pub mandante: String,
    pub signable: Vec<u8>,
}

/// Arma la transacción sin firmar. El fee se ajusta al que oxide calcula.
pub fn propose<R: RngCore + CryptoRng>(
    rng: &mut R,
    obra_id: &str,
    capital: u64,
    pct: u32,
    contratista: &MoneroAddress,
    mandante: &MoneroAddress,
    inputs: Vec<OutputWithDecoys>,
    fee_rate: monero_wallet::interface::FeeRate,
) -> Result<(SpendProposal, Split)> {
    let pot: u64 = inputs.iter().map(|i| i.commitment().amount).sum();
    let mut fee_guess = 0u64;
    let mut last_err = None;
    for _ in 0..6 {
        let split = split_pot(pot, capital, pct, fee_guess)?;
        let mut ovk = [0u8; 32];
        rng.fill_bytes(&mut ovk);
        let built = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            zeroize::Zeroizing::new(ovk),
            inputs.clone(),
            vec![
                (*contratista, split.contratista),
                (*mandante, split.mandante),
            ],
            Change::fingerprintable(None),
            vec![],
            fee_rate.clone(),
        );
        match built {
            Ok(tx) => {
                let need = tx.necessary_fee();
                if need == fee_guess {
                    let proposal = SpendProposal {
                        obra_id: obra_id.to_string(),
                        capital,
                        pct,
                        contratista: contratista.to_string(),
                        mandante: mandante.to_string(),
                        signable: tx.serialize(),
                    };
                    verify_proposal_tx(&tx, &proposal, &split)?;
                    return Ok((proposal, split));
                }
                fee_guess = need;
            }
            Err(monero_wallet::send::SendError::NotEnoughFunds {
                necessary_fee: Some(need),
                ..
            }) => {
                fee_guess = need;
                last_err = Some(format!("fondos, fee {need}"));
            }
            Err(e) => return Err(Error::Spend(e.to_string())),
        }
    }
    Err(Error::Spend(
        last_err.unwrap_or_else(|| "no cerró el fee del gasto".into()),
    ))
}

fn verify_proposal_tx(tx: &SignableTransaction, proposal: &SpendProposal, split: &Split) -> Result<()> {
    if tx.input_sum() != proposal.capital.saturating_mul(2) {
        return Err(Error::Spend("los inputs no son 2 × capital".into()));
    }
    if tx.necessary_fee() != split.fee {
        return Err(Error::Spend("el fee de la transacción no es el del reparto".into()));
    }
    let mut got_c = None;
    let mut got_m = None;
    for (addr, amount) in tx.external_payments() {
        let text = addr.to_string();
        if text == proposal.contratista {
            got_c = Some(amount);
        } else if text == proposal.mandante {
            got_m = Some(amount);
        } else {
            return Err(Error::Spend(format!("destino inesperado {text}")));
        }
    }
    if got_c != Some(split.contratista) || got_m != Some(split.mandante) {
        return Err(Error::Spend("los montos no son el reparto".into()));
    }
    Ok(())
}

/// Sesión de firma de una de las dos partes. Primero el preprocess, después el share.
pub struct SpendSession {
    sign_machine: TransactionSignMachine,
    peer_is_one: bool,
}

pub struct SpendSigned {
    machine: TransactionSignatureMachine,
    peer_is_one: bool,
}

impl SpendSession {
    /// Verifica el reparto y devuelve el preprocess para el otro.
    pub fn open<R: RngCore + CryptoRng>(
        account: &JointAccount,
        proposal: &SpendProposal,
        rng: &mut R,
    ) -> Result<(Self, Vec<u8>)> {
        if proposal.obra_id != account.obra_id() {
            return Err(Error::Spend("la propuesta es de otra obra".into()));
        }
        let tx = SignableTransaction::read(&mut Cursor::new(proposal.signable.as_slice()))
            .map_err(|e| Error::Spend(format!("signable: {e}")))?;
        let split = split_pot(tx.input_sum(), proposal.capital, proposal.pct, tx.necessary_fee())?;
        verify_proposal_tx(&tx, proposal, &split)?;
        let machine = tx
            .multisig(account.keys())
            .map_err(|e| Error::Spend(e.to_string()))?;
        let (sign_machine, preprocess) = machine.preprocess(rng);
        Ok((
            Self {
                sign_machine,
                peer_is_one: account.role() != crate::dkg::Party::Mandante,
            },
            write_pre(&preprocess)?,
        ))
    }

    pub fn sign(self, peer_preprocess: &[u8]) -> Result<(SpendSigned, Vec<u8>)> {
        let peer_pre = self
            .sign_machine
            .read_preprocess(&mut Cursor::new(peer_preprocess))
            .map_err(|e| Error::Spend(format!("preprocess: {e}")))?;
        let mut map = HashMap::new();
        map.insert(peer_participant(self.peer_is_one), peer_pre);
        let (machine, share) = self
            .sign_machine
            .sign(map, b"")
            .map_err(|e| Error::Spend(e.to_string()))?;
        let bytes = write_share(&share)?;
        Ok((
            SpendSigned {
                machine,
                peer_is_one: self.peer_is_one,
            },
            bytes,
        ))
    }
}

impl SpendSigned {
    pub fn complete(self, peer_share: &[u8]) -> Result<Transaction> {
        let share = self
            .machine
            .read_share(&mut Cursor::new(peer_share))
            .map_err(|e| Error::Spend(format!("share: {e}")))?;
        let mut map = HashMap::new();
        map.insert(peer_participant(self.peer_is_one), share);
        self.machine
            .complete(map)
            .map_err(|e| Error::Spend(e.to_string()))
    }
}

fn peer_participant(peer_is_one: bool) -> frost::Participant {
    frost::Participant::new(if peer_is_one { 1 } else { 2 }).expect("1 o 2")
}

fn write_pre(pre: &TransactionPreprocess) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    pre.write(&mut buf).map_err(|e| Error::Spend(e.to_string()))?;
    Ok(buf)
}

fn write_share(share: &monero_wallet::send::TransactionSignatureShare) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    share.write(&mut buf).map_err(|e| Error::Spend(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ochenta_deja_el_resto_al_mandante_despues_del_fee() {
        let split = split_pot(20_000, 10_000, 80, 100).unwrap();
        assert_eq!(split.contratista, 18_000);
        assert_eq!(split.mandante, 1_900);
        assert_eq!(split.fee, 100);
    }

    #[test]
    fn cien_deja_un_piconero_al_mandante() {
        let split = split_pot(20_000, 10_000, 100, 50).unwrap();
        assert_eq!(split.contratista, 19_949);
        assert_eq!(split.mandante, 1);
        assert_eq!(
            split.contratista + split.mandante + split.fee,
            20_000
        );
    }

    #[test]
    fn cero_devuelve_la_garantia_y_el_principal() {
        let split = split_pot(20_000, 10_000, 0, 50).unwrap();
        assert_eq!(split.contratista, 10_000);
        assert_eq!(split.mandante, 9_950);
    }
}
