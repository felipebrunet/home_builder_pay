use bitcoin::secp256k1::{rand::rngs::OsRng, PublicKey, Secp256k1, SecretKey};
use hbp_core::Network;
use serde::{Deserialize, Serialize};

use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub network: Network,
    pub role: Option<hbp_core::Role>,
    /// 32-byte secret, hex. Toy storage: never use this file for mainnet savings.
    pub secret_key: String,
    /// 33-byte compressed pubkey, hex.
    pub public_key: String,
}

pub fn generate_identity(network: Network) -> Result<Identity, Error> {
    let sk = SecretKey::new(&mut OsRng);
    identity_from_secret(network, &hex::encode(sk.secret_bytes()))
}

/// Restore an identity from the 32-byte secret (64 hex chars).
///
/// This is the backup: one key, not an HD xprv. The shareable half is
/// [`Identity::public_key`] (compressed 33-byte hex), not an xpub.
pub fn identity_from_secret(network: Network, secret_hex: &str) -> Result<Identity, Error> {
    let sk = crate::convert::parse_btc_sk(secret_hex)?;
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, &sk);
    Ok(Identity {
        network,
        role: None,
        secret_key: hex::encode(sk.secret_bytes()),
        public_key: hex::encode(pk.serialize()),
    })
}

impl Identity {
    pub fn secret(&self) -> Result<SecretKey, Error> {
        crate::convert::parse_btc_sk(&self.secret_key)
    }

    pub fn public(&self) -> Result<PublicKey, Error> {
        crate::convert::parse_btc_pk(&self.public_key)
    }
}
