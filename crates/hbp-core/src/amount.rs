use serde::{Deserialize, Serialize};

/// Smallest integer unit per major unit of a two-decimal contract currency.
///
/// [`Unit::Sats`] is 1:1 (the major amount *is* the stored minor). Everything
/// else uses this scale (cents / 1/100 BTC).
pub const MINOR_PER_MAJOR: u64 = 100;

/// Contract account unit. Serialized uppercase (`USD`, `SATS`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Unit {
    Usd,
    Uf,
    Clp,
    Eur,
    Gbp,
    Ars,
    Mxn,
    Brl,
    Pen,
    Cop,
    Uyu,
    Btc,
    Sats,
}

impl Unit {
    /// Dropdown / parse catalog. USD first (product default).
    pub const ALL: [Self; 13] = [
        Self::Usd,
        Self::Uf,
        Self::Clp,
        Self::Eur,
        Self::Gbp,
        Self::Ars,
        Self::Mxn,
        Self::Brl,
        Self::Pen,
        Self::Cop,
        Self::Uyu,
        Self::Btc,
        Self::Sats,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Uf => "UF",
            Self::Clp => "CLP",
            Self::Eur => "EUR",
            Self::Gbp => "GBP",
            Self::Ars => "ARS",
            Self::Mxn => "MXN",
            Self::Brl => "BRL",
            Self::Pen => "PEN",
            Self::Cop => "COP",
            Self::Uyu => "UYU",
            Self::Btc => "BTC",
            Self::Sats => "SATS",
        }
    }

    /// BTC and SATS are already bitcoin denominations (no fiat FX to get sats).
    pub fn is_bitcoin_denom(self) -> bool {
        matches!(self, Self::Btc | Self::Sats)
    }

    /// How many stored `amount_minor` units make one typed major unit.
    pub fn minor_per_major(self) -> u64 {
        match self {
            Self::Sats => 1,
            _ => MINOR_PER_MAJOR,
        }
    }
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Unit {
    type Err = crate::Error;

    fn from_str(s: &str) -> crate::Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "USD" => Ok(Self::Usd),
            "UF" => Ok(Self::Uf),
            "CLP" => Ok(Self::Clp),
            "EUR" => Ok(Self::Eur),
            "GBP" => Ok(Self::Gbp),
            "ARS" => Ok(Self::Ars),
            "MXN" => Ok(Self::Mxn),
            "BRL" => Ok(Self::Brl),
            "PEN" => Ok(Self::Pen),
            "COP" => Ok(Self::Cop),
            "UYU" => Ok(Self::Uyu),
            "BTC" => Ok(Self::Btc),
            "SAT" | "SATS" => Ok(Self::Sats),
            other => Err(crate::Error::protocol(format!("unknown unit {other}"))),
        }
    }
}

/// Parse a major amount in the two-decimal (1/100) scale. Fiat / UF / BTC.
pub fn minor_from_major(major: &str) -> crate::Result<u64> {
    parse_major_scaled(major, MINOR_PER_MAJOR)
}

/// Parse a typed major amount in `unit` (SATS are integers; others two decimals).
pub fn parse_major_amount(major: &str, unit: Unit) -> crate::Result<u64> {
    parse_major_scaled(major, unit.minor_per_major())
}

/// Format stored minor units for the given contract unit.
pub fn format_major_amount(minor: u64, unit: Unit) -> String {
    let scale = unit.minor_per_major();
    if scale == 1 {
        minor.to_string()
    } else {
        format!("{:.2}", minor as f64 / scale as f64)
    }
}

fn parse_major_scaled(major: &str, scale: u64) -> crate::Result<u64> {
    let major = major.trim().replace('_', "");
    if major.is_empty() {
        return Err(crate::Error::protocol("empty amount"));
    }
    let (whole, frac) = match major.split_once('.') {
        Some((w, f)) => (w, f),
        None => (major.as_str(), ""),
    };
    let max_frac = match scale {
        1 => 0,
        MINOR_PER_MAJOR => 2,
        _ => {
            return Err(crate::Error::protocol("unsupported amount scale"));
        }
    };
    if scale == 1 {
        if !frac.is_empty() && frac.chars().any(|c| c != '0') {
            return Err(crate::Error::protocol(
                "SATS are whole satoshis (no fractional part)",
            ));
        }
        return whole
            .parse()
            .map_err(|_| crate::Error::protocol("invalid amount"));
    }
    if frac.len() > max_frac {
        return Err(crate::Error::protocol(
            "at most two decimal places (stored as 1/100 of the unit)",
        ));
    }
    let whole: u64 = whole
        .parse()
        .map_err(|_| crate::Error::protocol("invalid amount"))?;
    let mut frac_pad = frac.to_string();
    while frac_pad.len() < max_frac {
        frac_pad.push('0');
    }
    let frac: u64 = if frac_pad.is_empty() {
        0
    } else {
        frac_pad
            .parse()
            .map_err(|_| crate::Error::protocol("invalid amount"))?
    };
    whole
        .checked_mul(scale)
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(|| crate::Error::protocol("amount overflow"))
}

