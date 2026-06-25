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
    #[error("disk full: {0}\n  hint: free up disk space before retrying; set COMMONMETA_TEMP_DIR to redirect temp files to a larger volume")]
    DiskFull(String),
}

/// Convert a turso SQLite error into the appropriate [`Error`] variant.
/// Distinguishes disk-full from other SQLite failures so callers receive an
/// actionable message instead of a raw "parse error".
pub(crate) fn sqlite_err(e: turso::Error, context: &str) -> Error {
    if matches!(e, turso::Error::DatabaseFull(_)) {
        Error::DiskFull(context.to_string())
    } else {
        Error::Parse(format!("{}: {}", context, e))
    }
}
