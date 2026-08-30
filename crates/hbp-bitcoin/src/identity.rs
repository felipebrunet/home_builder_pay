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
    let secp = Secp256k1::new();
    let sk = SecretKey::new(&mut OsRng);
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
