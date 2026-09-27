//! Reparto al pagar una partida.
//!
//! El pot es el doble de la garantía. El porcentaje es la parte del pago
//! que se queda el contratista. Su garantía vuelve siempre.
//! 100% → todo al contratista. 80% → 1.8 garantías al contratista y 0.2 al mandante.
//!
//! La firma es un acuerdo de los dos shares. El FROST sobre la transacción
//! de la cadena usa esos mismos shares cuando hay inputs.

use crate::error::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reparto {
    pub garantia: u64,
    pub porcentaje: u32,
    pub fee: u64,
    pub al_contratista: u64,
    pub al_mandante: u64,
    pub pot: u64,
}

pub fn repartir(garantia: u64, porcentaje: u32, fee: u64) -> Result<Reparto, Error> {
    if garantia == 0 || porcentaje == 0 || porcentaje > 100 {
        return Err(Error::Porcentaje);
    }
    let pot = garantia.checked_mul(2).ok_or(Error::Monto)?;
    if fee >= pot {
        return Err(Error::Monto);
    }
    let pago = garantia
        .checked_mul(u64::from(porcentaje))
        .ok_or(Error::Monto)?
        / 100;
    let al_contratista = pago.checked_add(garantia).ok_or(Error::Monto)?;
    let disponible = pot - fee;
    if al_contratista > disponible {
        return Err(Error::Monto);
    }
    let al_mandante = disponible - al_contratista;
    Ok(Reparto {
        garantia,
        porcentaje,
        fee,
        al_contratista,
        al_mandante,
        pot,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gasto {
    pub reparto: Reparto,
    share_m: Option<Vec<u8>>,
    share_c: Option<Vec<u8>>,
}

impl Gasto {
    pub fn nuevo(reparto: Reparto) -> Self {
        Self {
            reparto,
            share_m: None,
            share_c: None,
        }
    }

    pub fn poner_share_mandante(&mut self, share: Vec<u8>) {
        if !share.is_empty() {
            self.share_m = Some(share);
        }
    }

    pub fn poner_share_contratista(&mut self, share: Vec<u8>) {
        if !share.is_empty() {
            self.share_c = Some(share);
        }
    }

    pub fn cerrar(&self) -> Result<Reparto, Error> {
        if self.share_m.is_none() || self.share_c.is_none() {
            return Err(Error::FaltaFirma);
        }
        Ok(self.reparto.clone())
    }
}

/// Marca de los dos shares sobre el reparto. No es la transacción de la cadena.
pub fn firmar_reparto(reparto: &Reparto, share_bytes: &[u8]) -> Vec<u8> {
    let mut out = share_bytes.to_vec();
    out.extend(reparto.porcentaje.to_le_bytes());
    out.extend(reparto.al_contratista.to_le_bytes());
    out.extend(reparto.al_mandante.to_le_bytes());
    out.extend(reparto.fee.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cien_se_va_entero_al_contratista() {
        let r = repartir(2_000, 100, 0).unwrap();
        assert_eq!(r.al_contratista, 4_000);
        assert_eq!(r.al_mandante, 0);
    }

    #[test]
    fn ochenta_devuelve_un_quinto_del_pago() {
        let r = repartir(1_000, 80, 0).unwrap();
        assert_eq!(r.al_contratista, 1_800);
        assert_eq!(r.al_mandante, 200);
        assert_eq!(r.al_contratista + r.al_mandante, r.pot);
    }

    #[test]
    fn el_gasto_no_cierra_con_un_solo_share() {
        let r = repartir(2_000, 80, 0).unwrap();
        let mut g = Gasto::nuevo(r.clone());
        g.poner_share_mandante(firmar_reparto(&r, b"share-m"));
        assert_eq!(g.cerrar(), Err(Error::FaltaFirma));
        g.poner_share_contratista(firmar_reparto(&r, b"share-c"));
        let listo = g.cerrar().unwrap();
        assert_eq!(listo.al_contratista, 3_600);
        assert_eq!(listo.al_mandante, 400);
    }
}
