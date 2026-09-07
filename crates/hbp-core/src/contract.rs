use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::Error;
use crate::Result;

pub const CONTRACT_ID_TAG: &[u8] = b"hbp-p2wsh-terms";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Regtest,
    Signet,
    Testnet,
    Bitcoin,
}

impl std::str::FromStr for Network {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "regtest" => Ok(Self::Regtest),
            "signet" => Ok(Self::Signet),
            "testnet" => Ok(Self::Testnet),
            "bitcoin" | "mainnet" => Ok(Self::Bitcoin),
            other => Err(Error::protocol(format!("unknown network {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Mandante,
    Contratista,
}

impl std::str::FromStr for Role {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "mandante" => Ok(Self::Mandante),
            "contratista" => Ok(Self::Contratista),
            other => Err(Error::protocol(format!("unknown role {other}"))),
        }
    }
}

/// Unhappy path. Chosen by the mandante in the offer; contractor accepts or walks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mode {
    /// 2-of-2 forever. No agreement → UTXO sits indefinitely.
    Hold,
    /// Pre-signed nLockTime burn: OP_RETURN + 100% fee after `t_unix`.
    Burn { t_unix: u32 },
}

impl Mode {
    pub fn t_unix(&self) -> Option<u32> {
        match self {
            Mode::Hold => None,
            Mode::Burn { t_unix } => Some(*t_unix),
        }
    }

    pub fn is_burn(&self) -> bool {
        matches!(self, Mode::Burn { .. })
    }
}

/// Shared terms. Identity of each party is their BIP48 cosigner xpub — no secret in hbp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terms {
    pub network: Network,
    pub mode: Mode,
    /// Each party locks this many sats. Escrow output = 2 × sats.
    pub sats: u64,
    /// Funding miner fee, split in half (each input pays `fee/2`).
    pub fee: u64,
    /// Address index on `m/48'/…/2'/0/i`.
    #[serde(default)]
    pub index: u32,
    /// BIP48 account xpub or descriptor key (`[fpr/…]tpub…/0/*` or Zpub/Vpub/tpub).
    pub mandante_xpub: String,
    pub contratista_xpub: Option<String>,
}

impl Terms {
    pub fn escrow_sats(&self) -> u64 {
        self.sats.saturating_mul(2)
    }

    pub fn require_complete(&self) -> Result<(&str, &str)> {
        let c = self
            .contratista_xpub
            .as_deref()
            .ok_or_else(|| Error::protocol("contratista xpub missing; accept the offer first"))?;
        if self.mandante_xpub.trim().is_empty() {
            return Err(Error::protocol("mandante xpub missing"));
        }
        if self.sats == 0 {
            return Err(Error::protocol("sats must be > 0"));
        }
        if self.fee == 0 {
            return Err(Error::protocol("fee must be > 0"));
        }
        if let Mode::Burn { t_unix } = self.mode {
            if t_unix < 500_000_000 {
                return Err(Error::protocol(
                    "burn t_unix must be a unix time (>= 500000000), not a block height",
                ));
            }
        }
        Ok((self.mandante_xpub.trim(), c.trim()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub terms: Terms,
}

pub fn canonical_json<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::CompactFormatter,
    );
    v.serialize(&mut ser)?;
    Ok(buf)
}

pub fn contract_id(terms: &Terms) -> Result<String> {
    let complete = terms.clone();
    complete.require_complete()?;
    let body = canonical_json(&complete)?;
    let mut h = Sha256::new();
    h.update(CONTRACT_ID_TAG);
    h.update(&body);
    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_stable_and_needs_both_xpubs() {
        let mut t = Terms {
            network: Network::Signet,
            mode: Mode::Hold,
            sats: 5_000,
            fee: 500,
            index: 0,
            mandante_xpub: "tpubM".into(),
            contratista_xpub: None,
        };
        assert!(contract_id(&t).is_err());
        t.contratista_xpub = Some("tpubC".into());
        let a = contract_id(&t).unwrap();
        let b = contract_id(&t).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        t.sats = 5_001;
        assert_ne!(a, contract_id(&t).unwrap());
    }
}
