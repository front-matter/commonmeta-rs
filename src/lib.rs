//! commonmeta — a Rust port of front-matter/commonmeta.
//!
//! Convert scholarly metadata between formats. The native model is [`Data`];
//! format modules read into it and write out of it.

pub mod crockford;
pub mod data;
pub mod doi_utils;
pub mod error;
mod formats;
pub mod traits;
pub mod utils;
pub mod vocab;

pub use data::Data;
pub use error::{Error, Result};
pub use formats::crossref;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Read from one format and write to another in a single call.
pub fn convert(from: &str, to: &str, input: &str) -> Result<Vec<u8>> {
    let data = formats::read(from, input)?;
    formats::write(to, &data)
}

/// Like `convert`, but passes CSL `style` and `locale` through to the citation writer.
pub fn convert_citation(
    from: &str,
    input: &str,
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<u8>> {
    let data = formats::read(from, input)?;
    formats::write_citation("citation", &data, style, locale)
}

/// Write a list of commonmeta records as a single Parquet file, using a
/// flattened, lossy tabular projection of each record's fields.
pub fn write_parquet(list: &[Data]) -> Result<Vec<u8>> {
    formats::commonmeta::write_parquet_all(list)
}
