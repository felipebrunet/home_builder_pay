//! Gasto FROST de la caja. Dos destinos: contratista y mandante.
//!
//! Con un porcentaje P el contratista cobra P del principal más la garantía entera.
//! El mandante cobra el resto del principal. El fee sale primero de ese resto.
//! Si un lado queda en cero, su salida va igual pero con 0 XMR (como la salida dummy de
//! monero-wallet): la red pide al menos dos salidas y una salida RingCT de monto cero es válida.
//! Antes de 0.2.8 se le dejaba 1 piconero; `SpendSession::open` todavía acepta ese reparto viejo.

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
///
/// Un lado puede quedar en 0: esa salida igual se arma (monto cero), así el gasto
/// tiene dos salidas sin regalarle dust a nadie.
pub fn split_pot(pot: u64, capital: u64, pct: u32, fee: u64) -> Result<Split> {
    split_base(pot, capital, pct, fee)
}

/// Reparto de antes de 0.2.8: el lado en cero recibía 1 piconero. Solo para firmar
/// propuestas que armó un par con la versión vieja.
pub fn split_pot_legacy(pot: u64, capital: u64, pct: u32, fee: u64) -> Result<Split> {
    let mut s = split_base(pot, capital, pct, fee)?;
    if s.mandante == 0 {
        if s.contratista < 2 {
            return Err(Error::Spend("no queda dust para el mandante".into()));
        }
        s.contratista -= 1;
        s.mandante = 1;
    }
    if s.contratista == 0 {
        if s.mandante < 2 {
            return Err(Error::Spend("no queda dust para el contratista".into()));
        }
        s.mandante -= 1;
        s.contratista = 1;
    }
    Ok(s)
}

fn split_base(pot: u64, capital: u64, pct: u32, fee: u64) -> Result<Split> {
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
        if verify_proposal_tx(&tx, proposal, &split).is_err() {
            // Par con la versión vieja (1 piconero al lado en cero): mismo pago, se firma.
            let viejo =
                split_pot_legacy(tx.input_sum(), proposal.capital, proposal.pct, tx.necessary_fee())?;
            verify_proposal_tx(&tx, proposal, &viejo)?;
        }
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
    fn cien_deja_cero_al_mandante_sin_dust() {
        let split = split_pot(20_000, 10_000, 100, 50).unwrap();
        assert_eq!(split.contratista, 19_950);
        assert_eq!(split.mandante, 0);
        assert_eq!(split.contratista + split.mandante + split.fee, 20_000);
        // El reparto viejo se sigue reconociendo para firmar propuestas de 0.2.7.
        let viejo = split_pot_legacy(20_000, 10_000, 100, 50).unwrap();
        assert_eq!((viejo.contratista, viejo.mandante), (19_949, 1));
    }

    #[test]
    fn fee_mayor_que_el_resto_del_mandante_lo_deja_en_cero() {
        // 99 %: al mandante le tocan 100, el fee es 150 → mandante 0, el contratista pone 50.
        let split = split_pot(20_000, 10_000, 99, 150).unwrap();
        assert_eq!(split.mandante, 0);
        assert_eq!(split.contratista, 19_850);
    }

    #[test]
    fn cero_devuelve_la_garantia_y_el_principal() {
        let split = split_pot(20_000, 10_000, 0, 50).unwrap();
        assert_eq!(split.contratista, 10_000);
        assert_eq!(split.mandante, 9_950);
    }
}

#[cfg(test)]
mod pruebas_tx {
    use super::*;
    use crate::dkg::{DkgParty, Party};
    use crate::network::Net;
    use crate::prueba_tx::{billetera, fee_rate, montos_para, salida_falsa, verificar};
    use rand_core::OsRng;

    fn cuentas() -> (JointAccount, JointAccount) {
        let mut rng = OsRng;
        let (mut m, c1) = DkgParty::start(Party::Mandante, "obra-dust", Net::Stagenet, &mut rng).unwrap();
        let (mut c, c2) = DkgParty::start(Party::Contratista, "obra-dust", Net::Stagenet, &mut rng).unwrap();
        let s1 = m.ingest_commit(&c2, &mut rng).unwrap();
        let s2 = c.ingest_commit(&c1, &mut rng).unwrap();
        c.ingest_share(&s1, &mut rng).unwrap();
        let hecho = m.ingest_share(&s2, &mut rng).unwrap();
        let cuenta_c = c.ingest_view(&hecho.view.unwrap()).unwrap();
        (hecho.account.unwrap(), cuenta_c)
    }

