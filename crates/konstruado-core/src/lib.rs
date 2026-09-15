//! Shared application logic. No UI, no Tor, no Monero.

mod acuerdo;
mod error;
mod partida;

pub use acuerdo::{Aceptacion, EstadoObra, Oferta, Obra, PartidaEstado, Persona, Rol};
pub use error::Error;
pub use partida::{capital_por_lado, n_partidas, monto};

#[cfg(test)]
mod tests;
