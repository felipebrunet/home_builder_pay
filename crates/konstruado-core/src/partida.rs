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

/// One line per installment. Empty is allowed; extra items are dropped,
/// missing ones are filled with an empty string.
pub fn ajusta_detalles(n: u32, detalles: Vec<String>) -> Vec<String> {
    let mut d: Vec<String> = detalles.into_iter().map(|s| limpia_detalle(&s)).collect();
    d.resize(n as usize, String::new());
    d
}

pub fn limpia_detalle(s: &str) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 200 {
        t.chars().take(200).collect()
    } else {
        t
    }
}

pub fn titulo_partida(i: usize, detalle: &str) -> String {
    if detalle.is_empty() {
        format!("Partida {}", i + 1)
    } else {
        detalle.to_string()
    }
}

pub const MAX_NOTA: usize = 50;

pub fn limpia_nota(s: &str) -> Result<String, Error> {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > MAX_NOTA {
        Err(Error::Nota)
    } else {
        Ok(t)
    }
}

pub fn porcentaje(n: u32) -> Result<u32, Error> {
    if (1..=100).contains(&n) {
        Ok(n)
    } else {
        Err(Error::Porcentaje)
    }
}

pub fn monto_pct(total: u64, pct: u32) -> u64 {
    total * u64::from(pct) / 100
}
