use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Error;
use crate::partida::n_partidas;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Persona {
    pub id: String,
    pub nombre: String,
}

impl Persona {
    pub fn nueva(nombre: impl Into<String>) -> Result<Self, Error> {
        let nombre = limpia_nombre(&nombre.into()).ok_or(Error::Nombre)?;
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            nombre,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rol {
    Mandante,
    Contratista,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoObra {
    Publicada,
    Contra,
    Acordada,
    EnMarcha,
    Cerrada,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartidaEstado {
    Pendiente,
    Encerrada,
    Pagada,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Oferta {
    pub id: String,
    pub nombre: String,
    pub trabajo: u64,
    pub garantia_sugerida: u64,
    pub n_partidas_sugeridas: u32,
    pub mandante: Persona,
}

impl Oferta {
    pub fn publicar(
        mandante: Persona,
        nombre: impl Into<String>,
        trabajo: u64,
        garantia_sugerida: u64,
    ) -> Result<Self, Error> {
        let nombre = limpia_nombre(&nombre.into()).ok_or(Error::Nombre)?;
        let n = n_partidas(trabajo, garantia_sugerida)?;
        Ok(Self {
            id: Uuid::new_v4().to_string(),
            nombre,
            trabajo,
            garantia_sugerida,
            n_partidas_sugeridas: n,
            mandante,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aceptacion {
    pub oferta_id: String,
    pub contratista: Persona,
    pub garantia: u64,
    pub n_partidas: u32,
}

impl Aceptacion {
    pub fn de(oferta: &Oferta, contratista: Persona, garantia: u64) -> Result<Self, Error> {
        let n = n_partidas(oferta.trabajo, garantia)?;
        Ok(Self {
            oferta_id: oferta.id.clone(),
            contratista,
            garantia,
            n_partidas: n,
        })
    }

    pub fn es_contra(&self, oferta: &Oferta) -> bool {
        self.garantia != oferta.garantia_sugerida
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obra {
    pub id: String,
    pub nombre: String,
    pub trabajo: u64,
    pub garantia: u64,
    pub n_partidas: u32,
    pub mandante: Persona,
    pub contratista: Persona,
    pub estado: EstadoObra,
    pub partidas: Vec<PartidaEstado>,
    /// Contractor proposed a different bond; waiting on principal.
    pub contra: Option<Aceptacion>,
}

impl Obra {
    pub fn desde_oferta(oferta: Oferta, acc: Aceptacion) -> Result<Self, Error> {
        if acc.oferta_id != oferta.id {
            return Err(Error::NoEsta);
        }
        let n = n_partidas(oferta.trabajo, acc.garantia)?;
        let contra = if acc.es_contra(&oferta) {
            Some(acc.clone())
        } else {
            None
        };
        let estado = if contra.is_some() {
            EstadoObra::Contra
        } else {
            EstadoObra::Acordada
        };
        Ok(Self {
            id: oferta.id,
            nombre: oferta.nombre,
            trabajo: oferta.trabajo,
            garantia: acc.garantia,
            n_partidas: n,
            mandante: oferta.mandante,
            contratista: acc.contratista,
            estado,
            partidas: vec![PartidaEstado::Pendiente; n as usize],
            contra,
        })
    }

    pub fn confirmar_contra(&mut self, mandante_id: &str) -> Result<(), Error> {
        if self.mandante.id != mandante_id {
            return Err(Error::NoToca);
        }
        let acc = self.contra.take().ok_or(Error::NoEsta)?;
        self.garantia = acc.garantia;
        self.n_partidas = acc.n_partidas;
        self.partidas = vec![PartidaEstado::Pendiente; acc.n_partidas as usize];
        self.estado = EstadoObra::Acordada;
        Ok(())
    }

    pub fn rechazar_contra(&mut self, mandante_id: &str) -> Result<(), Error> {
        if self.mandante.id != mandante_id {
            return Err(Error::NoToca);
        }
        if self.contra.take().is_none() {
            return Err(Error::NoEsta);
        }
        Ok(())
    }

    /// Both sides lock the same `garantia` for installment `i`. Stub until XMR.
    pub fn encerrar_partida(&mut self, i: usize) -> Result<(), Error> {
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if *p != PartidaEstado::Pendiente {
            return Err(Error::YaExiste);
        }
        let _ = xmr_hook();
        *p = PartidaEstado::Encerrada;
        self.estado = EstadoObra::EnMarcha;
        Ok(())
    }

    pub fn pagar_partida(&mut self, i: usize) -> Result<(), Error> {
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if *p != PartidaEstado::Encerrada {
            return Err(Error::NoToca);
        }
        let _ = xmr_hook();
        *p = PartidaEstado::Pagada;
        if self.partidas.iter().all(|s| *s == PartidaEstado::Pagada) {
            self.estado = EstadoObra::Cerrada;
        }
        Ok(())
    }

    pub fn activa(&self) -> Option<usize> {
        self.partidas.iter().position(|s| *s != PartidaEstado::Pagada)
    }
}

fn xmr_hook() -> u32 {
    xmr_joint::monero_fn()
}

fn limpia_nombre(s: &str) -> Option<String> {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() || t.len() > 80 {
        None
    } else {
        Some(t)
    }
}
