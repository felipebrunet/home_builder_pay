//! Hot wallet single-sig. La secreta no sale de esta máquina.

use std::io::{self, Cursor};

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use monero_wallet::address::Network;
use monero_wallet::ed25519::{Point, Scalar};
use monero_wallet::ViewPair;
use rand_core::{CryptoRng, OsRng, RngCore};
use zeroize::Zeroizing;

use crate::Error;

/// Stagenet. Konstruado todavía no habla con mainnet.
pub fn red_laboratorio() -> Network {
    Network::Stagenet
}

pub struct HotWallet {
    spend: Zeroizing<Scalar>,
    view: Zeroizing<Scalar>,
    pub address: String,
}

impl HotWallet {
    pub fn generar(network: Network) -> Self {
        let mut rng = OsRng;
        let spend = Zeroizing::new(Scalar::random(&mut rng));
        let view = Zeroizing::new(view_de_spend(&spend));
        Self::armar(spend, view, network).expect("clave recién generada")
    }

    pub fn desde_spend_hex(spend_hex: &str, network: Network) -> Result<Self, Error> {
        let spend = scalar_de_hex(spend_hex)?;
        let view = Zeroizing::new(view_de_spend(&spend));
        Self::armar(spend, view, network)
    }

    pub fn spend_hex(&self) -> String {
        hex::encode(<[u8; 32]>::from((*self.spend).clone()))
    }

    pub fn view_hex(&self) -> String {
        hex::encode(<[u8; 32]>::from((*self.view).clone()))
    }

    pub fn spend(&self) -> Zeroizing<Scalar> {
        self.spend.clone()
    }

    pub fn par(&self) -> ViewPair {
        let dalek: curve25519_dalek::Scalar = (*self.spend).clone().into();
        let spend_pub = Point::from(ED25519_BASEPOINT_POINT * dalek);
        ViewPair::new(spend_pub, self.view.clone()).expect("par de la hot wallet")
    }

    fn armar(
        spend: Zeroizing<Scalar>,
        view: Zeroizing<Scalar>,
        network: Network,
    ) -> Result<Self, Error> {
        let dalek: curve25519_dalek::Scalar = (*spend).clone().into();
        let spend_pub = Point::from(ED25519_BASEPOINT_POINT * dalek);
        let par = ViewPair::new(spend_pub, view.clone()).map_err(|_| Error::Clave)?;
        let address = par.legacy_address(network).to_string();
        Ok(Self { spend, view, address })
    }
}

fn view_de_spend(spend: &Scalar) -> Scalar {
    let bytes = <[u8; 32]>::from((*spend).clone());
    Scalar::hash(bytes)
}

pub(crate) fn scalar_de_hex(s: &str) -> Result<Zeroizing<Scalar>, Error> {
    let raw = hex::decode(s.trim()).map_err(|_| Error::Clave)?;
    if raw.len() != 32 {
        return Err(Error::Clave);
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&raw);
    let scalar = Scalar::read(&mut Cursor::new(bytes)).map_err(|_| Error::Clave)?;
    Ok(Zeroizing::new(scalar))
}

/// Un scalar de view suelto, para la caja compartida. No deriva de una spend.
pub fn view_compartida<R: RngCore + CryptoRng>(rng: &mut R) -> String {
    hex::encode(<[u8; 32]>::from(Scalar::random(rng)))
}

pub fn direccion_de_spend_y_view(
    spend_pub: Point,
    view_hex: &str,
    network: Network,
) -> Result<String, Error> {
    let view = scalar_de_hex(view_hex)?;
    let par = ViewPair::new(spend_pub, view).map_err(|_| Error::Clave)?;
    Ok(par.legacy_address(network).to_string())
}

pub(crate) fn cursor_de(bytes: &[u8]) -> Cursor<&[u8]> {
    Cursor::new(bytes)
}

pub(crate) fn escribir(f: impl FnOnce(&mut Vec<u8>) -> io::Result<()>) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    f(&mut buf)?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_misma_spend_abre_la_misma_direccion() {
        let w = HotWallet::generar(red_laboratorio());
        let otra = HotWallet::desde_spend_hex(&w.spend_hex(), red_laboratorio()).unwrap();
        assert_eq!(w.address, otra.address);
        assert!(w.address.starts_with('5') || w.address.starts_with('7'));
        assert_ne!(w.spend_hex(), w.view_hex());
    }
}
