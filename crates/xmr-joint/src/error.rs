use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("clave inválida")]
    Clave,
    #[error("la caja todavía no está")]
    Caja,
    #[error("falta la firma del otro")]
    FaltaFirma,
    #[error("monto inválido")]
    Monto,
    #[error("porcentaje inválido")]
    Porcentaje,
    #[error("el protocolo de la caja falló")]
    Protocolo,
}
