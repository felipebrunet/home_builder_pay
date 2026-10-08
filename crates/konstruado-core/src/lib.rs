//! Shared application logic. No UI, no Tor, no Monero.

mod acuerdo;
mod caja;
mod error;
mod partida;
mod precio;
mod retiro;

pub use acuerdo::{
    oferta_en_tablero, Aceptacion, EstadoObra, ExtraPartida, NotaPartida, Oferta, Obra, Partida,
    PartidaEstado, Persona, ReciboPartida, Rol, TextoLeido,
};
pub use caja::{asegurar_clave, generar_clave};
pub use error::Error;
pub use precio::{
    fmt_monto, fmt_usd, leer_usd, usd_editable, precio_a_centavos, unidades_a_piconero, usd_a_piconero, Moneda,
    PrecioFijado, PICONERO, PICONERO_POR_UNIDAD,
};
pub use retiro::{retirar_oferta, sin_retiradas, RetiroOferta};
pub use partida::{
    ahora, ajusta_detalles, capital_por_lado, monto, monto_pct, n_partidas, titulo_partida, MAX_NOTA,
};

#[cfg(test)]
mod tests;
