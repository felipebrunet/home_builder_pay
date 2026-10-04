//! Sealed text for the two parties of one job.
//!
//! X25519 plus XChaCha20-Poly1305. Both sides derive the same box from
//! their own secret and the other party's public key. The swarm stores
//! the box and can see that a note exists.

use base64::Engine;
use crypto_box::aead::generic_array::typenum::Unsigned;
use crypto_box::aead::{Aead, AeadCore};
use crypto_box::{ChaChaBox, PublicKey, SecretKey};

use crate::error::Error;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub fn generar_clave() -> (String, String) {
    let sec = SecretKey::generate(&mut crypto_box::aead::OsRng);
    let pubk = sec.public_key();
    (B64.encode(sec.to_bytes()), B64.encode(pubk.as_bytes()))
}

pub fn pub_de(sec_b64: &str) -> Option<String> {
    let raw = B64.decode(sec_b64).ok()?;
    let sec = SecretKey::from_slice(&raw).ok()?;
    Some(B64.encode(sec.public_key().as_bytes()))
}

/// Keep a matching pair. Generate a new one when the secret is missing
/// or does not match the published public key.
pub fn asegurar_clave(sec: &str, pubk: &str) -> (String, String) {
    if let Some(derived) = pub_de(sec) {
        if pubk.is_empty() || pubk == derived {
            return (sec.to_string(), derived);
        }
    }
    generar_clave()
}

pub fn sellar(mi_sec: &str, otro_pub: &str, aad: &str, plano: &str) -> Result<String, Error> {
    let (sec, otro) = par(mi_sec, otro_pub)?;
    let b = ChaChaBox::new(&otro, &sec);
    let nonce = ChaChaBox::generate_nonce(&mut crypto_box::aead::OsRng);
    // crypto_box 0.9 rejects associated data. The context rides in the
    // authenticated plaintext so a box cannot be moved to another job.
    let mut msg = Vec::with_capacity(aad.len() + 1 + plano.len());
    msg.extend_from_slice(aad.as_bytes());
    msg.push(0);
    msg.extend_from_slice(plano.as_bytes());
    let ct = b
        .encrypt(&nonce, msg.as_slice())
        .map_err(|_| Error::SinClave)?;
    let mut raw = nonce.to_vec();
    raw.extend(ct);
    Ok(B64.encode(raw))
}

pub fn abrir(mi_sec: &str, otro_pub: &str, aad: &str, caja: &str) -> Result<String, Error> {
    let raw = B64.decode(caja).map_err(|_| Error::SinClave)?;
    let nlen = <ChaChaBox as AeadCore>::NonceSize::USIZE;
    if raw.len() < nlen {
        return Err(Error::SinClave);
    }
    let (sec, otro) = par(mi_sec, otro_pub)?;
    let b = ChaChaBox::new(&otro, &sec);
    let nonce = crypto_box::Nonce::from_slice(&raw[..nlen]);
    let pt = b
        .decrypt(nonce, &raw[nlen..])
        .map_err(|_| Error::SinClave)?;
    let pt = String::from_utf8(pt).map_err(|_| Error::SinClave)?;
    let (got, plano) = pt.split_once('\0').ok_or(Error::SinClave)?;
    if got != aad {
        return Err(Error::SinClave);
    }
    Ok(plano.to_string())
}

fn par(mi_sec: &str, otro_pub: &str) -> Result<(SecretKey, PublicKey), Error> {
    let sec_raw = B64.decode(mi_sec).map_err(|_| Error::SinClave)?;
    let pub_raw = B64.decode(otro_pub).map_err(|_| Error::SinClave)?;
    let sec = SecretKey::from_slice(&sec_raw).map_err(|_| Error::SinClave)?;
    let pubk = PublicKey::from_slice(&pub_raw).map_err(|_| Error::SinClave)?;
    Ok((sec, pubk))
}
