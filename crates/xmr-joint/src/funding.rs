//! Encierre de una partida: una sola salida, el doble de la garantía,
//! hacia la caja. Los dos tienen que aportar. Si falta uno, no cierra.
//!
//! El CLSAG de cada input se arma cuando hay anillos del daemon. Acá queda
//! el acuerdo y la traba de las dos firmas.

use crate::error::Error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanEncierre {
    pub garantia: u64,
    pub destino: String,
    /// Una sola salida. Los dos aportes van juntos.
    pub salida: u64,
}

pub fn plan_encierre(garantia: u64, destino: &str) -> Result<PlanEncierre, Error> {
    if garantia == 0 || destino.trim().is_empty() {
        return Err(Error::Monto);
    }
    let salida = garantia.checked_mul(2).ok_or(Error::Monto)?;
    Ok(PlanEncierre {
        garantia,
        destino: destino.to_string(),
        salida,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SesionEncierre {
    pub plan: PlanEncierre,
    mando: bool,
    contra: bool,
}

impl SesionEncierre {
    pub fn nueva(plan: PlanEncierre) -> Self {
        Self {
            plan,
            mando: false,
            contra: false,
        }
    }

    pub fn aportar_mandante(&mut self) {
        self.mando = true;
    }

    pub fn aportar_contratista(&mut self) {
        self.contra = true;
    }

    pub fn cerrar(&self) -> Result<PlanEncierre, Error> {
        if !self.mando || !self.contra {
            return Err(Error::FaltaFirma);
        }
        Ok(self.plan.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_salida_y_hacen_falta_los_dos() {
        let plan = plan_encierre(2_000, "5caja").unwrap();
        assert_eq!(plan.salida, 4_000);
        let mut s = SesionEncierre::nueva(plan.clone());
        s.aportar_mandante();
        assert_eq!(s.cerrar(), Err(Error::FaltaFirma));
        s.aportar_contratista();
        assert_eq!(s.cerrar().unwrap(), plan);
    }
}
