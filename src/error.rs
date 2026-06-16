use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("serialize error: {0}")]
    Serialize(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("invalid identifier: {0}")]
    InvalidId(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("http error: {0}")]
    Http(String),
}
