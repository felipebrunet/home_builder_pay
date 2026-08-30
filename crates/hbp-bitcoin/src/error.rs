use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Core(#[from] hbp_core::Error),
    #[error(transparent)]
    Secp(#[from] bitcoin::secp256k1::Error),
    #[error(transparent)]
    Hex(#[from] hex::FromHexError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("miniscript: {0}")]
    Miniscript(String),
    #[error("taproot: {0}")]
    Taproot(String),
    #[error("musig2: {0}")]
    Musig(String),
    #[error("sighash: {0}")]
    Sighash(String),
    #[error("address: {0}")]
    Address(String),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }
}
