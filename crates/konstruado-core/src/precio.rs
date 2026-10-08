//! Montos en dólares y precio fijado al fondear.
//!
//! Las obras nuevas se publican en USD (centavos). El XMR de cada partida queda
//! fijo al proponer el encierre: quien propone pone el precio del momento
//! ([`PrecioFijado`]) y el otro lo acepta al confirmar y fondear. Los dos leen el
//! mismo `piconero` del estado de la obra, así que no hay que volver a convertir.
//! Las obras viejas (`Moneda::Unidades`) siguen con la escala fija de stagenet.

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// 1 XMR en piconero.
pub const PICONERO: u64 = 1_000_000_000_000;

/// Escala de las obras viejas: 1 unidad del trato en piconero (2000 unidades = 0,04 XMR).
pub const PICONERO_POR_UNIDAD: u64 = 20_000_000;

/// En qué están los montos de una obra u oferta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Moneda {
    /// Unidades ficticias (hasta 0.2.9). Se ven como antes.
    #[default]
    Unidades,
    /// Centavos de dólar.
    Usd,
}

impl Moneda {
    pub fn es_usd(self) -> bool {
        self == Moneda::Usd
    }
}

/// XMR de una partida fijado con el precio del momento del fondeo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrecioFijado {
    /// Lo que pone cada lado, en centavos de dólar.
    pub usd_centavos: u64,
    /// Precio de referencia: centavos de dólar por 1 XMR (mainnet).
    pub centavos_por_xmr: u64,
    /// Lo que pone cada lado, en piconero. Es el número que usa la caja.
    pub piconero: u64,
    /// `coingecko` o `kraken`.
    pub fuente: String,
    /// Cuándo se leyó el precio (unix, segundos).
    pub cuando: i64,
}

impl PrecioFijado {
    pub fn nuevo(usd_centavos: u64, centavos_por_xmr: u64, fuente: &str, cuando: i64) -> Result<Self, Error> {
        let piconero = usd_a_piconero(usd_centavos, centavos_por_xmr).ok_or(Error::Precio)?;
        Ok(Self {
            usd_centavos,
            centavos_por_xmr,
            piconero,
            fuente: fuente.to_string(),
            cuando,
        })
    }

    /// El `piconero` corresponde a los dólares y al precio (redondeo incluido).
    pub fn coherente(&self) -> bool {
        usd_a_piconero(self.usd_centavos, self.centavos_por_xmr) == Some(self.piconero)
    }

    /// Piconero de un porcentaje del monto fijado (pago de una partida).
    pub fn piconero_pct(&self, pct: u32) -> u64 {
        crate::partida::monto_pct(self.piconero, pct)
    }
}

/// Dólares → piconero al precio dado, redondeando al piconero más cercano.
/// `None` si el precio es cero, el resultado es cero o no entra en u64.
pub fn usd_a_piconero(usd_centavos: u64, centavos_por_xmr: u64) -> Option<u64> {
    if centavos_por_xmr == 0 || usd_centavos == 0 {
        return None;
    }
    let num = u128::from(usd_centavos) * u128::from(PICONERO);
    let den = u128::from(centavos_por_xmr);
    let q = (num + den / 2) / den;
    u64::try_from(q).ok().filter(|&p| p > 0)
}

/// Unidades viejas → piconero.
pub fn unidades_a_piconero(unidades: u64) -> Option<u64> {
    unidades.checked_mul(PICONERO_POR_UNIDAD)
}

/// Precio de una API (dólares por XMR, con decimales) → centavos.
pub fn precio_a_centavos(usd_por_xmr: f64) -> Option<u64> {
    if !usd_por_xmr.is_finite() || usd_por_xmr <= 0.0 || usd_por_xmr > 1e9 {
        return None;
    }
    let c = (usd_por_xmr * 100.0).round();
    if c < 1.0 {
        return None;
    }
    Some(c as u64)
}

/// `USD 1.500` / `USD 1.500,50` (es) · `USD 1,500` / `USD 1,500.50` (en).
pub fn fmt_usd(centavos: u64, es: bool) -> String {
    let (mil, dec) = if es { ('.', ',') } else { (',', '.') };
    let entero = centavos / 100;
    let cent = centavos % 100;
    let raw = entero.to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(mil);
        }
        out.push(ch);
    }
    let entero: String = out.chars().rev().collect();
    if cent == 0 {
        format!("USD {entero}")
    } else {
        format!("USD {entero}{dec}{cent:02}")
    }
}

/// Lee un monto en dólares escrito a mano: `1500`, `1.500`, `1,500.50`,
/// `1.500,5`, `USD 20`. El último separador seguido de 1 o 2 cifras es el
/// decimal; los demás separan miles.
pub fn leer_usd(texto: &str) -> Result<u64, Error> {
    let t: String = texto
        .trim()
        .trim_start_matches(|c: char| c == '$' || c.is_ascii_alphabetic() || c.is_whitespace())
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ',') {
        return Err(Error::Monto);
    }
    let (entero, dec) = match t.rfind(['.', ',']) {
        Some(i) if (1..=2).contains(&(t.len() - i - 1)) => (&t[..i], &t[i + 1..]),
        _ => (t.as_str(), ""),
    };
    let entero: String = entero.chars().filter(|c| c.is_ascii_digit()).collect();
    if entero.is_empty() && dec.is_empty() {
        return Err(Error::Monto);
    }
    let e: u64 = if entero.is_empty() { 0 } else { entero.parse().map_err(|_| Error::Monto)? };
    let d: u64 = match dec.len() {
        0 => 0,
        1 => dec.parse::<u64>().map_err(|_| Error::Monto)? * 10,
        _ => dec.parse().map_err(|_| Error::Monto)?,
    };
    e.checked_mul(100).and_then(|c| c.checked_add(d)).ok_or(Error::Monto)
}

