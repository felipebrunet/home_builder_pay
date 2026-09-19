//! Shared application logic. No UI, no Tor, no Monero.

mod acuerdo;
mod error;
mod partida;

pub use acuerdo::{
    Aceptacion, EstadoObra, NotaPartida, Oferta, Obra, Partida, PartidaEstado, Persona, ReciboPartida,
    Rol,
};
pub use error::Error;
pub use partida::{
    ahora, ajusta_detalles, capital_por_lado, monto, monto_pct, n_partidas, titulo_partida, MAX_NOTA,
};

#[cfg(test)]
mod tests;
