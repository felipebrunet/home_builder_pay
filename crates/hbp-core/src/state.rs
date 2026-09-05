use serde::{Deserialize, Serialize};

use crate::contract::{ArbiterNomination, Quote, SignedContract};
use crate::error::Error;
use crate::Result;

pub const DEFAULT_CONFIRMATIONS: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Offered,
    Accepted,
    Active,
    Closed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BondStatus {
    Unfunded,
    Funded {
        txid: String,
        vout: u32,
        sats: u64,
        confirmations: u32,
    },
    Released {
        txid: String,
    },
    Unwound {
        txid: String,
    },
    /// First fee-burn fired: 50% consumed as miner fees; half remains.
    FeeBurnT1 {
        txid: String,
        continuation_vout: u32,
        remaining_sats: u64,
    },
    /// Second fee-burn fired: remaining half consumed as miner fees.
    FeeBurnT2 {
        txid: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PartidaStatus {
    Scheduled,
    AmountAgreed {
        sats: u64,
    },
    Funding {
        sats: u64,
        txid: String,
        vout: u32,
        confirmations: u32,
    },
    Locked {
        sats: u64,
        txid: String,
        vout: u32,
        confirmations: u32,
    },
    ReceptionProposed {
        sats: u64,
        txid: String,
        vout: u32,
    },
    Paid {
        payout_txid: String,
        sats: u64,
    },
    Unwound {
        txid: String,
        sats: u64,
    },
    FeeBurnT1 {
        txid: String,
        continuation_vout: u32,
        remaining_sats: u64,
    },
    FeeBurnT2 {
        txid: String,
        sats: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartidaRuntime {
    pub id: u32,
    pub state: PartidaStatus,
}

impl PartidaRuntime {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            PartidaStatus::Paid { .. }
                | PartidaStatus::Unwound { .. }
                | PartidaStatus::FeeBurnT2 { .. }
        )
    }

    pub fn locked_utxo(&self) -> Option<(&str, u32, u64)> {
        match &self.state {
            PartidaStatus::Locked {
                txid, vout, sats, ..
            }
            | PartidaStatus::ReceptionProposed { txid, vout, sats } => Some((txid, *vout, *sats)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub contract: SignedContract,
    pub quote: Option<Quote>,
    #[serde(default)]
    pub arbiter: Option<ArbiterNomination>,
    pub status: ProjectStatus,
    pub bond: BondStatus,
    pub partidas: Vec<PartidaRuntime>,
}

impl Project {
    pub fn from_signed(contract: SignedContract) -> Result<Self> {
        contract.body.validate()?;
        contract.require_both_keys()?;
        let partidas = contract
            .body
            .partidas
            .iter()
            .map(|p| PartidaRuntime {
                id: p.id,
                state: PartidaStatus::Scheduled,
            })
            .collect();
        Ok(Self {
            contract,
            quote: None,
            arbiter: None,
            status: ProjectStatus::Accepted,
            bond: BondStatus::Unfunded,
            partidas,
        })
    }

    pub fn partida_mut(&mut self, id: u32) -> Result<&mut PartidaRuntime> {
        self.partidas
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::protocol(format!("unknown partida {id}")))
    }

    pub fn partida(&self, id: u32) -> Result<&PartidaRuntime> {
        self.partidas
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::protocol(format!("unknown partida {id}")))
    }

    /// Only one partida may be in flight. Previous must be paid or unwound.
    pub fn active_partida_id(&self) -> Option<u32> {
        self.partidas
            .iter()
            .find(|p| !p.is_terminal())
            .map(|p| p.id)
    }

    pub fn set_quote(&mut self, quote: Quote) -> Result<()> {
        quote.validate_against(&self.contract.body)?;
        if quote.contract_id != self.contract.id()? {
            return Err(Error::protocol("quote contract_id mismatch"));
        }
        if quote.mandante_sig.is_none() || quote.contratista_sig.is_none() {
            return Err(Error::protocol("quote needs both signatures"));
        }
        for p in &mut self.partidas {
            if matches!(p.state, PartidaStatus::Scheduled) {
                let sats = quote.partida_sats(p.id)?;
                p.state = PartidaStatus::AmountAgreed { sats };
            }
        }
        self.quote = Some(quote);
        Ok(())
    }

    /// Drop a locked quote before any UTXO is funded so both sides can recotizar.
    pub fn clear_quote(&mut self) -> Result<()> {
        if self.funding_started() {
            return Err(Error::protocol(
                "too late to recotizar: el fondeo ya empezó",
            ));
        }
        self.quote = None;
        for p in &mut self.partidas {
            if matches!(p.state, PartidaStatus::AmountAgreed { .. }) {
                p.state = PartidaStatus::Scheduled;
            }
        }
        Ok(())
    }

    pub fn funding_started(&self) -> bool {
        !matches!(self.bond, BondStatus::Unfunded)
            || self.partidas.iter().any(|p| {
                !matches!(
                    p.state,
                    PartidaStatus::Scheduled | PartidaStatus::AmountAgreed { .. }
                )
            })
    }

    pub fn set_arbiter(&mut self, nom: ArbiterNomination) -> Result<()> {
        if nom.contract_id != self.contract.id()? {
            return Err(Error::protocol("arbiter nomination contract_id mismatch"));
        }
        nom.validate_against(&self.contract.body)?;
        if !nom.fully_signed() {
            return Err(Error::protocol("arbiter nomination needs both signatures"));
        }
        if let Some(existing) = self.named_arbiter_pubkey()? {
            if existing != nom.pubkey {
                return Err(Error::protocol(
                    "arbiter already locked; cannot change after both signed",
                ));
            }
            return Ok(());
        }
        if self.funding_started() {
            return Err(Error::protocol(
                "too late to name an arbiter: UTXOs already funded (address would change)",
            ));
        }
        self.arbiter = Some(nom);
        Ok(())
    }

    pub fn named_arbiter_pubkey(&self) -> Result<Option<&str>> {
        match &self.contract.body.dispute {
            crate::DisputePolicy::Arbiter { .. } => Ok(self
                .arbiter
                .as_ref()
                .filter(|a| a.fully_signed())
                .map(|a| a.pubkey.as_str())),
            _ => Ok(None),
        }
    }

    pub fn note_bond_funding(
        &mut self,
        txid: String,
        vout: u32,
        sats: u64,
        confs: u32,
    ) -> Result<()> {
        if !matches!(self.bond, BondStatus::Unfunded) {
            return Err(Error::protocol("bond already funded"));
        }
        let expected = self
            .quote
            .as_ref()
            .ok_or_else(|| Error::protocol("quote required before funding"))?
            .bond_sats;
        if sats != expected {
            return Err(Error::protocol(format!(
                "bond sats {sats} != quoted {expected}"
            )));
        }
        self.bond = BondStatus::Funded {
            txid,
            vout,
            sats,
            confirmations: confs,
        };
        self.maybe_activate();
        Ok(())
    }

    pub fn note_partida_funding(
        &mut self,
        id: u32,
        txid: String,
        vout: u32,
        sats: u64,
        confs: u32,
        required_confs: u32,
    ) -> Result<()> {
        if matches!(self.bond, BondStatus::Unfunded) {
            return Err(Error::protocol(
                "bond must be funded before any partida (contractor did not post the boleta)",
            ));
        }
        self.require_previous_terminal(id)?;
        let p = self.partida_mut(id)?;
        let expected = match p.state {
            PartidaStatus::AmountAgreed { sats } => sats,
            PartidaStatus::Funding { sats, .. } | PartidaStatus::Locked { sats, .. } => sats,
            _ => return Err(Error::protocol("partida not in a fundable state")),
        };
        if sats != expected {
            return Err(Error::protocol(format!(
                "partida {id} sats {sats} != quoted {expected}"
            )));
        }
        p.state = if confs >= required_confs {
            PartidaStatus::Locked {
                txid,
                vout,
                sats,
                confirmations: confs,
            }
        } else {
            PartidaStatus::Funding {
                txid,
                vout,
                sats,
                confirmations: confs,
            }
        };
        self.maybe_activate();
        Ok(())
    }

    pub fn propose_reception(&mut self, id: u32) -> Result<()> {
        let p = self.partida_mut(id)?;
        match &p.state {
            PartidaStatus::Locked {
                sats, txid, vout, ..
            } => {
                p.state = PartidaStatus::ReceptionProposed {
                    sats: *sats,
                    txid: txid.clone(),
                    vout: *vout,
                };
                Ok(())
            }
            _ => Err(Error::protocol("reception requires a locked partida")),
        }
    }

    pub fn reject_reception(&mut self, id: u32) -> Result<()> {
        let p = self.partida_mut(id)?;
        match &p.state {
            PartidaStatus::ReceptionProposed { sats, txid, vout } => {
                p.state = PartidaStatus::Locked {
                    sats: *sats,
                    txid: txid.clone(),
                    vout: *vout,
                    confirmations: DEFAULT_CONFIRMATIONS,
                };
                Ok(())
            }
            _ => Err(Error::protocol("no reception to reject")),
        }
    }

    pub fn mark_paid(&mut self, id: u32, payout_txid: String) -> Result<()> {
        let p = self.partida_mut(id)?;
        let sats = match &p.state {
            PartidaStatus::Locked { sats, .. } | PartidaStatus::ReceptionProposed { sats, .. } => {
                *sats
            }
            _ => return Err(Error::protocol("cannot pay a partida that is not locked")),
        };
        p.state = PartidaStatus::Paid { payout_txid, sats };
        Ok(())
    }

    pub fn mark_partida_unwound(&mut self, id: u32, txid: String) -> Result<()> {
        let p = self.partida_mut(id)?;
        let sats = match &p.state {
            PartidaStatus::Locked { sats, .. }
            | PartidaStatus::Funding { sats, .. }
            | PartidaStatus::ReceptionProposed { sats, .. }
            | PartidaStatus::AmountAgreed { sats } => *sats,
            _ => return Err(Error::protocol("partida cannot unwind from this state")),
        };
        p.state = PartidaStatus::Unwound { txid, sats };
        Ok(())
    }

    pub fn bond_utxo(&self) -> Option<(&str, u32, u64)> {
        match &self.bond {
            BondStatus::Funded {
                txid, vout, sats, ..
            } => Some((txid, *vout, *sats)),
            _ => None,
        }
    }

    pub fn bond_is_funded(&self) -> bool {
        matches!(self.bond, BondStatus::Funded { .. })
    }

    pub fn is_stopped(&self) -> bool {
        matches!(
            self.status,
            ProjectStatus::Cancelled | ProjectStatus::Closed
        ) && !matches!(self.bond, BondStatus::Funded { .. })
    }

    pub fn has_open_onchain_partida(&self) -> bool {
        self.partidas.iter().any(|p| {
            matches!(
                p.state,
                PartidaStatus::Funding { .. }
                    | PartidaStatus::Locked { .. }
                    | PartidaStatus::ReceptionProposed { .. }
            )
        })
    }

    pub fn mark_bond_released(&mut self, txid: String) -> Result<()> {
        if !matches!(self.bond, BondStatus::Funded { .. }) {
            return Err(Error::protocol("bond is not funded"));
        }
        if self.has_open_onchain_partida() {
            return Err(Error::protocol(
                "cannot release bond while a partida is locked on-chain",
            ));
        }
        self.bond = BondStatus::Released { txid };
        self.status = if self
            .partidas
            .iter()
            .any(|p| matches!(p.state, PartidaStatus::Paid { .. }))
        {
            ProjectStatus::Closed
        } else {
            ProjectStatus::Cancelled
        };
        Ok(())
    }

    pub fn mark_bond_unwound(&mut self, txid: String) -> Result<()> {
        if !matches!(self.bond, BondStatus::Funded { .. }) {
            return Err(Error::protocol("bond is not funded"));
        }
        self.bond = BondStatus::Unwound { txid };
        self.status = ProjectStatus::Cancelled;
        Ok(())
    }

    pub fn mark_partida_fee_burn_t1(
        &mut self,
        id: u32,
        txid: String,
        continuation_vout: u32,
        remaining_sats: u64,
    ) -> Result<()> {
        let p = self.partida_mut(id)?;
        match &p.state {
            PartidaStatus::Locked { .. }
            | PartidaStatus::Funding { .. }
            | PartidaStatus::ReceptionProposed { .. } => {
                p.state = PartidaStatus::FeeBurnT1 {
                    txid,
                    continuation_vout,
                    remaining_sats,
                };
                Ok(())
            }
            _ => Err(Error::protocol(
                "partida fee-burn t1 requires a locked/funding UTXO",
            )),
        }
    }

    pub fn mark_partida_fee_burn_t2(&mut self, id: u32, txid: String) -> Result<()> {
        let p = self.partida_mut(id)?;
        let sats = match &p.state {
            PartidaStatus::FeeBurnT1 { remaining_sats, .. } => *remaining_sats,
            _ => {
                return Err(Error::protocol(
                    "partida fee-burn t2 requires a t1 continuation",
                ))
            }
        };
        p.state = PartidaStatus::FeeBurnT2 { txid, sats };
        Ok(())
    }

    pub fn mark_bond_fee_burn_t1(
        &mut self,
        txid: String,
        continuation_vout: u32,
        remaining_sats: u64,
    ) -> Result<()> {
        if !matches!(self.bond, BondStatus::Funded { .. }) {
            return Err(Error::protocol("bond is not funded"));
        }
        self.bond = BondStatus::FeeBurnT1 {
            txid,
            continuation_vout,
            remaining_sats,
        };
        Ok(())
    }

    pub fn mark_bond_fee_burn_t2(&mut self, txid: String) -> Result<()> {
        if !matches!(self.bond, BondStatus::FeeBurnT1 { .. }) {
            return Err(Error::protocol(
                "bond fee-burn t2 requires a t1 continuation",
            ));
        }
        self.bond = BondStatus::FeeBurnT2 { txid };
        self.status = ProjectStatus::Cancelled;
        Ok(())
    }

    fn require_previous_terminal(&self, id: u32) -> Result<()> {
        for p in &self.partidas {
            if p.id == id {
                return Ok(());
            }
            if !p.is_terminal() {
                return Err(Error::protocol(format!(
                    "partida {} must be paid, unwound, or fully fee-burnt before funding {id}",
                    p.id
                )));
            }
        }
        Ok(())
    }

    fn maybe_activate(&mut self) {
        let bond_ok = matches!(self.bond, BondStatus::Funded { .. });
        let any_locked = self.partidas.iter().any(|p| {
            matches!(
                p.state,
                PartidaStatus::Locked { .. } | PartidaStatus::Funding { .. }
            )
        });
        if bond_ok && any_locked && self.status == ProjectStatus::Accepted {
            self.status = ProjectStatus::Active;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amount::Unit;
    use crate::contract::{ContractBody, Network, PartidaQuote, PartidaSpec, Quote};

    fn pk(n: u8) -> String {
        let mut b = [0x02u8; 33];
        b[32] = n;
        hex::encode(b)
    }

    fn project() -> Project {
        let body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            work_name: String::new(),
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![
                PartidaSpec {
                    id: 1,
                    description: "A".into(),
                    amount_minor: 100_000,
                    plazo_unix: 1_700_000_000,
                },
                PartidaSpec {
                    id: 2,
                    description: "B".into(),
                    amount_minor: 100_000,
                    plazo_unix: 1_710_000_000,
                },
            ],
            mandante_pubkey: pk(1),
            contratista_pubkey: Some(pk(2)),
            dispute: crate::DisputePolicy::Unwind,
        };
        let contract = SignedContract {
            body,
            mandante_sig: "aa".into(),
            contratista_sig: "bb".into(),
        };
        Project::from_signed(contract).unwrap()
    }

    fn signed_quote(p: &Project) -> Quote {
        Quote {
            contract_id: p.contract.id().unwrap(),
            bond_sats: 50_000,
            partidas: vec![
                PartidaQuote {
                    id: 1,
                    sats: 20_000,
                },
                PartidaQuote {
                    id: 2,
                    sats: 20_000,
                },
            ],
            fx_note: "test".into(),
            quoted_at_unix: 1_700_000_000,
            mandante_sig: Some("aa".into()),
            contratista_sig: Some("bb".into()),
            mad_sats: None,
        }
    }

    #[test]
    fn cannot_fund_partida_without_bond() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        let err = p
            .note_partida_funding(1, "x".into(), 0, 20_000, 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains("bond must be funded"));
    }

    #[test]
    fn cannot_fund_segunda_before_primera() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("t".into(), 0, 50_000, 1).unwrap();
        let err = p
            .note_partida_funding(2, "x".into(), 1, 20_000, 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains("partida 1 must be paid"));
    }

    #[test]
    fn clear_quote_before_funding_then_relock() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        assert!(p.quote.is_some());
        p.clear_quote().unwrap();
        assert!(p.quote.is_none());
        assert!(matches!(
            p.partida(1).unwrap().state,
            PartidaStatus::Scheduled
        ));
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("bond".into(), 0, 50_000, 1).unwrap();
        assert!(p.clear_quote().is_err());
    }

    #[test]
    fn happy_path_two_partidas() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("bond".into(), 0, 50_000, 1).unwrap();
        p.note_partida_funding(1, "p1".into(), 1, 20_000, 1, 1)
            .unwrap();
        assert_eq!(p.status, ProjectStatus::Active);
        p.propose_reception(1).unwrap();
        p.mark_paid(1, "pay1".into()).unwrap();
        p.note_partida_funding(2, "p2".into(), 1, 20_000, 1, 1)
            .unwrap();
        p.mark_paid(2, "pay2".into()).unwrap();
        p.mark_bond_released("bondout".into()).unwrap();
        assert_eq!(p.status, ProjectStatus::Closed);
    }

    #[test]
    fn can_release_bond_if_later_partidas_never_funded() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("bond".into(), 0, 50_000, 1).unwrap();
        p.note_partida_funding(1, "p1".into(), 1, 20_000, 1, 1)
            .unwrap();
        p.mark_paid(1, "pay1".into()).unwrap();
        p.mark_bond_released("bondout".into()).unwrap();
        assert_eq!(p.status, ProjectStatus::Closed);
        assert!(matches!(
            p.partida(2).unwrap().state,
            PartidaStatus::AmountAgreed { .. }
        ));
    }

    #[test]
    fn arbiter_named_only_after_both_sign_and_before_funding() {
        let mut p = project();
        p.contract.body.dispute = crate::DisputePolicy::Arbiter { window_secs: 15 };
        assert!(p.named_arbiter_pubkey().unwrap().is_none());
        let cid = p.contract.id().unwrap();
        let unsigned = crate::ArbiterNomination {
            contract_id: cid.clone(),
            pubkey: pk(9),
            mandante_sig: Some("aa".into()),
            contratista_sig: None,
        };
        assert!(p
            .set_arbiter(unsigned)
            .unwrap_err()
            .to_string()
            .contains("both signatures"));
        let signed = crate::ArbiterNomination {
            contract_id: cid.clone(),
            pubkey: pk(9),
            mandante_sig: Some("aa".into()),
            contratista_sig: Some("bb".into()),
        };
        p.set_arbiter(signed.clone()).unwrap();
        assert_eq!(p.named_arbiter_pubkey().unwrap(), Some(pk(9).as_str()));
        p.set_arbiter(signed).unwrap();
        let other = crate::ArbiterNomination {
            contract_id: cid,
            pubkey: pk(8),
            mandante_sig: Some("aa".into()),
            contratista_sig: Some("bb".into()),
        };
        assert!(p
            .set_arbiter(other)
            .unwrap_err()
            .to_string()
            .contains("already locked"));
    }

    #[test]
    fn cannot_name_arbiter_after_funding() {
        let mut p = project();
        p.contract.body.dispute = crate::DisputePolicy::Arbiter { window_secs: 15 };
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("bond".into(), 0, 50_000, 1).unwrap();
        let nom = crate::ArbiterNomination {
            contract_id: p.contract.id().unwrap(),
            pubkey: pk(9),
            mandante_sig: Some("aa".into()),
            contratista_sig: Some("bb".into()),
        };
        assert!(p
            .set_arbiter(nom)
            .unwrap_err()
            .to_string()
            .contains("too late"));
    }

    #[test]
    fn rejects_wrong_bond_amount() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        let err = p.note_bond_funding("t".into(), 0, 1, 1).unwrap_err();
        assert!(err.to_string().contains("bond sats"));
    }

    #[test]
    fn fee_burn_t1_then_t2_terminates_partida() {
        let mut p = project();
        p.contract.body.dispute = crate::DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000);
        p.set_quote(signed_quote(&p)).unwrap();
        p.note_bond_funding("bond".into(), 0, 50_000, 1).unwrap();
        p.note_partida_funding(1, "p1".into(), 1, 20_000, 1, 1)
            .unwrap();
        p.mark_partida_fee_burn_t1(1, "burn1".into(), 0, 10_000)
            .unwrap();
        assert!(!p.partida(1).unwrap().is_terminal());
        let err = p
            .note_partida_funding(2, "p2".into(), 1, 20_000, 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains("fee-burnt"));
        p.mark_partida_fee_burn_t2(1, "burn2".into()).unwrap();
        assert!(p.partida(1).unwrap().is_terminal());
        p.note_partida_funding(2, "p2".into(), 1, 20_000, 1, 1)
            .unwrap();
        p.mark_bond_fee_burn_t1("bb1".into(), 0, 25_000).unwrap();
        p.mark_bond_fee_burn_t2("bb2".into()).unwrap();
        assert_eq!(p.status, ProjectStatus::Cancelled);
    }
}
