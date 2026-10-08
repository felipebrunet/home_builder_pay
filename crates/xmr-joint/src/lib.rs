//! Monero de Konstruado: semilla personal, caja 2-de-2 y fondeo atómico.
//!
//! La ventana habla con [`STAGENET_DAEMON`]. `monero_fn` sigue en 4: el dominio
//! no afirma que haya plata. Stagenet se prueba con `cargo run -p xmr-joint --bin stagenet`.
//! Cada corrida genera secretos nuevos. No se reutilizan los del laboratorio.
//!
//! monero-oxide está en `third_party/monero-oxide` (ver `KONSTRUADO.txt`).

#![forbid(unsafe_code)]

pub mod backup;
pub mod sobre;
pub mod chain;
pub mod coop;
pub mod dkg;
pub mod fund;
pub mod network;
pub mod personal;
pub mod spend;
pub mod wallet;

#[cfg(test)]
pub(crate) mod prueba_tx;

pub use backup::{ShareBackup, SeedBackup};
pub use dkg::{DkgParty, JointAccount, Party, ShareOutcome, ViewAnnounce};
pub use monero_wallet::{OutputWithDecoys, ViewPair};
pub use zeroize::Zeroizing;

/// Llave de gasto de la billetera personal. No sale de la máquina.
pub type LlaveGasto = Zeroizing<monero_wallet::ed25519::Scalar>;
pub use network::{daemon_es_defecto, daemon_es_local, daemon_url, es_host_local, fijar_daemon, host_de_url, url_es_local, validar_daemon_url, Net, FEE_CUSHION, PICONERO, STAGENET_DAEMON};
pub use spend::{SpendProposal, SpendSession, Split};
pub use wallet::SingleWallet;

/// Gancho que el dominio llama al confirmar encierre y al pagar.
///
/// Sigue en 4 hasta que la ventana observe la cadena. No afirma que haya plata.
pub fn monero_fn() -> u32 {
    2 + 2
}

/// Errores de la fachada. El protocolo de fondeo conserva [`coop::Error`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("billetera: {0}")]
    Wallet(String),
    #[error("respaldo: {0}")]
    Backup(String),
    #[error("dkg: {0}")]
    Dkg(String),
    #[error("fondeo: {0}")]
    Fund(#[from] coop::Error),
    #[error("gasto: {0}")]
    Spend(String),
    #[error("cadena: {0}")]
    Chain(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_gancho_del_dominio_sigue_en_cuatro() {
        assert_eq!(monero_fn(), 4);
    }
}