/// BTC price in stored minor units of `unit` (1/100, except SATS).
///
/// `74_492_748` CLP/BTC → `7_449_274_800`. Never treat a USD major price as CLP.
pub fn btc_price_to_minor(btc_price_major: f64, unit: Unit) -> crate::Result<u64> {
    if !btc_price_major.is_finite() || btc_price_major <= 0.0 {
        return Err(crate::Error::protocol("btc price must be > 0"));
    }
    let v = (btc_price_major * unit.minor_per_major() as f64).round();
    if v < 1.0 || v > u64::MAX as f64 {
        return Err(crate::Error::protocol("btc price out of range"));
    }
    Ok(v as u64)
}

/// Convert contract-currency minor units to sats given a BTC price in the same minor units.
///
/// Example: 150_000 cents ($1,500) at 8_000_000 cents/BTC ($80,000) → 1_875_000 sats.
/// 500_000 (5_000 CLP) at 7_449_274_800 (74_492_748 CLP/BTC) → 6_712 sats.
pub fn fiat_minor_to_sats(amount_minor: u64, btc_price_minor: u64) -> crate::Result<u64> {
    if btc_price_minor == 0 {
        return Err(crate::Error::protocol("btc price must be > 0"));
    }
    amount_minor
        .checked_mul(100_000_000)
        .and_then(|v| v.checked_div(btc_price_minor))
        .ok_or_else(|| crate::Error::protocol("sats conversion overflow"))
}

pub fn bond_minor(total_minor: u64, bond_bps: u16) -> crate::Result<u64> {
    if bond_bps == 0 {
        return Err(crate::Error::protocol("bond_bps must be > 0"));
    }
    total_minor
        .checked_mul(u64::from(bond_bps))
        .and_then(|v| v.checked_div(10_000))
        .filter(|v| *v > 0)
        .ok_or_else(|| crate::Error::protocol("bond amount is zero"))
}

/// Product default: guarantee is 10% of total principal.
pub const DEFAULT_BOND_BPS: u16 = 1000;

/// How many equal stages make each stage's principal equal the bond.
///
/// `10000 / bond_bps` when that divides evenly (10% → 10 stages).
pub fn equal_stage_count(bond_bps: u16) -> crate::Result<u32> {
    if bond_bps == 0 {
        return Err(crate::Error::protocol("bond_bps must be > 0"));
    }
    if 10_000 % u32::from(bond_bps) != 0 {
        return Err(crate::Error::protocol(format!(
            "bond_bps {bond_bps} does not divide 10000; cannot split into equal stages = bond"
        )));
    }
    Ok(10_000 / u32::from(bond_bps))
}

/// One amount per stage so each equals the bond and they sum to `total_minor`.
///
/// If `total` is not an exact multiple of the bond (rounding), the last stage
/// absorbs the remainder and [`stages_equal_bond`] will be false.
pub fn suggest_equal_stage_minors(total_minor: u64, bond_bps: u16) -> crate::Result<Vec<u64>> {
    let n = u64::from(equal_stage_count(bond_bps)?);
    let bond = bond_minor(total_minor, bond_bps)?;
    let exact = n
        .checked_mul(bond)
        .ok_or_else(|| crate::Error::protocol("stage plan overflow"))?;
    if exact == total_minor {
        return Ok(vec![bond; n as usize]);
    }
    if exact < total_minor {
        let mut stages = vec![bond; n as usize];
        stages[n as usize - 1] = bond + (total_minor - exact);
        return Ok(stages);
    }
    Err(crate::Error::protocol(format!(
        "total {total_minor} is smaller than {n} × bond {bond}"
    )))
}

pub fn stages_equal_bond(total_minor: u64, bond_bps: u16, stages: &[u64]) -> bool {
    let Ok(bond) = bond_minor(total_minor, bond_bps) else {
        return false;
    };
    !stages.is_empty()
        && stages.iter().all(|s| *s == bond)
        && stages.iter().copied().sum::<u64>() == total_minor
}

