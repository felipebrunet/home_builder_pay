use serde::{Deserialize, Serialize};

/// Smallest integer unit per major unit of the contract currency.
pub const MINOR_PER_MAJOR: u64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Unit {
    Usd,
    Uf,
    Clp,
    Btc,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Uf => "UF",
            Self::Clp => "CLP",
            Self::Btc => "BTC",
        }
    }
}

impl std::str::FromStr for Unit {
    type Err = crate::Error;

    fn from_str(s: &str) -> crate::Result<Self> {
        match s.to_ascii_uppercase().as_str() {
            "USD" => Ok(Self::Usd),
            "UF" => Ok(Self::Uf),
            "CLP" => Ok(Self::Clp),
            "BTC" => Ok(Self::Btc),
            other => Err(crate::Error::protocol(format!("unknown unit {other}"))),
        }
    }
}

pub fn minor_from_major(major: &str) -> crate::Result<u64> {
    let major = major.trim().replace('_', "");
    if major.is_empty() {
        return Err(crate::Error::protocol("empty amount"));
    }
    let (whole, frac) = match major.split_once('.') {
        Some((w, f)) => (w, f),
        None => (major.as_str(), ""),
    };
    if frac.len() > 2 {
        return Err(crate::Error::protocol(
            "at most two decimal places (stored as 1/100 of the unit)",
        ));
    }
    let whole: u64 = whole
        .parse()
        .map_err(|_| crate::Error::protocol("invalid amount"))?;
    let mut frac_pad = frac.to_string();
    while frac_pad.len() < 2 {
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
        .checked_mul(MINOR_PER_MAJOR)
        .and_then(|w| w.checked_add(frac))
        .ok_or_else(|| crate::Error::protocol("amount overflow"))
}

/// Convert contract-currency minor units to sats given a BTC price in the same minor units.
///
/// Example: 150_000 cents ($1,500) at 8_000_000 cents/BTC ($80,000) → 1_875_000 sats.
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
    }

    #[test]
    fn converts_sats() {
        assert_eq!(fiat_minor_to_sats(150_000, 8_000_000).unwrap(), 1_875_000);
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
