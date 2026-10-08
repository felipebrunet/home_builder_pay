//! Sobre cifrado del respaldo completo (desde 0.2.8).
//!
//! Formato v1, todo en un archivo binario:
//!
//! ```text
//! 0..8    magia  "KSTRBAK\0"
//! 8       versión del formato (1)
//! 9       KDF (1 = argon2id v1.3)
//! 10..14  memoria en KiB (u32 LE)
//! 14..18  pasadas (u32 LE)
//! 18      hilos (u8)
//! 19..35  sal (16 bytes)
//! 35..59  nonce XChaCha20 (24 bytes)
//! 59..    texto cifrado XChaCha20-Poly1305 (con su tag de 16 bytes)
//! ```
//!
//! La cabecera entera (0..59) va como dato asociado: cambiar un parámetro o la
//! sal hace fallar el tag igual que cambiar el contenido.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};
use zeroize::Zeroizing;

pub const MAGIA: &[u8; 8] = b"KSTRBAK\0";
pub const VERSION: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const CABECERA: usize = 59;
const TAG: usize = 16;

/// Parámetros de argon2id. Los por defecto (64 MiB, 3 pasadas) andan en un
/// teléfono en ~1 s y encarecen bastante probar contraseñas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParamsKdf {
    pub memoria_kib: u32,
    pub pasadas: u32,
    pub hilos: u8,
}

impl Default for ParamsKdf {
    fn default() -> Self {
        Self { memoria_kib: 64 * 1024, pasadas: 3, hilos: 1 }
    }
}

impl ParamsKdf {
    /// Límites al abrir: un archivo no puede pedirnos 4 GiB de RAM.
    fn sanos(&self) -> bool {
        (8 * 1024..=1024 * 1024).contains(&self.memoria_kib)
            && (1..=16).contains(&self.pasadas)
            && (1..=8).contains(&self.hilos)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorSobre {
    /// No empieza con la magia: no es un respaldo completo de Konstruado.
    NoEsRespaldo,
    /// Versión de formato que esta app no conoce (respaldo de una versión más nueva).
    Version(u8),
    /// Cabecera truncada o con parámetros fuera de rango.
    Danado,
    /// La contraseña no abre el sobre (o el contenido fue alterado).
    Clave,
    /// Contraseña vacía o algo falló al derivar la clave.
    Interno(String),
}

impl std::fmt::Display for ErrorSobre {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorSobre::NoEsRespaldo => write!(f, "codigo:respaldo-no-es"),
            ErrorSobre::Version(v) => write!(f, "codigo:respaldo-version:{v}"),
            ErrorSobre::Danado => write!(f, "codigo:respaldo-danado"),
            ErrorSobre::Clave => write!(f, "codigo:respaldo-clave"),
            ErrorSobre::Interno(e) => write!(f, "{e}"),
        }
    }
}

fn derivar(clave: &str, sal: &[u8], p: ParamsKdf) -> Result<Zeroizing<[u8; 32]>, ErrorSobre> {
    if clave.is_empty() {
        return Err(ErrorSobre::Interno("codigo:respaldo-clave-vacia".into()));
    }
    let params = Params::new(p.memoria_kib, p.pasadas, p.hilos as u32, Some(32))
        .map_err(|e| ErrorSobre::Interno(e.to_string()))?;
    let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut k = Zeroizing::new([0u8; 32]);
    a.hash_password_into(clave.as_bytes(), sal, k.as_mut())
        .map_err(|e| ErrorSobre::Interno(e.to_string()))?;
    Ok(k)
}

pub fn sellar(plano: &[u8], clave: &str) -> Result<Vec<u8>, ErrorSobre> {
    sellar_con(plano, clave, ParamsKdf::default())
}

pub fn sellar_con(plano: &[u8], clave: &str, p: ParamsKdf) -> Result<Vec<u8>, ErrorSobre> {
    if !p.sanos() {
        return Err(ErrorSobre::Danado);
    }
    let mut sal = [0u8; 16];
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut sal);
    OsRng.fill_bytes(&mut nonce);
    let mut out = Vec::with_capacity(CABECERA + plano.len() + TAG);
    out.extend_from_slice(MAGIA);
    out.push(VERSION);
    out.push(KDF_ARGON2ID);
    out.extend_from_slice(&p.memoria_kib.to_le_bytes());
    out.extend_from_slice(&p.pasadas.to_le_bytes());
    out.push(p.hilos);
    out.extend_from_slice(&sal);
    out.extend_from_slice(&nonce);
    debug_assert_eq!(out.len(), CABECERA);
    let k = derivar(clave, &sal, p)?;
    let cifra = XChaCha20Poly1305::new(k.as_ref().into());
    let ct = cifra
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plano, aad: &out[..CABECERA] })
        .map_err(|_| ErrorSobre::Interno("no pude cifrar".into()))?;
    out.extend_from_slice(&ct);
    Ok(out)
}