    /// Gasto 2-de-2 completo (propuesta, preprocess, shares, firma) sobre un fondeo falso.
    fn pagar(pct: u32) -> (Split, Vec<u64>, Vec<u64>) {
        let (cuenta_m, cuenta_c) = cuentas();
        let capital = 20_000_000_000u64; // 0,02 XMR por lado
        let grupo = cuenta_m.keys().group_key().0;
        let fondeo = [salida_falsa(grupo, capital), salida_falsa(grupo, capital)];
        let anillos: Vec<_> = fondeo.iter().map(|f| f.anillo.clone()).collect();
        let (_, vista_c) = billetera();
        let (_, vista_m) = billetera();
        let dir_c = vista_c.legacy_address(Net::Stagenet.oxide());
        let dir_m = vista_m.legacy_address(Net::Stagenet.oxide());
        let inputs = fondeo.iter().map(|f| f.salida.clone()).collect();
        let (prop, split) =
            propose(&mut OsRng, "obra-dust", capital, pct, &dir_c, &dir_m, inputs, fee_rate()).unwrap();
        let (sm, pre_m) = SpendSession::open(&cuenta_m, &prop, &mut OsRng).unwrap();
        let (sc, pre_c) = SpendSession::open(&cuenta_c, &prop, &mut OsRng).unwrap();
        let (fm, share_m) = sm.sign(&pre_c).unwrap();
        let (fc, share_c) = sc.sign(&pre_m).unwrap();
        let tx = fm.complete(&share_c).unwrap();
        let tx_c = fc.complete(&share_m).unwrap();
        assert_eq!(tx.hash(), tx_c.hash(), "los dos llegan a la misma transacción");
        assert_eq!(verificar(&tx, &anillos), split.fee);
        (split, montos_para(&vista_c, &tx), montos_para(&vista_m, &tx))
    }

    #[test]
    fn pago_cien_por_ciento_deja_salida_de_cero_al_mandante() {
        let (split, al_c, al_m) = pagar(100);
        assert_eq!(split.mandante, 0);
        assert_eq!(al_c, vec![40_000_000_000 - split.fee]);
        // La salida existe (dos salidas) pero lleva 0 XMR: nada de 1 piconero.
        assert_eq!(al_m, vec![0]);
    }

    #[test]
    fn pago_parcial_reparte_sin_dust() {
        let (split, al_c, al_m) = pagar(80);
        assert_eq!(al_c, vec![20_000_000_000 + 16_000_000_000]);
        assert_eq!(al_m, vec![4_000_000_000 - split.fee]);
    }

    #[test]
    fn se_firma_una_propuesta_vieja_con_un_piconero() {
        // Un par con 0.2.7 arma el reparto con 1 piconero al mandante: 0.2.8 lo firma igual.
        let (cuenta_m, _) = cuentas();
        let capital = 20_000_000_000u64;
        let grupo = cuenta_m.keys().group_key().0;
        let inputs: Vec<_> = (0..2).map(|_| salida_falsa(grupo, capital).salida).collect();
        let (_, vc) = billetera();
        let (_, vm) = billetera();
        let dir_c = vc.legacy_address(Net::Stagenet.oxide());
        let dir_m = vm.legacy_address(Net::Stagenet.oxide());
        // Fee real con el reparto nuevo, después se arma a mano el viejo con ese fee.
        let (_, nuevo) =
            propose(&mut OsRng, "obra-dust", capital, 100, &dir_c, &dir_m, inputs.clone(), fee_rate()).unwrap();
        let viejo = split_pot_legacy(2 * capital, capital, 100, nuevo.fee).unwrap();
        let tx = SignableTransaction::new(
            RctType::ClsagBulletproofPlus,
            zeroize::Zeroizing::new([7u8; 32]),
            inputs,
            vec![(dir_c, viejo.contratista), (dir_m, viejo.mandante)],
            Change::fingerprintable(None),
            vec![],
            fee_rate(),
        )
        .unwrap();
        let prop = SpendProposal {
            obra_id: "obra-dust".into(),
            capital,
            pct: 100,
            contratista: dir_c.to_string(),
            mandante: dir_m.to_string(),
            signable: tx.serialize(),
        };
        assert!(SpendSession::open(&cuenta_m, &prop, &mut OsRng).is_ok());
    }
}
