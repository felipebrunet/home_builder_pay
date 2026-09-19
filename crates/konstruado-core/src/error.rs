#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Nombre,
    Monto,
    Garantia,
    NoDivide,
    NoEsta,
    NoToca,
    YaExiste,
    Nota,
    Porcentaje,
    Detalle,
    Desconectado,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Nombre => write!(f, "Poné un nombre."),
            Error::Monto => write!(f, "El trabajo tiene que ser mayor a cero."),
            Error::Garantia => write!(f, "La garantía tiene que ser mayor a cero."),
            Error::NoDivide => write!(
                f,
                "La garantía tiene que caber exactamente en el trabajo (partidas iguales)."
            ),
            Error::NoEsta => write!(f, "No encuentro eso."),
            Error::NoToca => write!(f, "Esto no te toca a vos."),
            Error::YaExiste => write!(f, "Eso ya está hecho."),
            Error::Nota => write!(f, "La nota no puede pasar de 50 caracteres."),
            Error::Porcentaje => write!(f, "El pago va del 1 al 100 por ciento."),
            Error::Detalle => write!(f, "Escribí qué es la partida extra."),
            Error::Desconectado => write!(
                f,
                "El otro no está en línea. Tiene que tener Konstruado abierto."
            ),
        }
    }
}

impl std::error::Error for Error {}