/// ¿Empieza con la magia del respaldo completo?
pub fn es_sobre(datos: &[u8]) -> bool {
    datos.len() >= MAGIA.len() && &datos[..MAGIA.len()] == MAGIA
}

/// Lee la cabecera sin derivar la clave (para avisar formato/versión antes de pedir contraseña).
pub fn revisar_cabecera(datos: &[u8]) -> Result<ParamsKdf, ErrorSobre> {
    if !es_sobre(datos) {
        return Err(ErrorSobre::NoEsRespaldo);
    }
    if datos.len() < 9 {
        return Err(ErrorSobre::Danado);
    }
    if datos[8] != VERSION {
        return Err(ErrorSobre::Version(datos[8]));
    }
    if datos.len() < CABECERA + TAG || datos[9] != KDF_ARGON2ID {
        return Err(ErrorSobre::Danado);
    }
    let p = ParamsKdf {
        memoria_kib: u32::from_le_bytes(datos[10..14].try_into().unwrap()),
        pasadas: u32::from_le_bytes(datos[14..18].try_into().unwrap()),
        hilos: datos[18],
    };
    if !p.sanos() {
        return Err(ErrorSobre::Danado);
    }
    Ok(p)
}

pub fn abrir(datos: &[u8], clave: &str) -> Result<Zeroizing<Vec<u8>>, ErrorSobre> {
    let p = revisar_cabecera(datos)?;
    let sal = &datos[19..35];
    let nonce = &datos[35..59];
    let k = derivar(clave, sal, p)?;
    let cifra = XChaCha20Poly1305::new(k.as_ref().into());
    cifra
        .decrypt(XNonce::from_slice(nonce), Payload { msg: &datos[CABECERA..], aad: &datos[..CABECERA] })
        .map(Zeroizing::new)
        .map_err(|_| ErrorSobre::Clave)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAPIDO: ParamsKdf = ParamsKdf { memoria_kib: 8 * 1024, pasadas: 1, hilos: 1 };

    #[test]
    fn ida_y_vuelta() {
        let s = sellar_con(b"hola caja", "clave larga 123", RAPIDO).unwrap();
        assert!(es_sobre(&s));
        assert_eq!(&abrir(&s, "clave larga 123").unwrap()[..], b"hola caja");
        // Dos sellados del mismo texto no se parecen (sal y nonce nuevos).
        let s2 = sellar_con(b"hola caja", "clave larga 123", RAPIDO).unwrap();
        assert_ne!(s, s2);
    }

    #[test]
    fn clave_equivocada_y_archivo_tocado_fallan() {
        let s = sellar_con(b"secreto", "correcta", RAPIDO).unwrap();
        assert_eq!(abrir(&s, "otra").unwrap_err(), ErrorSobre::Clave);
        let mut t = s.clone();
        let n = t.len() - 1;
        t[n] ^= 1;
        assert_eq!(abrir(&t, "correcta").unwrap_err(), ErrorSobre::Clave);
        // La cabecera también está autenticada: cambiar la sal rompe el tag.
        let mut t = s.clone();
        t[20] ^= 1;
        assert_eq!(abrir(&t, "correcta").unwrap_err(), ErrorSobre::Clave);
        assert_eq!(abrir(&s[..40], "correcta").unwrap_err(), ErrorSobre::Danado);
        assert_eq!(abrir(b"konstruado single-sig v1\n", "x").unwrap_err(), ErrorSobre::NoEsRespaldo);
        let mut v = s.clone();
        v[8] = 9;
        assert_eq!(abrir(&v, "correcta").unwrap_err(), ErrorSobre::Version(9));
        // Parámetros absurdos no se intentan.
        let mut m = s.clone();
        m[10..14].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(abrir(&m, "correcta").unwrap_err(), ErrorSobre::Danado);
        assert!(sellar_con(b"x", "", RAPIDO).is_err());
    }

    #[test]
    fn parametros_por_defecto_son_los_del_formato() {
        let s = sellar(b"x", "clave").unwrap();
        let p = revisar_cabecera(&s).unwrap();
        assert_eq!(p, ParamsKdf::default());
        assert_eq!(&abrir(&s, "clave").unwrap()[..], b"x");
    }
}
