//! Error types for openNetDrive

use thiserror::Error;

/// Result type alias
pub type Result<T> = std::result::Result<T, Error>;

/// Main error type
#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Credential error: {0}")]
    Credential(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Unknown(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error::Unknown(s.to_string())
    }
}
