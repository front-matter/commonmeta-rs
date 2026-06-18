//! commonmeta — a Rust port of front-matter/commonmeta.
//!
//! Convert scholarly metadata between formats. The native model is [`Data`];
//! format modules read into it and write out of it.

pub mod crockford;
pub mod data;
pub mod doi_utils;
pub mod error;
pub mod schema_utils;
mod formats;
pub mod traits;
pub mod utils;
pub mod vocab;

pub use data::Data;
pub use error::{Error, Result};
pub use formats::crossref;
pub use formats::inveniordm::PushResult;
pub use formats::ror::AffiliationMatch;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Read a single record from `from` format, without writing it back out.
pub fn read(from: &str, input: &str) -> Result<Data> {
    formats::read(from, input)
}

/// Read from one format and write to another in a single call.
pub fn convert(from: &str, to: &str, input: &str) -> Result<Vec<u8>> {
    let data = formats::read(from, input)?;
    formats::write(to, &data)
}

/// Write an already-loaded record to `to` format.
pub fn write(to: &str, data: &Data) -> Result<Vec<u8>> {
    formats::write(to, data)
}

/// Write a ROR-derived record as raw ROR-shaped JSON (as opposed to
/// `write("ror", data)`, which produces InvenioRDM vocabulary YAML).
pub fn write_ror_json(data: &Data) -> Result<Vec<u8>> {
    formats::ror::write_json(data)
}

/// Match a free-text affiliation string against ROR organizations using the
/// ROR v2 affiliation endpoint.
pub fn match_ror_affiliation(affiliation: &str) -> Result<Vec<AffiliationMatch>> {
    formats::ror::match_affiliation(affiliation)
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

/// Read a list of commonmeta records back from the flattened Parquet schema
/// written by [`write_parquet`]. Lossy: only the fields captured by the
/// flattened projection (e.g. first author, first title) are restored.
pub fn read_parquet(bytes: &[u8]) -> Result<Vec<Data>> {
    formats::commonmeta::read_parquet_all(bytes)
}

/// Create-or-update, then publish, a list of records in InvenioRDM.
///
/// This performs real, network-visible writes against `host` (a live record
/// is created/updated and published) using `token` for Bearer authentication.
/// Registration with other services (Crossref, DataCite) is not yet supported.
pub fn push_inveniordm(list: &[Data], host: &str, token: &str) -> Vec<PushResult> {
    formats::inveniordm::upsert_all(list, host, token)
}

/// Create-or-update, then publish, a single record in InvenioRDM.
///
/// This performs a real, network-visible write against `host` (a live record
/// is created/updated and published) using `token` for Bearer authentication.
/// Registration with other services (Crossref, DataCite) is not yet supported.
pub fn put_inveniordm(data: &Data, host: &str, token: &str) -> PushResult {
    formats::inveniordm::upsert(data, host, token)
}
