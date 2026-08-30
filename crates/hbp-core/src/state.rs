use serde::{Deserialize, Serialize};

use crate::contract::{Quote, SignedContract};
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
            PartidaStatus::Paid { .. } | PartidaStatus::Unwound { .. }
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
            | PartidaStatus::ReceptionProposed { sats, .. } => *sats,
            _ => return Err(Error::protocol("partida cannot unwind from this state")),
        };
        p.state = PartidaStatus::Unwound { txid, sats };
        Ok(())
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

    fn require_previous_terminal(&self, id: u32) -> Result<()> {
        for p in &self.partidas {
            if p.id == id {
                return Ok(());
            }
            if !p.is_terminal() {
                return Err(Error::protocol(format!(
                    "partida {} must be paid or unwound before funding {id}",
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
            arbiter_pubkey: None,
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
        }
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
    fn rejects_wrong_bond_amount() {
        let mut p = project();
        p.set_quote(signed_quote(&p)).unwrap();
        let err = p.note_bond_funding("t".into(), 0, 1, 1).unwrap_err();
        assert!(err.to_string().contains("bond sats"));
    }
}
