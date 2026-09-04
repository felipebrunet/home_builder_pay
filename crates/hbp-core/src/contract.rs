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

/// Product GUI and product-path contracts: Signet only. No mainnet.
/// Regtest stays for CLI / catalog / unit tests.
pub const PRODUCT_NETWORK: Network = Network::Signet;

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

/// Product v1: arbiter nomination and UI are hard-off. Legacy `Arbiter` trees
/// remain parseable so the mined catalog still compiles; the native app never
/// constructs this policy.
pub const ARBITER_ENABLED: bool = false;

/// Offeror proposes this; accept locks it. Cannot change after funding (address depends on it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum DisputePolicy {
    /// Dual-deadline miner-fee burn. **Product default** no-agreement path.
    /// At `t1`, 50% of the locked UTXO is consumed as miner fees and the
    /// remaining half continues; at `t2` the rest is likewise consumed as fees.
    /// Cooperative MuSig2 key-path remains when both agree.
    FeeBurn {
        /// First burn deadline (unix CLTV). 50% of bond + active partida → fees.
        t1: u32,
        /// Second burn deadline (unix CLTV, must be > t1). Remaining 50% → fees.
        t2: u32,
    },
    /// Legacy: timeout, each recovers their own funds. Kept for the mined catalog.
    Unwind,
    /// Legacy: same unwind, plus a small symmetric NUMS stake.
    Mad {
        /// Basis points of partida 1 sats, **each** party (100 = 1%).
        mad_bps: u16,
    },
    /// Legacy slot. Disabled in the product UI (`ARBITER_ENABLED = false`).
    Arbiter { window_secs: u32 },
}

impl Default for DisputePolicy {
    fn default() -> Self {
        // Serde default for offers that omit `dispute` (pre-fee-burn JSON).
        // New product offers always emit `fee_burn`.
        Self::Unwind
    }
}

impl DisputePolicy {
    pub fn fee_burn(t1: u32, t2: u32) -> Self {
        Self::FeeBurn { t1, t2 }
    }

    pub fn is_fee_burn(&self) -> bool {
        matches!(self, Self::FeeBurn { .. })
    }

