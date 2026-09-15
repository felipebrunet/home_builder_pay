#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Nombre,
    Monto,
    Garantia,
    NoDivide,
    NoEsta,
    NoToca,
    YaExiste,
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
        }
    }
}

impl std::error::Error for Error {}
