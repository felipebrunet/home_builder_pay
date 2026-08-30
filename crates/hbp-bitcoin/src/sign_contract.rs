//! BIP340 signatures over protocol JSON (tagged `hbp-contract`, `hbp-arbiter`, `hbp-quote`).

use bitcoin::secp256k1::{Keypair, Message, Secp256k1, SecretKey, XOnlyPublicKey};
use hbp_core::{canonical_json, sha256_bytes, ContractBody, Quote};
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

fn arbiter_message(contract_id: &str, arbiter_pubkey: &str) -> Message {
    let payload = format!("{contract_id}:{arbiter_pubkey}");
    Message::from_digest(tagged_hash(b"hbp-arbiter", payload.as_bytes()))
}

pub fn sign_arbiter(
    secret: &SecretKey,
    contract_id: &str,
    arbiter_pubkey: &str,
) -> Result<String, Error> {
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, secret);
    let sig =
        secp.sign_schnorr_no_aux_rand(&arbiter_message(contract_id, arbiter_pubkey), &keypair);
    Ok(hex::encode(sig.as_ref()))
}

pub fn verify_arbiter(
    party_pubkey_hex: &str,
    sig_hex: &str,
    contract_id: &str,
    arbiter_pubkey: &str,
) -> Result<(), Error> {
    let secp = Secp256k1::verification_only();
    let pk = parse_btc_pk(party_pubkey_hex)?;
    let (xonly, _) = pk.x_only_public_key();
    let bytes = hex::decode(sig_hex.trim())?;
    let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(&bytes)?;
    secp.verify_schnorr(&sig, &arbiter_message(contract_id, arbiter_pubkey), &xonly)?;
    Ok(())
}

fn quote_message(quote: &Quote) -> Result<Message, Error> {
    let mut unsigned = quote.clone();
    unsigned.mandante_sig = None;
    unsigned.contratista_sig = None;
    Ok(Message::from_digest(tagged_hash(
        b"hbp-quote",
        &canonical_json(&unsigned)?,
    )))
}

pub fn sign_quote(secret: &SecretKey, quote: &Quote) -> Result<String, Error> {
    let secp = Secp256k1::new();
    let keypair = Keypair::from_secret_key(&secp, secret);
    let sig = secp.sign_schnorr_no_aux_rand(&quote_message(quote)?, &keypair);
    Ok(hex::encode(sig.as_ref()))
}

pub fn verify_quote(pubkey_hex: &str, sig_hex: &str, quote: &Quote) -> Result<(), Error> {
    let secp = Secp256k1::verification_only();
    let pk = parse_btc_pk(pubkey_hex)?;
    let (xonly, _) = pk.x_only_public_key();
    let bytes = hex::decode(sig_hex.trim())?;
    let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(&bytes)?;
    secp.verify_schnorr(&sig, &quote_message(quote)?, &xonly)?;
    Ok(())
}