/// Centavos → texto para un campo editable: `200`, `200.5` → `200.50`.
pub fn usd_editable(centavos: u64) -> String {
    if centavos % 100 == 0 {
        (centavos / 100).to_string()
    } else {
        format!("{}.{:02}", centavos / 100, centavos % 100)
    }
}

/// Un monto de la obra para mostrar: dólares o las unidades de antes.
pub fn fmt_monto(moneda: Moneda, n: u64, es: bool) -> String {
    match moneda {
        Moneda::Usd => fmt_usd(n, es),
        Moneda::Unidades => crate::partida::monto(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usd_a_piconero_redondea_al_mas_cercano() {
        // USD 20 a USD 160/XMR = 0,125 XMR exacto.
        assert_eq!(usd_a_piconero(2_000, 16_000), Some(125_000_000_000));
        // USD 1 a USD 3/XMR = 0,333333333333(3) → baja.
        assert_eq!(usd_a_piconero(100, 300), Some(333_333_333_333));
        // USD 2 a USD 3/XMR = 0,666666666666(6) → sube.
        assert_eq!(usd_a_piconero(200, 300), Some(666_666_666_667));
        // Medio piconero exacto sube: 1 centavo a 2e12 centavos/XMR = 0,5 piconero.
        assert_eq!(usd_a_piconero(1, 2_000_000_000_000), Some(1));
        // Menos de medio piconero no alcanza.
        assert_eq!(usd_a_piconero(1, 3_000_000_000_000), None);
        assert_eq!(usd_a_piconero(100, 0), None);
        assert_eq!(usd_a_piconero(0, 100), None);
        // Sin desborde con montos grandes: u64::MAX centavos a 1 centavo/XMR no entra.
        assert_eq!(usd_a_piconero(u64::MAX, 1), None);
    }

    #[test]
    fn precio_fijado_es_coherente_y_paga_porcentajes() {
        let p = PrecioFijado::nuevo(5_000, 15_432, "coingecko", 1).unwrap();
        assert!(p.coherente());
        assert_eq!(p.piconero, usd_a_piconero(5_000, 15_432).unwrap());
        assert_eq!(p.piconero_pct(100), p.piconero);
        assert_eq!(p.piconero_pct(50), p.piconero / 2);
        let mut roto = p.clone();
        roto.piconero += 1;
        assert!(!roto.coherente());
        assert_eq!(PrecioFijado::nuevo(5_000, 0, "x", 1), Err(Error::Precio));
    }

    #[test]
    fn precio_de_api_a_centavos() {
        assert_eq!(precio_a_centavos(154.326), Some(15_433));
        assert_eq!(precio_a_centavos(0.0), None);
        assert_eq!(precio_a_centavos(f64::NAN), None);
        assert_eq!(precio_a_centavos(-3.0), None);
    }

    #[test]
    fn formato_y_lectura_de_dolares() {
        assert_eq!(fmt_usd(150_000, true), "USD 1.500");
        assert_eq!(fmt_usd(150_050, true), "USD 1.500,50");
        assert_eq!(fmt_usd(150_050, false), "USD 1,500.50");
        assert_eq!(fmt_usd(5, false), "USD 0.05");
        assert_eq!(leer_usd("1500"), Ok(150_000));
        assert_eq!(leer_usd("1.500"), Ok(150_000));
        assert_eq!(leer_usd("1,500.50"), Ok(150_050));
        assert_eq!(leer_usd("1.500,5"), Ok(150_050));
        assert_eq!(leer_usd("USD 20"), Ok(2_000));
        assert_eq!(leer_usd("$ 0,99"), Ok(99));
        assert_eq!(leer_usd(""), Err(Error::Monto));
        assert_eq!(leer_usd("diez"), Err(Error::Monto));
        assert_eq!(leer_usd("1-2"), Err(Error::Monto));
        assert_eq!(fmt_monto(Moneda::Unidades, 2_000, true), "2.000");
        assert_eq!(fmt_monto(Moneda::Usd, 2_000, true), "USD 20");
        assert_eq!(usd_editable(20_000), "200");
        assert_eq!(usd_editable(20_050), "200.50");
        assert_eq!(leer_usd(&usd_editable(20_050)), Ok(20_050));
    }

    #[test]
    fn moneda_por_defecto_es_la_vieja() {
        #[derive(Deserialize)]
        struct T {
            #[serde(default)]
            m: Moneda,
        }
        let t: T = serde_json::from_str("{}").unwrap();
        assert_eq!(t.m, Moneda::Unidades);
        assert_eq!(serde_json::to_string(&Moneda::Usd).unwrap(), "\"usd\"");
    }
}
