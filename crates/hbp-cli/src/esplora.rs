//! HTTP client for a public (or self-hosted) Esplora. Not our server.

use anyhow::{bail, Context, Result};
use bitcoin::{Address, OutPoint};
use serde::Deserialize;

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

pub struct Esplora {
    pub base: String,
}

#[derive(Debug, Deserialize)]
struct UtxoJson {
    txid: String,
    vout: u32,
    value: u64,
    status: StatusJson,
}

#[derive(Debug, Deserialize)]
struct StatusJson {
    confirmed: bool,
}

impl Esplora {
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// Probe candidates (tip height) and keep the first that answers.
    pub fn connect(candidates: &[String]) -> Result<Self> {
        let mut errors = Vec::new();
        for raw in candidates {
            let client = Esplora::new(raw);
            match client.get("/blocks/tip/height") {
                Ok(_) => return Ok(client),
                Err(e) => errors.push(format!("{}: {e:#}", client.base)),
            }
        }
        bail!(
            "no Esplora reachable (tried {}):\n{}",
            candidates.len(),
            errors.join("\n")
        )
    }

    fn get(&self, path: &str) -> Result<ureq::Response> {
        let url = format!("{}{path}", self.base);
        match ureq::get(&url)
            .set("User-Agent", "home_builder_pay")
            .timeout(TIMEOUT)
            .call()
        {
            Ok(r) => Ok(r),
            Err(ureq::Error::Status(code, resp)) => {
                let body = resp.into_string().unwrap_or_default();
                let snippet: String = body.chars().take(180).collect();
                bail!("GET {url} -> HTTP {code} {snippet}");
            }
            Err(ureq::Error::Transport(t)) => {
                bail!("GET {url} failed: {t}");
            }
        }
    }

    pub fn address_utxos(&self, addr: &Address) -> Result<Vec<(OutPoint, u64, bool)>> {
        let path = format!("/address/{addr}/utxo");
        let list: Vec<UtxoJson> = self.get(&path)?.into_json().context("esplora utxo json")?;
        let mut out = Vec::with_capacity(list.len());
        for u in list {
            let op = format!("{}:{}", u.txid, u.vout)
                .parse()
                .with_context(|| format!("outpoint {}:{}", u.txid, u.vout))?;
            out.push((op, u.value, u.status.confirmed));
        }
        Ok(out)
    }

    pub fn tx_hex(&self, txid: &str) -> Result<String> {
        let path = format!("/tx/{txid}/hex");
        let hex = self.get(&path)?.into_string().context("esplora tx hex")?;
        Ok(hex.trim().to_string())
    }
}
