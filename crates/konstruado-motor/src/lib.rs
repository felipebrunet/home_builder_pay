//! Motor compartido de Konstruado.
//!
//! Lo usan tal cual el escritorio (`crates/konstruado`, Dioxus) y Android
//! (`crates/konstruado-ffi`, UniFFI): DKG y fondeo de la caja 2-de-2, gasto
//! FROST, scan, envío personal, respaldo completo, persistencia y textos ES/EN.
//! Cada interfaz usa una parte distinta de la API; por eso vive en una lib.

pub mod caja;
pub mod cotizacion;
pub mod i18n;
pub mod persist;
pub mod respaldo;