    pub fn fee_burn_deadlines(&self) -> Option<(u32, u32)> {
        match self {
            Self::FeeBurn { t1, t2 } => Some((*t1, *t2)),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        match self {
            Self::FeeBurn { t1, t2 } => validate_fee_burn_deadlines(*t1, *t2),
            Self::Unwind => Ok(()),
            Self::Mad { mad_bps } => {
                if *mad_bps == 0 || *mad_bps > 500 {
                    return Err(Error::protocol("mad_bps must be in 1..=500 (0.01%–5%)"));
                }
                Ok(())
            }
            Self::Arbiter { window_secs } => {
                if *window_secs == 0 {
                    return Err(Error::protocol("arbiter window_secs must be > 0"));
                }
                Ok(())
            }
        }
    }
}

/// Unix locktimes, `t2 > t1`. Same rule used by the GUI and the burn txs.
pub fn validate_fee_burn_deadlines(t1: u32, t2: u32) -> Result<()> {
    if t1 < 500_000_000 {
        return Err(Error::protocol(
            "fee-burn t1 must be a unix locktime (>= 500000000)",
        ));
    }
    if t2 < 500_000_000 {
        return Err(Error::protocol(
            "fee-burn t2 must be a unix locktime (>= 500000000)",
        ));
    }
    if t2 <= t1 {
        return Err(Error::protocol("fee-burn t2 must be strictly after t1"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractBody {
    pub network: Network,
    pub unit: Unit,
    /// Human name of this work (one secp identity per named work in the GUI).
    /// Omitted from old JSON; empty string does not change `contract_id`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub work_name: String,
    /// Basis points of the sum of partidas (1000 = 10%). Product default is 10%.
    pub bond_bps: u16,
    pub t_project: u32,
    pub partidas: Vec<PartidaSpec>,
    pub mandante_pubkey: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contratista_pubkey: Option<String>,
    /// Offeror-defined. Product default is [`DisputePolicy::FeeBurn`]; omitted
    /// field still deserializes as legacy [`DisputePolicy::Unwind`].
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
        if let DisputePolicy::FeeBurn { t2, .. } = self.dispute {
            if self.t_project < t2 {
                return Err(Error::protocol(
                    "t_project must be >= fee-burn t2 (use t_project = t2)",
                ));
            }
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
        self.dispute.validate()?;
        Ok(())
    }

    pub fn terms(&self) -> Terms {
        Terms {
            network: self.network,
            unit: self.unit,
            work_name: self.work_name.clone(),
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
    pub work_name: String,
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

/// Joint naming of an arbiter. Not part of the offer hash; both must sign
/// the same pubkey before addresses/funding. Either party can propose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArbiterNomination {
    pub contract_id: ContractId,
    pub pubkey: String,
    pub mandante_sig: Option<String>,
    pub contratista_sig: Option<String>,
}

impl ArbiterNomination {
    pub fn fully_signed(&self) -> bool {
        self.mandante_sig.is_some() && self.contratista_sig.is_some()
    }

    pub fn validate_against(&self, body: &ContractBody) -> Result<()> {
        if !matches!(body.dispute, DisputePolicy::Arbiter { .. }) {
            return Err(Error::protocol(
                "arbiter nomination only valid if dispute policy is arbiter",
            ));
        }
        decode_compressed_pubkey(&self.pubkey)?;
        if self.pubkey == body.mandante_pubkey {
            return Err(Error::protocol("arbiter must not be the mandante"));
        }
        if body.contratista_pubkey.as_ref() == Some(&self.pubkey) {
            return Err(Error::protocol("arbiter must not be the contratista"));
        }
        Ok(())
    }
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
            work_name: String::new(),
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

    #[test]
    fn arbiter_policy_has_no_pubkey() {
        let policy = DisputePolicy::Arbiter {
            window_secs: DEFAULT_ARBITER_WINDOW_SECS,
        };
        policy.validate().unwrap();
        let json = serde_json::to_value(&policy).unwrap();
        assert_eq!(json["policy"], "arbiter");
        assert!(json.get("pubkey").is_none());
        assert_eq!(json["window_secs"], DEFAULT_ARBITER_WINDOW_SECS);
    }

    #[test]
    fn nomination_is_outside_contract_id() {
        let body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            work_name: String::new(),
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![PartidaSpec {
                id: 1,
                description: "Muro".into(),
                amount_minor: 150_000,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: pk(1),
            contratista_pubkey: Some(pk(2)),
            dispute: DisputePolicy::Arbiter { window_secs: 15 },
        };
        let id = contract_id(&body).unwrap();
        let nom = ArbiterNomination {
            contract_id: id.clone(),
            pubkey: pk(9),
            mandante_sig: None,
            contratista_sig: None,
        };
        nom.validate_against(&body).unwrap();
        assert_eq!(contract_id(&body).unwrap(), id);
        assert!(!nom.fully_signed());
    }

    #[test]
    fn nomination_rejects_parties_and_unwind() {
        let mut body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            work_name: String::new(),
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![PartidaSpec {
                id: 1,
                description: "Muro".into(),
                amount_minor: 150_000,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: pk(1),
            contratista_pubkey: Some(pk(2)),
            dispute: DisputePolicy::Arbiter { window_secs: 15 },
        };
        let id = contract_id(&body).unwrap();
        let as_m = ArbiterNomination {
            contract_id: id.clone(),
            pubkey: pk(1),
            mandante_sig: None,
            contratista_sig: None,
        };
        assert!(as_m
            .validate_against(&body)
            .unwrap_err()
            .to_string()
            .contains("mandante"));
        let as_c = ArbiterNomination {
            contract_id: id.clone(),
            pubkey: pk(2),
            mandante_sig: None,
            contratista_sig: None,
        };
        assert!(as_c
            .validate_against(&body)
            .unwrap_err()
            .to_string()
            .contains("contratista"));
        body.dispute = DisputePolicy::Unwind;
        let ok_pk = ArbiterNomination {
            contract_id: id,
            pubkey: pk(9),
            mandante_sig: None,
            contratista_sig: None,
        };
        assert!(ok_pk
            .validate_against(&body)
            .unwrap_err()
            .to_string()
            .contains("only valid if dispute policy is arbiter"));
    }

    #[test]
    fn fee_burn_policy_roundtrip_and_deadlines() {
        let policy = DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000);
        policy.validate().unwrap();
        let json = serde_json::to_value(&policy).unwrap();
        assert_eq!(json["policy"], "fee_burn");
        assert_eq!(json["t1"], 1_700_000_000);
        assert_eq!(json["t2"], 1_800_000_000);
        assert!(DisputePolicy::fee_burn(1_800_000_000, 1_700_000_000)
            .validate()
            .is_err());
        assert!(DisputePolicy::fee_burn(100, 200).validate().is_err());
        assert!(!ARBITER_ENABLED);
    }

    #[test]
    fn fee_burn_body_rejects_t_project_before_t2() {
        let body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            work_name: "Casa".into(),
            bond_bps: 1000,
            t_project: 1_750_000_000,
            partidas: vec![PartidaSpec {
                id: 1,
                description: "Radier".into(),
                amount_minor: 10_000,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: pk(1),
            contratista_pubkey: Some(pk(2)),
            dispute: DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000),
        };
        assert!(body.validate().unwrap_err().to_string().contains("t_project"));
        let mut ok = body.clone();
        ok.t_project = 1_800_000_000;
        ok.validate().unwrap();
        assert_eq!(ok.terms().work_name, "Casa");
    }

    #[test]
    fn work_name_omitted_keeps_legacy_contract_id() {
        let mut body = ContractBody {
            network: Network::Regtest,
            unit: Unit::Usd,
            work_name: String::new(),
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
        let id_empty = contract_id(&body).unwrap();
        let json = serde_json::to_value(&body).unwrap();
        assert!(json.get("work_name").is_none());
        body.work_name = "Obra Norte".into();
        let id_named = contract_id(&body).unwrap();
        assert_ne!(id_empty, id_named);
    }
}
