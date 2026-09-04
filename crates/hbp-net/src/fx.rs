//! BTC price preview: Yadio → CoinGecko → CoinMarketCap. Cache ~60s.

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::http::get_text;
use hbp_core::Unit;

#[derive(Debug, Clone, PartialEq)]
pub struct FxQuote {
    pub unit: Unit,
    /// One BTC in *major* units of `unit` (e.g. 80_000 USD).
    pub btc_price_major: f64,
    pub source: &'static str,
}

static CACHE: Mutex<Option<(Instant, Unit, FxQuote)>> = Mutex::new(None);
const TTL: Duration = Duration::from_secs(60);

pub fn fiat_ticker(unit: Unit) -> Option<&'static str> {
    match unit {
        Unit::Usd => Some("USD"),
        Unit::Eur => Some("EUR"),
        Unit::Gbp => Some("GBP"),
        Unit::Clp => Some("CLP"),
        Unit::Ars => Some("ARS"),
        Unit::Mxn => Some("MXN"),
        Unit::Brl => Some("BRL"),
        Unit::Pen => Some("PEN"),
        Unit::Cop => Some("COP"),
        Unit::Uyu => Some("UYU"),
        Unit::Uf => Some("UF"),
        Unit::Btc | Unit::Sats => None,
    }
}

/// Sats equivalent of a stored `amount_minor` given an FX quote (or identity for BTC/SATS).
pub fn preview_sats(amount_minor: u64, unit: Unit, quote: Option<&FxQuote>) -> Option<u64> {
    match unit {
        Unit::Sats => Some(amount_minor),
        Unit::Btc => amount_minor.checked_mul(1_000_000),
        _ => {
            let q = quote?;
            if q.unit != unit || q.btc_price_major <= 0.0 {
                return None;
            }
            let price_minor = (q.btc_price_major * 100.0).round() as u64;
            hbp_core::fiat_minor_to_sats(amount_minor, price_minor).ok()
        }
    }
}

pub fn quote_btc(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    if fiat_ticker(unit).is_none() {
        return Err(crate::Error::msg("no fiat FX for this unit"));
    }
    if let Ok(g) = CACHE.lock() {
        if let Some((at, u, q)) = g.as_ref() {
            if *u == unit && at.elapsed() < TTL {
                return Ok(q.clone());
            }
        }
    }
    let quote = fetch_yadio(unit, socks)
        .or_else(|_| fetch_coingecko(unit, socks))
        .or_else(|_| fetch_cmc(unit, socks))?;
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), unit, quote.clone()));
    }
    Ok(quote)
}

fn ticker(unit: Unit) -> crate::Result<&'static str> {
    fiat_ticker(unit).ok_or_else(|| crate::Error::msg("unsupported FX unit"))
}

fn fetch_yadio(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    let t = ticker(unit)?;
    let raw = get_text(socks, "https://api.yadio.io/exrates/BTC")?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let price = v
        .get("BTC")
        .and_then(|b| b.get(t))
        .and_then(|x| x.as_f64())
        .or_else(|| v.get(t).and_then(|x| x.as_f64()))
        .ok_or_else(|| crate::Error::msg("yadio: no ticker"))?;
    if price <= 0.0 {
        return Err(crate::Error::msg("yadio: bad price"));
    }
    Ok(FxQuote {
        unit,
        btc_price_major: price,
        source: "Yadio",
    })
}

fn fetch_coingecko(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    let t = ticker(unit)?.to_ascii_lowercase();
    let url =
        format!("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies={t}");
    let raw = get_text(socks, &url)?;
    let v: serde_json::Value = serde_json::from_str(&raw)?;
    let price = v
        .get("bitcoin")
        .and_then(|b| b.get(&t))
        .and_then(|x| x.as_f64())
        .ok_or_else(|| crate::Error::msg("coingecko: no ticker"))?;
    if price <= 0.0 {
        return Err(crate::Error::msg("coingecko: bad price"));
    }
    Ok(FxQuote {
        unit,
        btc_price_major: price,
        source: "CoinGecko",
    })
}

#[derive(Deserialize)]
struct CmcWrap {
    data: Option<serde_json::Value>,
}

fn fetch_cmc(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    let t = ticker(unit)?;
    let url = format!(
        "https://api.coinmarketcap.com/data-api/v3/cryptocurrency/listing?start=1&limit=1&convert={t}"
    );
    let raw = get_text(socks, &url)?;
    let wrap: CmcWrap = serde_json::from_str(&raw).unwrap_or(CmcWrap { data: None });
    let listing = wrap
        .data
        .or_else(|| serde_json::from_str(&raw).ok())
        .ok_or_else(|| crate::Error::msg("cmc: no data"))?;
    let row = listing
        .get("cryptoCurrencyList")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .or_else(|| {
            listing
                .get("body")
                .and_then(|b| b.get("data"))
                .and_then(|a| a.as_array())
                .and_then(|a| a.first())
        })
        .ok_or_else(|| crate::Error::msg("cmc: no row"))?;
    let quotes = row
        .get("quotes")
        .and_then(|q| q.as_array())
        .ok_or_else(|| crate::Error::msg("cmc: no quotes"))?;
    for q in quotes {
        let name = q
            .get("name")
            .or_else(|| q.get("symbol"))
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if name.eq_ignore_ascii_case(t) {
            let price = q
                .get("price")
                .and_then(|p| p.as_f64())
                .ok_or_else(|| crate::Error::msg("cmc: bad price"))?;
            if price <= 0.0 {
                break;
            }
            return Ok(FxQuote {
                unit,
                btc_price_major: price,
                source: "CoinMarketCap",
            });
        }
    }
    Err(crate::Error::msg("cmc: ticker missing"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_sats_identity_units() {
        assert_eq!(preview_sats(12_345, Unit::Sats, None), Some(12_345));
        assert_eq!(preview_sats(100, Unit::Btc, None), Some(100_000_000));
        assert!(preview_sats(150_000, Unit::Usd, None).is_none());
        let q = FxQuote {
            unit: Unit::Usd,
            btc_price_major: 80_000.0,
            source: "test",
        };
        assert_eq!(preview_sats(150_000, Unit::Usd, Some(&q)), Some(1_875_000));
    }

    #[test]
    fn parses_yadio_shape() {
        let raw = r#"{"BTC":{"USD":80000.0,"CLP":75000000}}"#;
        let v: serde_json::Value = serde_json::from_str(raw).unwrap();
        assert_eq!(v["BTC"]["USD"].as_f64(), Some(80_000.0));
    }
}
