//! Passphrase encryption for identity.json (toy: no strength policy).

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};

use crate::Error;

pub const ENC_MARK: &str = "hbp-identity-v1";

const M_COST: u32 = 8 * 1024;
const T_COST: u32 = 2;
const P_COST: u32 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    enc: String,
    kdf: String,
    m: u32,
    t: u32,
    p: u32,
    salt: String,
    nonce: String,
    ct: String,
}

pub fn is_encrypted(raw: &str) -> bool {
    serde_json::from_str::<Envelope>(raw)
        .map(|e| e.enc == ENC_MARK)
        .unwrap_or(false)
}

pub fn encrypt(plaintext: &[u8], passphrase: &str) -> crate::Result<String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::getrandom(&mut salt).map_err(|e| Error::protocol(e.to_string()))?;
    getrandom::getrandom(&mut nonce).map_err(|e| Error::protocol(e.to_string()))?;
    let key = derive(passphrase, &salt, M_COST, T_COST, P_COST)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| Error::protocol("encrypt failed"))?;
    let env = Envelope {
        enc: ENC_MARK.to_string(),
        kdf: "argon2id".to_string(),
        m: M_COST,
        t: T_COST,
        p: P_COST,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
        ct: hex::encode(ct),
    };
    Ok(serde_json::to_string_pretty(&env)?)
}

pub fn decrypt(raw: &str, passphrase: &str) -> crate::Result<Vec<u8>> {
    let env: Envelope = serde_json::from_str(raw)?;
    if env.enc != ENC_MARK {
        return Err(Error::protocol("not an encrypted identity"));
    }
    let salt = hex::decode(env.salt.trim())?;
    let nonce = hex::decode(env.nonce.trim())?;
    let ct = hex::decode(env.ct.trim())?;
    if nonce.len() != NONCE_LEN {
        return Err(Error::protocol("bad nonce"));
    }
    let key = derive(passphrase, &salt, env.m, env.t, env.p)?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    cipher
        .decrypt(XNonce::from_slice(&nonce), ct.as_slice())
        .map_err(|_| Error::protocol("wrong passphrase"))
}

fn derive(passphrase: &str, salt: &[u8], m: u32, t: u32, p: u32) -> crate::Result<[u8; KEY_LEN]> {
    let params = Params::new(m, t, p, Some(KEY_LEN)).map_err(|e| Error::protocol(e.to_string()))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| Error::protocol(e.to_string()))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_short_passphrase() {
        let pt = br#"{"secret_key":"aa"}"#;
        let enc = encrypt(pt, "ab").unwrap();
        assert!(is_encrypted(&enc));
        assert!(!enc.contains("secret_key"));
        assert_eq!(decrypt(&enc, "ab").unwrap(), pt);
        assert!(decrypt(&enc, "zz").is_err());
        let enc4 = encrypt(pt, "abcd").unwrap();
        assert_eq!(decrypt(&enc4, "abcd").unwrap(), pt);
        let nonces = br#"{"used_seed_hashes":[],"pending":{}}"#;
        let encn = encrypt(nonces, "ab").unwrap();
        assert_eq!(decrypt(&encn, "ab").unwrap(), nonces);
    }
}
