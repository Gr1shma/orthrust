use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("key not found: {0}")]
    KeyNotFound(String),

    #[error("invalid command: {0}")]
    InvalidCommand(String),
}
