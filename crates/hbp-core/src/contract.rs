use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::amount::Unit;
use crate::error::Error;
use crate::Result;

/// 32-byte contract id, hex-encoded in JSON.
pub type ContractId = String;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartidaSpec {
    pub id: u32,
    pub description: String,
    /// Amount in 1/100 of [`ContractBody::unit`].
    pub amount_minor: u64,
    /// Absolute CLTV as Bitcoin-style locktime (unix if >= 500_000_000).
    pub plazo_unix: u32,
}

/// Default 7 days between arbiter-window start (plazo) and last-resort unwind.
pub const DEFAULT_ARBITER_WINDOW_SECS: u32 = 7 * 24 * 60 * 60;

/// Offeror proposes this; accept locks it. Cannot change after funding (address depends on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum DisputePolicy {
    /// Timeout: each recovers their own funds. Default.
    Unwind,
    /// Same unwind, plus a small symmetric stake that becomes unspendable if nobody cooperates after T.
    Mad {
        /// Basis points of partida 1 sats, **each** party (100 = 1%).
        mad_bps: u16,
    },
    /// After plazo, arbiter+one party can spend; after plazo+window, last-resort unwind.
    Arbiter { pubkey: String, window_secs: u32 },
}

impl Default for DisputePolicy {
    fn default() -> Self {
        Self::Unwind
    }
}

