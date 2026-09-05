//! Public Esplora (blockstream / mempool Signet). Optional Tor SOCKS.

use std::net::SocketAddr;

use serde::Deserialize;

use crate::http::{get_text, post_text};

#[derive(Debug, Clone)]
pub struct EsploraUtxo {
    pub txid: String,
    pub vout: u32,
    pub sats: u64,
    pub confirmed: bool,
}

#[derive(Debug, Clone)]
pub struct EsploraTxOut {
    pub value: u64,
    pub scriptpubkey: String,
}

#[derive(Debug, Clone)]
pub struct EsploraTx {
    pub txid: String,
    pub vout: Vec<EsploraTxOut>,
    pub confirmed: bool,
}

#[derive(Deserialize)]
struct UtxoJson {
    txid: String,
    vout: u32,
    value: u64,
    status: StatusJson,
}

#[derive(Deserialize)]
struct StatusJson {
    confirmed: bool,
}

#[derive(Deserialize)]
struct TxJson {
    txid: String,
    vout: Vec<VoutJson>,
    status: StatusJson,
}

#[derive(Deserialize)]
struct VoutJson {
    value: u64,
    #[serde(default)]
    scriptpubkey: String,
}

pub fn esplora_try_bases(bases: &[&str], socks: Option<SocketAddr>) -> crate::Result<String> {
    let mut errs = Vec::new();
    for raw in bases {
        let base = raw.trim().trim_end_matches('/');
        if base.is_empty() {
            continue;
        }
        match get_text(socks, &format!("{base}/blocks/tip/height")) {
            Ok(_) => return Ok(base.to_string()),
            Err(e) => errs.push(format!("{base}: {e}")),
        }
    }
    Err(crate::Error::msg(format!(
        "ningún Esplora responde ({})",
        errs.join("; ")
    )))
}

pub fn esplora_address_utxos(
    base: &str,
    socks: Option<SocketAddr>,
    addr: &str,
) -> crate::Result<Vec<EsploraUtxo>> {
    let base = base.trim_end_matches('/');
    let raw = get_text(socks, &format!("{base}/address/{addr}/utxo"))?;
    let list: Vec<UtxoJson> = serde_json::from_str(&raw)?;
    Ok(list
        .into_iter()
        .map(|u| EsploraUtxo {
            txid: u.txid,
            vout: u.vout,
            sats: u.value,
            confirmed: u.status.confirmed,
        })
        .collect())
}

pub fn esplora_address_txs(
    base: &str,
    socks: Option<SocketAddr>,
    addr: &str,
) -> crate::Result<Vec<EsploraTx>> {
    let base = base.trim_end_matches('/');
    let raw = get_text(socks, &format!("{base}/address/{addr}/txs"))?;
    let list: Vec<TxJson> = serde_json::from_str(&raw)?;
    Ok(list
        .into_iter()
        .map(|t| EsploraTx {
            txid: t.txid,
            confirmed: t.status.confirmed,
            vout: t
                .vout
                .into_iter()
                .map(|o| EsploraTxOut {
                    value: o.value,
                    scriptpubkey: o.scriptpubkey,
                })
                .collect(),
        })
        .collect())
}

pub fn esplora_tx_hex(base: &str, socks: Option<SocketAddr>, txid: &str) -> crate::Result<String> {
    let base = base.trim_end_matches('/');
    let raw = get_text(socks, &format!("{base}/tx/{txid}/hex"))?;
    Ok(raw.trim().to_string())
}

/// Blockstream / mempool Esplora: `POST {base}/tx` with raw hex.
pub fn esplora_broadcast_url(base: &str) -> String {
    format!("{}/tx", base.trim().trim_end_matches('/'))
}

pub fn esplora_broadcast_tx(
    base: &str,
    socks: Option<SocketAddr>,
    tx_hex: &str,
) -> crate::Result<String> {
    let url = esplora_broadcast_url(base);
    let body = tx_hex.trim();
    if body.is_empty() {
        return Err(crate::Error::msg("falta el tx hex"));
    }
    let resp = post_text(socks, &url, body)?;
    Ok(resp.trim().to_string())
}

pub fn esplora_try_broadcast(bases: &[&str], socks: Option<SocketAddr>, tx_hex: &str) -> crate::Result<(String, String)> {
    let mut errs = Vec::new();
    for raw in bases {
        let base = raw.trim().trim_end_matches('/');
        if base.is_empty() {
            continue;
        }
        match esplora_broadcast_tx(base, socks, tx_hex) {
            Ok(txid) => return Ok((txid, base.to_string())),
            Err(e) => errs.push(format!("{base}: {e}")),
        }
    }
    Err(crate::Error::msg(format!(
        "no pude publicar ({})",
        errs.join("; ")
    )))
}

/// First tx that pays both quoted escrow amounts (boleta + partida).
pub fn find_dual_amount<'a>(
    txs: &'a [EsploraTx],
    bond_sats: u64,
    partida_sats: u64,
) -> Option<&'a EsploraTx> {
    txs.iter().find(|t| {
        let has_bond = t.vout.iter().any(|o| o.value == bond_sats);
        let has_p1 = t.vout.iter().any(|o| o.value == partida_sats);
        has_bond && has_p1
    })
}

pub fn find_dual_amount_txid(txs: &[EsploraTx], bond_sats: u64, partida_sats: u64) -> Option<&str> {
    find_dual_amount(txs, bond_sats, partida_sats).map(|t| t.txid.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utxo_json() {
        let raw = r#"[{"txid":"aa","vout":1,"value":50000,"status":{"confirmed":true}}]"#;
        let list: Vec<UtxoJson> = serde_json::from_str(raw).unwrap();
        assert_eq!(list[0].value, 50_000);
        assert!(list[0].status.confirmed);
    }

    #[test]
    fn finds_bond_and_p1_on_same_tx() {
        let txs = vec![
            EsploraTx {
                txid: "no".into(),
                confirmed: true,
                vout: vec![EsploraTxOut {
                    value: 10,
                    scriptpubkey: String::new(),
                }],
            },
            EsploraTx {
                txid: "yes".into(),
                confirmed: false,
                vout: vec![
                    EsploraTxOut {
                        value: 20_000,
                        scriptpubkey: String::new(),
                    },
                    EsploraTxOut {
                        value: 30_000,
                        scriptpubkey: String::new(),
                    },
                ],
            },
        ];
        assert_eq!(find_dual_amount_txid(&txs, 20_000, 30_000), Some("yes"));
    }

    #[test]
    fn broadcast_url_is_base_plus_tx() {
        assert_eq!(
            esplora_broadcast_url("https://blockstream.info/signet/api/"),
            "https://blockstream.info/signet/api/tx"
        );
        assert_eq!(
            esplora_broadcast_url("https://mempool.space/signet/api"),
            "https://mempool.space/signet/api/tx"
        );
    }
}
