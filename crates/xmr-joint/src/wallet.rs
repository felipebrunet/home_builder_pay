//! Billetera personal de 25 palabras. La view sale de la semilla. No se guarda aparte.

use curve25519_dalek::constants::ED25519_BASEPOINT_TABLE;
use monero_seed::{Language, Seed};
use monero_wallet::{
    ed25519::{Point, Scalar},
    OutputWithDecoys, ViewPair,
};
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

use crate::network::Net;
use crate::{Error, Result};

/// Llaves de una persona. La semilla de 25 palabras alcanza para reconstruirlas.
pub struct SingleWallet {
    spend: Zeroizing<Scalar>,
    view_private: Zeroizing<Scalar>,
    view: ViewPair,
    address: String,
    net: Net,
}

impl SingleWallet {
    /// Semilla nueva. Las palabras salen una vez, en [`SeedBackup`] vía el llamador.
    pub fn generate<R: RngCore + CryptoRng>(
        rng: &mut R,
        net: Net,
    ) -> Result<(Self, Zeroizing<String>)> {
        let seed = Seed::new(rng, Language::English);
        let words = seed.to_string();
        let wallet = Self::from_entropy(net, &seed.entropy())?;
        Ok((wallet, words))
    }

    pub fn restore(net: Net, words: &str) -> Result<Self> {
        let seed = Seed::from_string(Language::English, Zeroizing::new(words.to_string()))
            .map_err(|e| Error::Wallet(e.to_string()))?;
        Self::from_entropy(net, &seed.entropy())
    }

    fn from_entropy(net: Net, entropy: &Zeroizing<[u8; 32]>) -> Result<Self> {
        let spend = Zeroizing::new(
            Scalar::read(&mut entropy.as_slice()).map_err(|e| Error::Wallet(e.to_string()))?,
        );
        // Misma derivación que Monero: view = keccak(spend) reducido.
        let view_private = Zeroizing::new(Scalar::hash(entropy.as_slice()));
        let spend_pub = Point::from(&(*spend).into() * ED25519_BASEPOINT_TABLE);
        let view = ViewPair::new(spend_pub, view_private.clone())
            .map_err(|e| Error::Wallet(e.to_string()))?;
        let address = view.legacy_address(net.oxide()).to_string();
        Ok(Self {
            spend,
            view_private,
            view,
            address,
            net,
        })
    }

    pub fn net(&self) -> Net {
        self.net
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    pub fn view_pair(&self) -> ViewPair {
        self.view.clone()
    }

    pub fn spend_key(&self) -> &Zeroizing<Scalar> {
        &self.spend
    }

    /// View privada, por si un respaldo quiere mostrarla. La semilla ya la incluye.
    pub fn view_private_bytes(&self) -> [u8; 32] {
        <[u8; 32]>::from(*self.view_private)
    }
}

impl Drop for SingleWallet {
    fn drop(&mut self) {
        self.address.zeroize();
    }
}

/// Spend de un output: llave de la billetera más el offset de oxide.
pub fn one_time_spend(spend: &Scalar, output: &OutputWithDecoys) -> Zeroizing<Scalar> {
    let combined = (*spend).into() + output.key_offset().into();
    Zeroizing::new(Scalar::from(combined))
}

/// Key image comprimida de 32 bytes.
pub fn key_image_bytes(spend: &Scalar, output: &OutputWithDecoys) -> [u8; 32] {
    let input_key = (*spend).into() + output.key_offset().into();
    let generator = Point::biased_hash(output.key().compress().to_bytes());
    let image = Point::from(&input_key * generator.into());
    image.compress().to_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn la_semilla_regenera_la_misma_direccion() {
        let (wallet, words) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        let again = SingleWallet::restore(Net::Stagenet, &words).unwrap();
        assert_eq!(wallet.address(), again.address());
        assert_eq!(wallet.view_private_bytes(), again.view_private_bytes());
        assert_eq!(
            <[u8; 32]>::from(**wallet.spend_key()),
            <[u8; 32]>::from(**again.spend_key())
        );
        assert_eq!(words.split_whitespace().count(), 25);
    }

    #[test]
    fn dos_semillas_no_comparten_direccion() {
        let (a, _) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        let (b, _) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        assert_ne!(a.address(), b.address());
    }
}