impl DisputePolicy {
    pub fn validate(&self, mandante: &str) -> Result<()> {
        match self {
            Self::Unwind => Ok(()),
            Self::Mad { mad_bps } => {
                if *mad_bps == 0 || *mad_bps > 500 {
                    return Err(Error::protocol("mad_bps must be in 1..=500 (0.01%–5%)"));
                }
                Ok(())
            }
            Self::Arbiter {
                pubkey,
                window_secs,
            } => {
                decode_compressed_pubkey(pubkey)?;
                if pubkey == mandante {
                    return Err(Error::protocol("arbiter must not be the mandante"));
                }
                if *window_secs == 0 {
                    return Err(Error::protocol("arbiter window_secs must be > 0"));
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractBody {
    pub network: Network,
    pub unit: Unit,
    /// Basis points of the sum of partidas (1000 = 10%).
    pub bond_bps: u16,
    pub t_project: u32,
    pub partidas: Vec<PartidaSpec>,
    pub mandante_pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contratista_pubkey: Option<String>,
    /// Offeror-defined; default unwind. Both lock it at accept.
    #[serde(default)]
    pub dispute: DisputePolicy,
}

impl ContractBody {
    pub fn total_minor(&self) -> u64 {
        self.partidas.iter().map(|p| p.amount_minor).sum()
    }

    pub fn partida(&self, id: u32) -> Result<&PartidaSpec> {
        self.partidas
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::protocol(format!("unknown partida {id}")))
    }

    pub fn validate(&self) -> Result<()> {
        if self.partidas.is_empty() {
            return Err(Error::protocol("need at least one partida"));
        }
        if self.bond_bps == 0 || self.bond_bps > 10_000 {
            return Err(Error::protocol("bond_bps must be in 1..=10000"));
        }
        if self.t_project < 500_000_000 {
            return Err(Error::protocol(
                "t_project must be a unix locktime (>= 500000000)",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut last_plazo = 0u32;
        for p in &self.partidas {
            if p.description.trim().is_empty() {
                return Err(Error::protocol(format!(
                    "partida {} has empty description",
                    p.id
                )));
            }
            if p.amount_minor == 0 {
                return Err(Error::protocol(format!("partida {} has zero amount", p.id)));
            }
            if p.plazo_unix < 500_000_000 {
                return Err(Error::protocol(format!(
                    "partida {} plazo must be unix locktime",
                    p.id
                )));
            }
            if p.plazo_unix > self.t_project {
                return Err(Error::protocol(format!(
                    "partida {} plazo is after t_project",
                    p.id
                )));
            }
            if p.plazo_unix < last_plazo {
                return Err(Error::protocol(
                    "partidas must be in non-decreasing plazo order",
                ));
            }
            if !seen.insert(p.id) {
                return Err(Error::protocol(format!("duplicate partida id {}", p.id)));
            }
            last_plazo = p.plazo_unix;
        }
        decode_compressed_pubkey(&self.mandante_pubkey)?;
        if let Some(pk) = &self.contratista_pubkey {
            decode_compressed_pubkey(pk)?;
            if pk == &self.mandante_pubkey {
                return Err(Error::protocol("mandante and contratista keys must differ"));
            }
        }
        self.dispute.validate(&self.mandante_pubkey)?;
        if let DisputePolicy::Arbiter { pubkey, .. } = &self.dispute {
            if self.contratista_pubkey.as_ref() == Some(pubkey) {
                return Err(Error::protocol("arbiter must not be the contratista"));
            }
        }
        Ok(())
    }

    pub fn terms(&self) -> Terms {
        Terms {
            network: self.network,
            unit: self.unit,
            bond_bps: self.bond_bps,
            t_project: self.t_project,
            partidas: self.partidas.clone(),
            mandante_pubkey: self.mandante_pubkey.clone(),
            dispute: self.dispute.clone(),
        }
    }
}

/// Subset of the body that the mandante offered. The contratista may only add their key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Terms {
    pub network: Network,
    pub unit: Unit,
    pub bond_bps: u16,
    pub t_project: u32,
    pub partidas: Vec<PartidaSpec>,
    pub mandante_pubkey: String,
    pub dispute: DisputePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub body: ContractBody,
    /// BIP340 schnorr, 64-byte hex, over `hbp-contract` tagged hash of the body.
    pub mandante_sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedContract {
    pub body: ContractBody,
    pub mandante_sig: String,
    pub contratista_sig: String,
}

impl SignedContract {
    pub fn id(&self) -> Result<ContractId> {
        contract_id(&self.body)
    }

    pub fn require_both_keys(&self) -> Result<(&str, &str)> {
        let c = self
            .body
            .contratista_pubkey
            .as_deref()
            .ok_or_else(|| Error::protocol("contratista pubkey missing"))?;
        Ok((self.body.mandante_pubkey.as_str(), c))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub contract_id: ContractId,
    pub bond_sats: u64,
    pub partidas: Vec<PartidaQuote>,
    pub fx_note: String,
    pub quoted_at_unix: u32,
    pub mandante_sig: Option<String>,
    pub contratista_sig: Option<String>,
    /// Per-party MAD stake (sats). Output on-chain is `2 * mad_sats` if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mad_sats: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartidaQuote {
    pub id: u32,
    pub sats: u64,
}

impl Quote {
    pub fn partida_sats(&self, id: u32) -> Result<u64> {
        self.partidas
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.sats)
            .ok_or_else(|| Error::protocol(format!("quote has no partida {id}")))
    }

    pub fn validate_against(&self, body: &ContractBody) -> Result<()> {
        if self.bond_sats == 0 {
            return Err(Error::protocol("bond_sats must be > 0"));
        }
        if self.partidas.len() != body.partidas.len() {
            return Err(Error::protocol("quote must cover every partida"));
        }
        for spec in &body.partidas {
            let q = self.partida_sats(spec.id)?;
            if q < 546 {
                return Err(Error::protocol(format!(
                    "partida {} sats below dust",
                    spec.id
                )));
            }
        }
        match &body.dispute {
            DisputePolicy::Mad { mad_bps } => {
                let Some(each) = self.mad_sats else {
                    return Err(Error::protocol("mad policy requires quote.mad_sats"));
                };
                let p1 = self.partida_sats(body.partidas[0].id)?;
                let expect = p1
                    .checked_mul(u64::from(*mad_bps))
                    .and_then(|v| v.checked_div(10_000))
                    .ok_or_else(|| Error::protocol("mad_sats overflow"))?;
                if each != expect || each < 546 {
                    return Err(Error::protocol(format!(
                        "mad_sats {each} != {expect} (partida1 * mad_bps / 10000)"
                    )));
                }
            }
            _ => {
                if self.mad_sats.is_some() {
                    return Err(Error::protocol("mad_sats set but dispute is not mad"));
                }
            }
        }
        Ok(())
    }
}

pub fn decode_compressed_pubkey(hex_str: &str) -> Result<[u8; 33]> {
    let bytes = hex::decode(hex_str.trim())?;
    let arr: [u8; 33] = bytes
        .try_into()
        .map_err(|_| Error::protocol("pubkey must be 33-byte compressed hex"))?;
    if arr[0] != 0x02 && arr[0] != 0x03 {
        return Err(Error::protocol("pubkey must be compressed"));
    }
    Ok(arr)
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // Struct field order is deterministic. We still go through serde_json::Value
    // so maps (if any) sort by key.
    let value = serde_json::to_value(value)?;
    Ok(to_canonical(&value)?)
}

fn to_canonical(value: &serde_json::Value) -> Result<Vec<u8>> {
    match value {
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => Ok(serde_json::to_vec(value)?),
        serde_json::Value::Array(items) => {
            let mut out = Vec::from(b"[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend(to_canonical(item)?);
            }
            out.push(b']');
            Ok(out)
        }
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut out = Vec::from(b"{");
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key)?);
                out.push(b':');
                out.extend(to_canonical(&map[*key])?);
            }
            out.push(b'}');
            Ok(out)
        }
    }
}

pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn contract_id(body: &ContractBody) -> Result<ContractId> {
    body.validate()?;
    if body.contratista_pubkey.is_none() {
        return Err(Error::protocol(
            "contract id requires both pubkeys (accepted contract)",
        ));
    }
    Ok(hex::encode(sha256_bytes(&canonical_json(body)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pk(n: u8) -> String {
        let mut b = [0x02u8; 33];
        b[32] = n;
        hex::encode(b)
    }

    #[test]
    fn canonical_is_stable() {
        let body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![PartidaSpec {
                id: 1,
                description: "Radier".into(),
                amount_minor: 150_000,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: pk(1),
            contratista_pubkey: Some(pk(2)),
            dispute: DisputePolicy::Unwind,
        };
        let a = canonical_json(&body).unwrap();
        let b = canonical_json(&body).unwrap();
        assert_eq!(a, b);
        assert_eq!(contract_id(&body).unwrap().len(), 64);
    }
}
