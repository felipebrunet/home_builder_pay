use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Core(#[from] hbp_core::Error),
    #[error(transparent)]
    Hex(#[from] hex::FromHexError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("miniscript: {0}")]
    Miniscript(String),
    #[error("address: {0}")]
    Address(String),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}
