use serde::{Deserialize, Serialize};

use crate::contract::{contract_id, Mode, Offer, Terms};
use crate::error::Error;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProjectStatus {
    Offered,
    Accepted,
    Funded {
        txid: String,
        vout: u32,
        sats: u64,
    },
    Closed {
        txid: String,
    },
    Burned {
        txid: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub terms: Terms,
    pub status: ProjectStatus,
    /// Fully signed burn PSBT (base64), present only in burn mode after round A.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub burn_psbt: Option<String>,
}

impl Project {
    pub fn from_offer(offer: Offer) -> Self {
        Self {
            terms: offer.terms,
            status: ProjectStatus::Offered,
            burn_psbt: None,
        }
    }

    pub fn id(&self) -> Result<String> {
        contract_id(&self.terms)
    }

    pub fn accept(&mut self, contratista_xpub: String) -> Result<()> {
        if !matches!(self.status, ProjectStatus::Offered) {
            return Err(Error::protocol("can only accept an offer"));
        }
        if self.terms.contratista_xpub.is_some() {
            return Err(Error::protocol("offer already has a contratista xpub"));
        }
        if contratista_xpub.trim().is_empty() {
            return Err(Error::protocol("contratista xpub empty"));
        }
        self.terms.contratista_xpub = Some(contratista_xpub.trim().to_string());
        self.terms.require_complete()?;
        self.status = ProjectStatus::Accepted;
        Ok(())
    }

    pub fn import_accepted(&mut self, other: Terms) -> Result<()> {
        if other.contratista_xpub.is_none() {
            return Err(Error::protocol("accepted terms missing contratista xpub"));
        }
        other.require_complete()?;
        if self.terms.mandante_xpub != other.mandante_xpub
            || self.terms.sats != other.sats
            || self.terms.fee != other.fee
            || self.terms.mode != other.mode
            || self.terms.network != other.network
            || self.terms.index != other.index
        {
            return Err(Error::protocol(
                "accepted terms do not match the local offer",
            ));
        }
        self.terms = other;
        self.status = ProjectStatus::Accepted;
        Ok(())
    }

    pub fn require_accepted(&self) -> Result<()> {
        match self.status {
            ProjectStatus::Accepted | ProjectStatus::Funded { .. } => Ok(()),
            _ => Err(Error::protocol("contract is not accepted/funded")),
        }
    }

    pub fn set_burn_psbt(&mut self, b64: String) -> Result<()> {
        if !self.terms.mode.is_burn() {
            return Err(Error::protocol("hold mode has no burn PSBT"));
        }
        self.require_accepted()?;
        self.burn_psbt = Some(b64);
        Ok(())
    }

    pub fn mark_funded(&mut self, txid: String, vout: u32, sats: u64) -> Result<()> {
        match &self.status {
            ProjectStatus::Accepted | ProjectStatus::Funded { .. } => {}
            ProjectStatus::Closed { .. } | ProjectStatus::Burned { .. } => {
                return Err(Error::protocol("already closed"));
            }
            ProjectStatus::Offered => {
                return Err(Error::protocol("cannot fund before accept"));
            }
        }
        if self.terms.mode.is_burn() && self.burn_psbt.is_none() {
            return Err(Error::protocol(
                "burn mode: both parties must sign the burn PSBT before funding",
            ));
        }
        self.status = ProjectStatus::Funded { txid, vout, sats };
        Ok(())
    }

    pub fn mark_closed(&mut self, txid: String) -> Result<()> {
        match &self.status {
            ProjectStatus::Closed { txid: old } if old == &txid => Ok(()),
            ProjectStatus::Closed { .. } | ProjectStatus::Burned { .. } => Ok(()),
            ProjectStatus::Funded { .. } => {
                self.status = ProjectStatus::Closed { txid };
                Ok(())
            }
            _ => Err(Error::protocol("cannot close: not funded")),
        }
    }

    pub fn mark_burned(&mut self, txid: String) -> Result<()> {
        if !matches!(self.terms.mode, Mode::Burn { .. }) {
            return Err(Error::protocol("hold mode cannot burn"));
        }
        match &self.status {
            ProjectStatus::Burned { txid: old } if old == &txid => Ok(()),
            ProjectStatus::Burned { .. } | ProjectStatus::Closed { .. } => Ok(()),
            ProjectStatus::Funded { .. } => {
                self.status = ProjectStatus::Burned { txid };
                Ok(())
            }
            _ => Err(Error::protocol("cannot burn: not funded")),
        }
    }

    pub fn funded_utxo(&self) -> Option<(&str, u32, u64)> {
        match &self.status {
            ProjectStatus::Funded { txid, vout, sats } => Some((txid, *vout, *sats)),
            _ => None,
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            ProjectStatus::Closed { .. } | ProjectStatus::Burned { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{Mode, Network};

    fn terms() -> Terms {
        Terms {
            network: Network::Signet,
            mode: Mode::Burn { t_unix: 1_800_000_000 },
            sats: 5_000,
            fee: 500,
            index: 0,
            mandante_xpub: "tpubM".into(),
            contratista_xpub: None,
        }
    }

    #[test]
    fn burn_requires_psbt_before_funding() {
        let mut p = Project::from_offer(Offer { terms: terms() });
        p.accept("tpubC".into()).unwrap();
        assert!(p.mark_funded("aa".into(), 0, 10_000).is_err());
        p.set_burn_psbt("cHNidP8=".into()).unwrap();
        p.mark_funded("aa".into(), 0, 10_000).unwrap();
        p.mark_burned("bb".into()).unwrap();
        p.mark_burned("bb".into()).unwrap();
        assert!(p.is_terminal());
    }

    #[test]
    fn hold_funds_without_burn() {
        let mut t = terms();
        t.mode = Mode::Hold;
        let mut p = Project::from_offer(Offer { terms: t });
        p.accept("tpubC".into()).unwrap();
        p.mark_funded("aa".into(), 0, 10_000).unwrap();
        assert!(p.set_burn_psbt("x".into()).is_err());
        p.mark_closed("cc".into()).unwrap();
    }
}
