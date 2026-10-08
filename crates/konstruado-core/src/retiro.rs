//! Retiro de una oferta publicada que nadie tomó.
//!
//! El tablero se replica por gossip como una unión: si el mandante solo borra
//! la oferta de su almacén, el próximo anuncio de cualquier par se la vuelve a
//! traer. El retiro es una lápida que viaja por la red y gana a la oferta.
//!
//! Solo el autor puede retirar. Al publicar, la oferta lleva `retiro_hash` =
//! SHA-256 de un secreto derivado de la clave X25519 local y del id de la
//! oferta. El retiro revela ese secreto (`prueba`), así que un tercero no puede
//! fabricar una lápida para una oferta ajena. Las ofertas viejas sin
//! `retiro_hash` solo aceptan un retiro firmado con el id del mandante.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::acuerdo::{oferta_en_tablero, Obra, Oferta};
use crate::error::Error;
use crate::partida::ahora;

const DOMINIO: &[u8] = b"konstruado-retiro-v1";

/// Lápida de una oferta. Viaja por el DHT y queda en `estado.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetiroOferta {
    pub oferta_id: String,
    pub autor_id: String,
    #[serde(default)]
    pub cuando: i64,
    /// Secreto en hex cuyo SHA-256 es el `retiro_hash` de la oferta.
    #[serde(default)]
    pub prueba: String,
}

fn secreto(clave_sec: &str, oferta_id: &str) -> Option<[u8; 32]> {
    if clave_sec.trim().is_empty() {
        return None;
    }
    let mut h = Sha256::new();
    h.update(DOMINIO);
    h.update(clave_sec.as_bytes());
    h.update([0u8]);
    h.update(oferta_id.as_bytes());
    Some(h.finalize().into())
}

fn hash_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

impl Oferta {
    /// Compromete el secreto de retiro antes de publicar. Sin clave no hace nada:
    /// la oferta queda como las viejas (retiro por id del mandante).
    pub fn sellar_retiro(&mut self, clave_sec: &str) {
        if let Some(s) = secreto(clave_sec, &self.id) {
            self.retiro_hash = hash_hex(&s);
        }
    }

    /// Copia con más garantías: entre dos copias del mismo id, la que trae
    /// `retiro_hash` gana a la que lo perdió en un par viejo.
    pub fn mejor_que(&self, otra: &Oferta) -> bool {
        !self.retiro_hash.is_empty() && otra.retiro_hash.is_empty()
    }
}

impl RetiroOferta {
    /// True si esta lápida vale para `o`: mismo id, mismo autor y, si la oferta
    /// lo exige, el secreto correcto.
    pub fn vale_para(&self, o: &Oferta) -> bool {
        if self.oferta_id != o.id || self.autor_id != o.mandante.id {
            return false;
        }
        if o.retiro_hash.is_empty() {
            return true;
        }
        match unhex(&self.prueba) {
            Some(raw) => hash_hex(&raw) == o.retiro_hash,
            None => false,
        }
    }
}

/// Arma el retiro de una oferta propia que ningún contratista tomó.
/// `obras` tienen que ser todas las del almacén (incluidas las archivadas).
pub fn retirar_oferta(
    oferta: &Oferta,
    yo_id: &str,
    clave_sec: &str,
    obras: &[Obra],
) -> Result<RetiroOferta, Error> {
    if oferta.mandante.id != yo_id {
        return Err(Error::NoEsTuya);
    }
    if !oferta_en_tablero(&oferta.id, obras) {
        return Err(Error::OfertaTomada);
    }
    let prueba = secreto(clave_sec, &oferta.id)
        .map(|s| hex(&s))
        .unwrap_or_default();
    let r = RetiroOferta {
        oferta_id: oferta.id.clone(),
        autor_id: yo_id.to_string(),
        cuando: ahora(),
        prueba,
    };
    if !r.vale_para(oferta) {
        // La clave cambió desde que se publicó: el resto de la red no la va a
        // aceptar. Se informa como "no es tuya" para no fingir que salió.
        return Err(Error::NoEsTuya);
    }
    Ok(r)
}

/// Saca del tablero las ofertas con una lápida válida.
pub fn sin_retiradas(ofertas: Vec<Oferta>, retiros: &[RetiroOferta]) -> Vec<Oferta> {
    ofertas
        .into_iter()
        .filter(|o| !retiros.iter().any(|r| r.vale_para(o)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{generar_clave, Aceptacion, Persona};

    fn oferta_de(m: &Persona, sec: &str) -> Oferta {
        let mut o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec![]).unwrap();
        o.sellar_retiro(sec);
        o
    }

    #[test]
    fn el_autor_retira_y_la_lapida_vale() {
        let m = Persona::nueva("Felipe").unwrap();
        let (sec, _) = generar_clave();
        let o = oferta_de(&m, &sec);
        assert!(!o.retiro_hash.is_empty());
        let r = retirar_oferta(&o, &m.id, &sec, &[]).unwrap();
        assert!(r.vale_para(&o));
        assert!(sin_retiradas(vec![o], &[r]).is_empty());
    }

    #[test]
    fn otro_no_puede_retirar() {
        let m = Persona::nueva("Felipe").unwrap();
        let intruso = Persona::nueva("Intruso").unwrap();
        let (sec, _) = generar_clave();
        let (sec_i, _) = generar_clave();
        let o = oferta_de(&m, &sec);
        assert_eq!(retirar_oferta(&o, &intruso.id, &sec_i, &[]), Err(Error::NoEsTuya));
        // Lápida fabricada con el id del mandante pero sin el secreto.
        let falsa = RetiroOferta {
            oferta_id: o.id.clone(),
            autor_id: m.id.clone(),
            cuando: 1,
            prueba: hex(&secreto(&sec_i, &o.id).unwrap()),
        };
        assert!(!falsa.vale_para(&o));
        let vacia = RetiroOferta { prueba: String::new(), ..falsa };
        assert!(!vacia.vale_para(&o));
        assert_eq!(sin_retiradas(vec![o.clone()], &[vacia]).len(), 1);
    }

    #[test]
    fn no_se_retira_si_ya_hay_obra() {
        let m = Persona::nueva("Felipe").unwrap();
        let c = Persona::nueva("Caco").unwrap();
        let (sec, _) = generar_clave();
        let o = oferta_de(&m, &sec);
        let a = Aceptacion::de(&o, c, 2_000).unwrap();
        let obra = Obra::desde_oferta(o.clone(), a).unwrap();
        assert_eq!(retirar_oferta(&o, &m.id, &sec, &[obra]), Err(Error::OfertaTomada));
    }

    #[test]
    fn oferta_vieja_sin_hash_acepta_retiro_del_mandante() {
        let m = Persona::nueva("Felipe").unwrap();
        let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec![]).unwrap();
        assert!(o.retiro_hash.is_empty());
        let r = retirar_oferta(&o, &m.id, "", &[]).unwrap();
        assert!(r.vale_para(&o));
        let otro = RetiroOferta { autor_id: "x".into(), ..r };
        assert!(!otro.vale_para(&o));
    }

    #[test]
    fn la_oferta_sin_campo_nuevo_se_lee() {
        let m = Persona::nueva("Felipe").unwrap();
        let o = Oferta::publicar(m, "Casa", 10_000, 2_000, vec![]).unwrap();
        let raw = serde_json::to_string(&o).unwrap();
        assert!(!raw.contains("retiro_hash"));
        let back: Oferta = serde_json::from_str(&raw).unwrap();
        assert_eq!(back, o);
    }
}
