//! Bridge bitcoin 0.32 (secp256k1 0.29) and musig2 (secp256k1 0.31) via serialized bytes.

use bitcoin::secp256k1::{PublicKey as BtcPk, SecretKey as BtcSk};
use musig2::secp256k1::{PublicKey as MusigPk, SecretKey as MusigSk};

use crate::Error;

pub fn to_musig_pk(pk: &BtcPk) -> Result<MusigPk, Error> {
    MusigPk::from_slice(&pk.serialize()).map_err(|e| Error::Musig(e.to_string()))
}

pub fn to_musig_sk(sk: &BtcSk) -> Result<MusigSk, Error> {
    MusigSk::from_byte_array(sk.secret_bytes()).map_err(|e| Error::Musig(e.to_string()))
}

pub fn from_musig_pk(pk: &MusigPk) -> Result<BtcPk, Error> {
    Ok(BtcPk::from_slice(&pk.serialize())?)
}

pub fn parse_btc_pk(hex_str: &str) -> Result<BtcPk, Error> {
    let bytes = hex::decode(hex_str.trim())?;
    Ok(BtcPk::from_slice(&bytes)?)
}

pub fn parse_btc_sk(hex_str: &str) -> Result<BtcSk, Error> {
    let bytes = hex::decode(hex_str.trim())?;
    Ok(BtcSk::from_slice(&bytes)?)
}
