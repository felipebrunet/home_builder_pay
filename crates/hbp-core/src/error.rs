use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Protocol(String),
    #[error("invalid hex: {0}")]
    Hex(#[from] hex::FromHexError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("nonce seed was already used; aborting to protect the key")]
    NonceReused,
}

impl Error {
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }
}
