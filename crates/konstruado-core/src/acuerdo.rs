use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Error;
use crate::partida::{ajusta_detalles, limpia_nota, n_partidas, porcentaje};

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

impl Rol {
    pub fn etiqueta(self) -> &'static str {
        match self {
            Rol::Mandante => "mandante",
            Rol::Contratista => "contratista",
        }
    }

    pub fn verbo(self) -> &'static str {
        match self {
            Rol::Mandante => "Pago la obra",
            Rol::Contratista => "La construyo",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoObra {
    Publicada,
    Contra,
    Rechazada,
    Acordada,
    EnMarcha,
    Cerrada,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartidaEstado {
    Pendiente,
    Encerrada,
    EnTrato,
    Pagada,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotaPartida {
    pub autor_id: String,
    pub autor_nombre: String,
    pub porcentaje: u32,
    pub texto: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partida {
    pub detalle: String,
    pub estado: PartidaEstado,
    #[serde(default)]
    pub propuesto: Option<u32>,
    #[serde(default)]
    pub pago: Option<u32>,
    #[serde(default)]
    pub turno: Option<Rol>,
    #[serde(default)]
    pub notas: Vec<NotaPartida>,
}

impl PartidaEstado {
    fn rango(self) -> u8 {
        match self {
            PartidaEstado::Pendiente => 0,
            PartidaEstado::Encerrada => 1,
            PartidaEstado::EnTrato => 2,
            PartidaEstado::Pagada => 3,
        }
    }
}

impl Partida {
    pub fn pendiente(detalle: impl Into<String>) -> Self {
        Self {
            detalle: crate::partida::limpia_detalle(&detalle.into()),
            estado: PartidaEstado::Pendiente,
            propuesto: None,
            pago: None,
            turno: None,
            notas: Vec::new(),
        }
    }

    pub fn fusionar(&mut self, otra: Partida) {
        if self.detalle.is_empty() && !otra.detalle.is_empty() {
            self.detalle = otra.detalle.clone();
        }
        if otra.estado.rango() > self.estado.rango() {
            self.estado = otra.estado;
            self.propuesto = otra.propuesto;
            self.pago = otra.pago;
            self.turno = otra.turno;
            if otra.notas.len() >= self.notas.len() {
                self.notas = otra.notas;
            }
            return;
        }
        if otra.estado == self.estado {
            if otra.notas.len() > self.notas.len() {
                self.notas = otra.notas;
                self.propuesto = otra.propuesto;
                self.turno = otra.turno;
            }
            if self.pago.is_none() {
                self.pago = otra.pago;
            }
        }
        if self.estado == PartidaEstado::Pagada {
            self.turno = None;
        }
    }
}

impl EstadoObra {
    fn rango(self) -> u8 {
        match self {
            EstadoObra::Publicada => 0,
            EstadoObra::Contra => 1,
            EstadoObra::Rechazada => 2,
            EstadoObra::Acordada => 3,
            EstadoObra::EnMarcha => 4,
            EstadoObra::Cerrada => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Oferta {
    pub id: String,
    pub nombre: String,
    pub trabajo: u64,
    pub garantia_sugerida: u64,
    pub n_partidas_sugeridas: u32,
    pub mandante: Persona,
    #[serde(default)]
    pub detalles: Vec<String>,
}

impl Oferta {
    pub fn publicar(
        mandante: Persona,
        nombre: impl Into<String>,
        trabajo: u64,
        garantia_sugerida: u64,
        detalles: Vec<String>,
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
            detalles: ajusta_detalles(n, detalles),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aceptacion {
    pub oferta_id: String,
    pub contratista: Persona,
    pub garantia: u64,
    pub n_partidas: u32,
    #[serde(default)]
    pub detalles: Vec<String>,
}

impl Aceptacion {
    pub fn de(oferta: &Oferta, contratista: Persona, garantia: u64) -> Result<Self, Error> {
        Self::de_con(oferta, contratista, garantia, oferta.detalles.clone())
    }

    pub fn de_con(
        oferta: &Oferta,
        contratista: Persona,
        garantia: u64,
        detalles: Vec<String>,
    ) -> Result<Self, Error> {
        let n = n_partidas(oferta.trabajo, garantia)?;
        Ok(Self {
            oferta_id: oferta.id.clone(),
            contratista,
            garantia,
            n_partidas: n,
            detalles: ajusta_detalles(n, detalles),
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
    /// Bond the mandante published, before any counter.
    #[serde(default)]
    pub garantia_publicada: u64,
    pub n_partidas: u32,
    pub mandante: Persona,
    pub contratista: Persona,
    pub estado: EstadoObra,
    pub partidas: Vec<Partida>,
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
            garantia_publicada: oferta.garantia_sugerida,
            n_partidas: n,
            mandante: oferta.mandante,
            contratista: acc.contratista,
            estado,
            partidas: acc
                .detalles
                .iter()
                .map(|d| Partida::pendiente(d.clone()))
                .collect(),
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
        self.partidas = acc
            .detalles
            .iter()
            .map(|d| Partida::pendiente(d.clone()))
            .collect();
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
        self.estado = EstadoObra::Rechazada;
        let g = if self.garantia_publicada > 0 {
            self.garantia_publicada
        } else {
            self.garantia
        };
        if let Ok(n) = n_partidas(self.trabajo, g) {
            self.garantia = g;
            self.n_partidas = n;
            let dets: Vec<String> = self.partidas.iter().map(|p| p.detalle.clone()).collect();
            self.partidas = ajusta_detalles(n, dets)
                .into_iter()
                .map(Partida::pendiente)
                .collect();
        }
        Ok(())
    }

    /// Both sides lock the same `garantia` for installment `i`. Stub until XMR.
    pub fn encerrar_partida(&mut self, i: usize) -> Result<(), Error> {
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if p.estado != PartidaEstado::Pendiente {
            return Err(Error::YaExiste);
        }
        let _ = xmr_hook();
        p.estado = PartidaEstado::Encerrada;
        self.estado = EstadoObra::EnMarcha;
        Ok(())
    }

    /// Contractor says the installment is done and proposes a pay %.
    pub fn avisar_termino(
        &mut self,
        i: usize,
        quien: &Persona,
        pct: u32,
        nota: impl Into<String>,
    ) -> Result<(), Error> {
        if quien.id != self.contratista.id {
            return Err(Error::NoToca);
        }
        let pct = porcentaje(pct)?;
        let texto = limpia_nota(&nota.into())?;
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if p.estado != PartidaEstado::Encerrada {
            return Err(Error::NoToca);
        }
        p.notas.push(NotaPartida {
            autor_id: quien.id.clone(),
            autor_nombre: quien.nombre.clone(),
            porcentaje: pct,
            texto,
        });
        p.propuesto = Some(pct);
        p.turno = Some(Rol::Mandante);
        p.estado = PartidaEstado::EnTrato;
        Ok(())
    }

    /// The person whose turn it is proposes another %.
    pub fn contra_pago(
        &mut self,
        i: usize,
        quien: &Persona,
        pct: u32,
        nota: impl Into<String>,
    ) -> Result<(), Error> {
        let rol = self.rol_de(&quien.id)?;
        let pct = porcentaje(pct)?;
        let texto = limpia_nota(&nota.into())?;
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if p.estado != PartidaEstado::EnTrato || p.turno != Some(rol) {
            return Err(Error::NoToca);
        }
        if p.propuesto == Some(pct) {
            return Err(Error::YaExiste);
        }
        p.notas.push(NotaPartida {
            autor_id: quien.id.clone(),
            autor_nombre: quien.nombre.clone(),
            porcentaje: pct,
            texto,
        });
        p.propuesto = Some(pct);
        p.turno = Some(match rol {
            Rol::Mandante => Rol::Contratista,
            Rol::Contratista => Rol::Mandante,
        });
        Ok(())
    }

    /// The person whose turn it is accepts the current %.
    pub fn aceptar_pago(&mut self, i: usize, quien: &Persona) -> Result<(), Error> {
        let rol = self.rol_de(&quien.id)?;
        let p = self.partidas.get_mut(i).ok_or(Error::NoEsta)?;
        if p.estado != PartidaEstado::EnTrato || p.turno != Some(rol) {
            return Err(Error::NoToca);
        }
        let pct = p.propuesto.ok_or(Error::NoEsta)?;
        let _ = xmr_hook();
        p.pago = Some(pct);
        p.turno = None;
        p.estado = PartidaEstado::Pagada;
        if self
            .partidas
            .iter()
            .all(|s| s.estado == PartidaEstado::Pagada)
        {
            self.estado = EstadoObra::Cerrada;
        }
        Ok(())
    }

    pub fn activa(&self) -> Option<usize> {
        self.partidas
            .iter()
            .position(|s| s.estado != PartidaEstado::Pagada)
    }

    fn rol_de(&self, id: &str) -> Result<Rol, Error> {
        if self.mandante.id == id {
            Ok(Rol::Mandante)
        } else if self.contratista.id == id {
            Ok(Rol::Contratista)
        } else {
            Err(Error::NoToca)
        }
    }

    /// Gossip must not roll a job backwards. Encerrar/pagar on one
    /// node wins over a stale copy on the other.
    pub fn fusionar(&mut self, mut otra: Obra) {
        if otra.estado.rango() > self.estado.rango() {
            self.estado = otra.estado;
            self.garantia = otra.garantia;
            self.n_partidas = otra.n_partidas;
            if otra.garantia_publicada > 0 {
                self.garantia_publicada = otra.garantia_publicada;
            }
            self.contra = otra.contra.take();
            Self::fusionar_partidas(&mut self.partidas, otra.partidas, self.n_partidas as usize);
        } else if otra.estado.rango() == self.estado.rango() {
            if self.contra.is_none() {
                self.contra = otra.contra.take();
            }
            if otra.n_partidas == self.n_partidas {
                Self::fusionar_partidas(&mut self.partidas, otra.partidas, self.n_partidas as usize);
            }
        } else if otra.n_partidas == self.n_partidas {
            Self::fusionar_partidas(&mut self.partidas, otra.partidas, self.n_partidas as usize);
        }
        if self.estado != EstadoObra::Rechazada
            && self.partidas.iter().any(|p| p.estado.rango() >= PartidaEstado::Encerrada.rango())
            && self.estado.rango() < EstadoObra::EnMarcha.rango()
        {
            self.estado = EstadoObra::EnMarcha;
        }
        if self
            .partidas
            .iter()
            .all(|p| p.estado == PartidaEstado::Pagada)
            && !self.partidas.is_empty()
        {
            self.estado = EstadoObra::Cerrada;
        }
    }

    fn fusionar_partidas(dst: &mut Vec<Partida>, mut src: Vec<Partida>, n: usize) {
        dst.resize(n, Partida::pendiente(String::new()));
        src.resize(n, Partida::pendiente(String::new()));
        for (a, b) in dst.iter_mut().zip(src) {
            a.fusionar(b);
        }
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