/// Product guidance: each stage should equal the 10% bond.
pub fn stage_bond_warnings(bond_minor_amount: u64, stage_minor: u64) -> Vec<String> {
    if stage_minor == bond_minor_amount {
        Vec::new()
    } else {
        vec![format!(
            "partida {stage_minor} != boleta {bond_minor_amount}; product default is stage amount = bond (10% of total → N equal stages)"
        )]
    }
}

/// Human warnings, never hard errors.
pub fn bond_warnings(bond_minor_amount: u64, total_minor: u64, partida_minor: u64) -> Vec<String> {
    let mut out = Vec::new();
    if total_minor > 0 && bond_minor_amount * 20 < total_minor {
        out.push("boleta under 5% of the project — weak commitment".into());
    }
    if total_minor > 0 && bond_minor_amount * 10 > total_minor * 3 {
        out.push("boleta over 30% of the project — contractor capital may not exist".into());
    }
    out.extend(stage_bond_warnings(bond_minor_amount, partida_minor));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_major() {
        assert_eq!(minor_from_major("1500").unwrap(), 150_000);
        assert_eq!(minor_from_major("1500.5").unwrap(), 150_050);
        assert_eq!(minor_from_major("1500.50").unwrap(), 150_050);
        assert_eq!(parse_major_amount("1500", Unit::Usd).unwrap(), 150_000);
        assert_eq!(parse_major_amount("100000", Unit::Sats).unwrap(), 100_000);
        assert_eq!(parse_major_amount("100.0", Unit::Sats).unwrap(), 100);
        assert!(parse_major_amount("1.5", Unit::Sats).is_err());
        assert_eq!(format_major_amount(150_000, Unit::Clp), "1500.00");
        assert_eq!(format_major_amount(100_000, Unit::Sats), "100000");
    }

    #[test]
    fn unit_parse_display_serde_roundtrip() {
        for u in Unit::ALL {
            assert_eq!(u.to_string(), u.as_str());
            assert_eq!(u.as_str().parse::<Unit>().unwrap(), u);
            let json = serde_json::to_string(&u).unwrap();
            assert_eq!(json, format!("\"{}\"", u.as_str()));
            assert_eq!(serde_json::from_str::<Unit>(&json).unwrap(), u);
        }
        assert_eq!("sat".parse::<Unit>().unwrap(), Unit::Sats);
        assert!("XYZ".parse::<Unit>().is_err());
        assert!(!Unit::Usd.is_bitcoin_denom());
        assert!(Unit::Sats.is_bitcoin_denom());
        assert!(Unit::Btc.is_bitcoin_denom());
    }

    #[test]
    fn converts_sats() {
        assert_eq!(fiat_minor_to_sats(150_000, 8_000_000).unwrap(), 1_875_000);
    }

    #[test]
    fn five_thousand_clp_at_known_clp_btc() {
        let amount = parse_major_amount("5000", Unit::Clp).unwrap();
        assert_eq!(amount, 500_000, "CLP is stored as 1/100");
        let price = btc_price_to_minor(74_492_748.0, Unit::Clp).unwrap();
        assert_eq!(price, 7_449_274_800);
        assert_eq!(fiat_minor_to_sats(amount, price).unwrap(), 6_712);
        let usd_price = btc_price_to_minor(79_600.0, Unit::Usd).unwrap();
        assert_eq!(usd_price, 7_960_000);
        let wrong = fiat_minor_to_sats(amount, usd_price).unwrap();
        assert!(
            wrong > 1_000_000,
            "USD/BTC applied to CLP is ~1000× too high ({wrong})"
        );
        assert_ne!(wrong, 6_712);
    }

    #[test]
    fn bond_is_percent_of_total() {
        assert_eq!(bond_minor(2_000_000, 1_000).unwrap(), 200_000);
    }

    #[test]
    fn ten_percent_bond_suggests_ten_equal_stages() {
        assert_eq!(equal_stage_count(DEFAULT_BOND_BPS).unwrap(), 10);
        let stages = suggest_equal_stage_minors(1_000_000, DEFAULT_BOND_BPS).unwrap();
        assert_eq!(stages.len(), 10);
        assert!(stages.iter().all(|s| *s == 100_000));
        assert!(stages_equal_bond(1_000_000, DEFAULT_BOND_BPS, &stages));
        assert!(stage_bond_warnings(100_000, 100_000).is_empty());
        assert!(!stage_bond_warnings(100_000, 50_000).is_empty());
    }
}
