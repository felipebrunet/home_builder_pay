//! BIP340 signatures over the canonical contract JSON (tagged `hbp-contract`).

use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey, XOnlyPublicKey};
use hbp_core::{canonical_json, sha256_bytes, ContractBody};
use sha2::{Digest, Sha256};

use crate::convert::parse_btc_pk;
use crate::Error;

fn tagged_hash(tag: &[u8], msg: &[u8]) -> [u8; 32] {
    let tag_hash = sha256_bytes(tag);
    let mut h = Sha256::new();
    h.update(tag_hash);
    h.update(tag_hash);
    h.update(msg);
    h.finalize().into()
}

fn message(body: &ContractBody) -> Result<Message, Error> {
    let bytes = canonical_json(body)?;
    Ok(Message::from_digest(tagged_hash(b"hbp-contract", &bytes)))
}

pub fn sign_body(secret: &SecretKey, body: &ContractBody) -> Result<String, Error> {
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, secret);
    let sig = secp.sign_schnorr_no_aux_rand(&message(body)?, &keypair);
    Ok(hex::encode(sig.as_ref()))
}

pub fn verify_body(pubkey_hex: &str, sig_hex: &str, body: &ContractBody) -> Result<(), Error> {
    let secp = Secp256k1::verification_only();
    let pk = parse_btc_pk(pubkey_hex)?;
    let (xonly, _): (XOnlyPublicKey, _) = pk.x_only_public_key();
    let bytes = hex::decode(sig_hex.trim())?;
    let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(&bytes)?;
    secp.verify_schnorr(&sig, &message(body)?, &xonly)?;
    Ok(())
}
