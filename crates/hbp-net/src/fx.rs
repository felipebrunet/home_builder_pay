//! BTC price in the **contract unit** per BTC (CLP/BTC, USD/BTC, …).
//! Yadio → CoinGecko → CoinMarketCap. Cache ~60s. SATS/BTC skip FX.

use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::http::get_text;
use hbp_core::{btc_price_to_minor, Unit};

#[derive(Debug, Clone, PartialEq)]
pub struct FxQuote {
    pub unit: Unit,
    /// One BTC in *major* units of `unit` (e.g. 74_492_748 CLP, 80_000 USD).
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
            let price_minor = btc_price_to_minor(q.btc_price_major, q.unit).ok()?;
            hbp_core::fiat_minor_to_sats(amount_minor, price_minor).ok()
        }
    }
}

/// Price in stored minor units, only if the quote is for `want`.
pub fn fx_price_minor(q: &FxQuote, want: Unit) -> Option<u64> {
    if q.unit != want {
        return None;
    }
    btc_price_to_minor(q.btc_price_major, q.unit).ok()
}

pub fn fx_pair_label(unit: Unit) -> String {
    format!("{}/BTC", unit.as_str())
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
    if quote.unit != unit {
        return Err(crate::Error::msg(format!(
            "FX devolvió {}/BTC; el trato es {}",
            quote.unit,
            fx_pair_label(unit)
        )));
    }
    require_plausible_pair(unit, quote.btc_price_major)?;
    if let Ok(mut g) = CACHE.lock() {
        *g = Some((Instant::now(), unit, quote.clone()));
    }
    Ok(quote)
}

/// Reject a USD-sized number when the contract is CLP (the ~1000× bug).
pub fn require_plausible_pair(unit: Unit, btc_price_major: f64) -> crate::Result<()> {
    if !btc_price_major.is_finite() || btc_price_major <= 0.0 {
        return Err(crate::Error::msg("precio BTC inválido"));
    }
    match unit {
        Unit::Clp | Unit::Ars | Unit::Cop => {
            if btc_price_major < 100_000.0 {
                return Err(crate::Error::msg(format!(
                    "ese precio parece USD/BTC, no {}",
                    fx_pair_label(unit)
                )));
            }
        }
        Unit::Usd | Unit::Eur | Unit::Gbp => {
            if btc_price_major >= 1_000_000.0 {
                return Err(crate::Error::msg(format!(
                    "ese precio no parece {}",
                    fx_pair_label(unit)
                )));
            }
        }
        _ => {}
    }
    Ok(())
}

fn ticker(unit: Unit) -> crate::Result<&'static str> {
    fiat_ticker(unit).ok_or_else(|| crate::Error::msg("unsupported FX unit"))
}

pub fn parse_yadio_btc_price(raw: &str, ticker: &str) -> crate::Result<f64> {
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let t = ticker.to_ascii_uppercase();
    let price = v
        .get("BTC")
        .and_then(|b| b.get(&t).or_else(|| b.get(ticker)))
        .and_then(|x| x.as_f64())
        .or_else(|| v.get("result").and_then(|x| x.as_f64()))
        .or_else(|| v.get("rate").and_then(|x| x.as_f64()))
        .or_else(|| v.get(&t).and_then(|x| x.as_f64()))
        .ok_or_else(|| crate::Error::msg("yadio: no ticker"))?;
    if price <= 0.0 {
        return Err(crate::Error::msg("yadio: bad price"));
    }
    Ok(price)
}

fn fetch_yadio(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    let t = ticker(unit)?;
    let convert = format!("https://api.yadio.io/convert/1/BTC/{t}");
    let price = match get_text(socks, &convert) {
        Ok(raw) => parse_yadio_btc_price(&raw, t).ok(),
        Err(_) => None,
    };
    let price = match price {
        Some(p) => p,
        None => {
            let raw = get_text(socks, "https://api.yadio.io/exrates/BTC")?;
            parse_yadio_btc_price(&raw, t)?
        }
    };
    require_plausible_pair(unit, price)?;
    Ok(FxQuote {
        unit,
        btc_price_major: price,
        source: "Yadio",
    })
}

pub fn parse_coingecko_btc_price(raw: &str, ticker: &str) -> crate::Result<f64> {
    let t = ticker.to_ascii_lowercase();
    let v: serde_json::Value = serde_json::from_str(raw)?;
    let price = v
        .get("bitcoin")
        .and_then(|b| b.get(&t))
        .and_then(|x| x.as_f64())
        .ok_or_else(|| crate::Error::msg("coingecko: no ticker"))?;
    if price <= 0.0 {
        return Err(crate::Error::msg("coingecko: bad price"));
    }
    Ok(price)
}

fn fetch_coingecko(unit: Unit, socks: Option<SocketAddr>) -> crate::Result<FxQuote> {
    let t = ticker(unit)?.to_ascii_lowercase();
    let url =
        format!("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies={t}");
    let raw = get_text(socks, &url)?;
    let price = parse_coingecko_btc_price(&raw, &t)?;
    require_plausible_pair(unit, price)?;
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
            require_plausible_pair(unit, price)?;
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
        let clp = FxQuote {
            unit: Unit::Clp,
            btc_price_major: 74_492_748.0,
            source: "test",
        };
        assert_eq!(preview_sats(500_000, Unit::Clp, Some(&clp)), Some(6_712));
        assert!(
            preview_sats(500_000, Unit::Clp, Some(&q)).is_none(),
            "USD quote must not convert a CLP amount"
        );
        assert_eq!(fx_price_minor(&clp, Unit::Clp), Some(7_449_274_800));
        assert!(fx_price_minor(&q, Unit::Clp).is_none());
    }

    #[test]
    fn parses_yadio_and_coingecko_in_contract_unit() {
        let raw = r#"{"BTC":{"USD":80000.0,"CLP":74492748}}"#;
        assert_eq!(parse_yadio_btc_price(raw, "CLP").unwrap(), 74_492_748.0);
        assert_eq!(parse_yadio_btc_price(raw, "USD").unwrap(), 80_000.0);
        let conv = r#"{"result":74492748.0,"rate":74492748.0}"#;
        assert_eq!(parse_yadio_btc_price(conv, "CLP").unwrap(), 74_492_748.0);
        let cg = r#"{"bitcoin":{"clp":74492748,"usd":80000}}"#;
        assert_eq!(parse_coingecko_btc_price(cg, "CLP").unwrap(), 74_492_748.0);
        assert!(require_plausible_pair(Unit::Clp, 80_000.0).is_err());
        assert!(require_plausible_pair(Unit::Clp, 74_492_748.0).is_ok());
        assert_eq!(fx_pair_label(Unit::Clp), "CLP/BTC");
    }
}
