use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Protocol(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn protocol(msg: impl Into<String>) -> Self {
        Self::Protocol(msg.into())
    }
}
