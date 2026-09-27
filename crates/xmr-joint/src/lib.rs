//! Monero para Konstruado: hot wallet, caja 2-de-2 y el reparto de una partida.
//!
//! La transacción que entra a la cadena necesita outputs reales (anillos).
//! Estos módulos arman las claves y el acuerdo. No hablan con un daemon.

mod dkg;
mod error;
mod funding;
mod hot;
mod pago;

pub use error::Error;

pub use dkg::{cerrar_dkg, contexto_obra, ronda_compromiso, ronda_shares, RolCaja, ShareLocal};
pub use funding::{plan_encierre, PlanEncierre, SesionEncierre};
pub use hot::{red_laboratorio, HotWallet};
pub use pago::{firmar_reparto, repartir, Gasto, Reparto};

/// Lo llama el dominio al encerrar y al pagar. Sigue siendo 4: la cadena
/// todavía no se emite desde acá.
pub fn monero_fn() -> u32 {
    2 + 2
}