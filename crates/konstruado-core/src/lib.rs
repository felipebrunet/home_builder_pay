//! Shared application logic. No UI, no Tor, no Monero.

mod acuerdo;
mod caja;
mod error;
mod partida;

pub use acuerdo::{
    oferta_en_tablero, Aceptacion, EstadoObra, ExtraPartida, NotaPartida, Oferta, Obra, Partida,
    PartidaEstado, Persona, ReciboPartida, Rol, TextoLeido,
};
pub use caja::{asegurar_clave, generar_clave};
pub use error::Error;
pub use partida::{
    ahora, ajusta_detalles, capital_por_lado, monto, monto_pct, n_partidas, titulo_partida, MAX_NOTA,
};

#[cfg(test)]
mod tests;
