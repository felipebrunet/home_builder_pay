use crate::error::Error;

/// Number of installments. Each is `garantia` from the principal and `garantia`
/// from the contractor, so capital in play is always equal.
pub fn n_partidas(trabajo: u64, garantia: u64) -> Result<u32, Error> {
    if trabajo == 0 {
        return Err(Error::Monto);
    }
    if garantia == 0 {
        return Err(Error::Garantia);
    }
    if trabajo % garantia != 0 {
        return Err(Error::NoDivide);
    }
    Ok((trabajo / garantia) as u32)
}

/// Amount each side locks for one installment.
pub fn capital_por_lado(garantia: u64) -> u64 {
    garantia
}

pub fn monto(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}
