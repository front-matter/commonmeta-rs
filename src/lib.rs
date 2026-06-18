//! commonmeta — a Rust port of front-matter/commonmeta.
//!
//! Convert scholarly metadata between formats. The native model is [`Data`];
//! format modules read into it and write out of it.

pub mod crockford;
pub mod data;
pub mod doi_utils;
pub mod error;
pub mod file_utils;
pub mod progress;
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

/// Write a list of commonmeta records as a single Parquet file. Alongside a
/// flattened tabular projection of each record's fields (for filtering in
/// tools like DuckDB without parsing JSON), every row also carries a `json`
/// column with the record's complete serialization, so [`read_parquet`]
/// round-trips losslessly.
pub fn write_parquet(list: &[Data]) -> Result<Vec<u8>> {
    formats::commonmeta::write_parquet_all(list)
}

/// Read a list of commonmeta records back from the Parquet schema written by
/// [`write_parquet`]. Lossless: each record is restored from its `json`
/// column, the complete original serialization.
pub fn read_parquet(bytes: &[u8]) -> Result<Vec<Data>> {
    formats::commonmeta::read_parquet_all(bytes)
}

/// Render a list of records to `to` format as a single buffer: a JSON array
/// for object-shaped formats (`commonmeta`, `csl`, `datacite`, `inveniordm`,
/// `schemaorg`, `ror`), or newline-joined output for line/document-shaped
/// formats (e.g. `bibtex`, `ris`, `crossref_xml`).
pub fn write_list(list: &[Data], to: &str) -> Result<Vec<u8>> {
    let bar = progress::count_bar("rendering", list.len() as u64);

    if matches!(to, "commonmeta" | "csl" | "datacite" | "inveniordm" | "schemaorg" | "ror") {
        let mut items: Vec<serde_json::Value> = Vec::with_capacity(list.len());
        for item in list {
            let rendered = write(to, item)?;
            let value: serde_json::Value = serde_json::from_slice(&rendered).map_err(|e| {
                Error::Serialize(format!("failed to parse {} output as JSON: {}", to, e))
            })?;
            items.push(value);
            bar.inc(1);
        }
        bar.finish_and_clear();
        return serde_json::to_vec_pretty(&items).map_err(|e| Error::Serialize(e.to_string()));
    }

    let mut output = String::new();
    for (idx, item) in list.iter().enumerate() {
        let rendered = write(to, item)?;
        if idx > 0 {
            output.push('\n');
        }
        output.push_str(&String::from_utf8_lossy(&rendered));
        bar.inc(1);
    }
    bar.finish_and_clear();
    Ok(output.into_bytes())
}

/// Render `list` to `to` format, split into entries of at most `batch_size`
/// records each — suitable for packing into an archive via
/// [`file_utils::write_zip_archive`]/[`file_utils::write_tar_gz_archive`].
/// `base_name` (e.g. `"out.json"`) names the single entry directly when
/// there's only one batch, or gets a numbered suffix (`"out-00000.json"`,
/// `"out-00001.json"`, ...) when there are several.
pub fn write_archive(
    list: &[Data],
    to: &str,
    base_name: &str,
    batch_size: usize,
) -> Result<Vec<(String, Vec<u8>)>> {
    if list.is_empty() {
        return Err(Error::Serialize("no records to write".to_string()));
    }
    let chunks: Vec<&[Data]> = list.chunks(batch_size.max(1)).collect();
    let multi = chunks.len() > 1;

    let mut entries = Vec::with_capacity(chunks.len());
    for (idx, chunk) in chunks.into_iter().enumerate() {
        let bytes = write_list(chunk, to)?;
        let name = batch_entry_name(base_name, if multi { Some(idx) } else { None });
        entries.push((name, bytes));
    }
    Ok(entries)
}

/// Build the entry name for a batch: `base_name` itself when `idx` is
/// `None`, or `{stem}-{idx:05}.{ext}` for numbered batches.
fn batch_entry_name(base_name: &str, idx: Option<usize>) -> String {
    match idx {
        None => base_name.to_string(),
        Some(i) => {
            let path = std::path::Path::new(base_name);
            let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            if ext.is_empty() {
                format!("{}-{:05}", stem, i)
            } else {
                format!("{}-{:05}.{}", stem, i, ext)
            }
        }
    }
}

/// Read commonmeta records from a VRAIX daily dump SQLite file already on
/// disk at `sqlite_path`, e.g. an already-downloaded `crossref-2026-06-14.sqlite3`.
///
/// `from` ("crossref" or "datacite") picks how every row is parsed — VRAIX
/// dumps are single-source per file, so this isn't read from the data
/// itself. `limit: None` reads every row; `Some(n)` reads `n` rows starting
/// at `offset`.
pub fn read_vraix_sqlite(sqlite_path: &str, from: &str, limit: Option<usize>, offset: usize) -> Result<Vec<Data>> {
    formats::vraix::read_dump(sqlite_path, from, limit, offset)
}

/// Fetch commonmeta records from a VRAIX daily dump for `from` ("crossref"
/// or "datacite") and `date` (YYYY-MM-DD).
///
/// With `input_path`, the local SQLite file at that path is read directly
/// via [`read_vraix_sqlite`] (e.g. an already-downloaded dump); otherwise
/// `{from}-{date}.sqlite3.zst` is downloaded from metadata.vraix.org —
/// cached locally for `cache_ttl` via [`file_utils::download_file_cached`]
/// — and decompressed into a temp file first.
///
/// `limit`/`offset` window the rows read from the dump; `limit: None` reads
/// every row.
pub fn fetch_vraix_dump(
    from: &str,
    date: &str,
    input_path: Option<&str>,
    limit: Option<usize>,
    offset: usize,
    cache_ttl: std::time::Duration,
) -> Result<Vec<Data>> {
    if let Some(path) = input_path {
        return read_vraix_sqlite(path, from, limit, offset);
    }

    let url = format!("https://metadata.vraix.org/{}-{}.sqlite3.zst", from, date);
    let cache_key = format!("{}-{}.sqlite3.zst", from, date);
    let (compressed, _from_cache) = file_utils::download_file_cached(&url, "vraix", &cache_key, cache_ttl)
        .map_err(|e| Error::Http(format!("failed to download '{}': {}", url, e)))?;
    let decompressed = file_utils::unzst_content(&compressed)
        .map_err(|e| Error::Parse(format!("failed to decompress '{}': {}", url, e)))?;

    let tmp_path = std::env::temp_dir()
        .join(format!("commonmeta-vraix-{}-{}-{}.sqlite3", from, date, std::process::id()));
    file_utils::write_file(&tmp_path, &decompressed)
        .map_err(|e| Error::Parse(format!("failed to write temp file '{}': {}", tmp_path.display(), e)))?;

    let result = read_vraix_sqlite(tmp_path.to_str().unwrap(), from, limit, offset);
    std::fs::remove_file(&tmp_path).ok();
    result
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data(id: &str) -> Data {
        Data { id: id.to_string(), type_: "JournalArticle".to_string(), ..Data::default() }
    }

    #[test]
    fn test_write_list_json_array_formats() {
        let list = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        let bytes = write_list(&list, "commonmeta").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_write_list_newline_joined_formats() {
        let list = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        let bytes = write_list(&list, "ris").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        // Two records, newline-joined rather than a JSON array.
        assert_eq!(text.lines().filter(|l| l.starts_with("TY  -")).count(), 2);
    }

    #[test]
    fn test_write_archive_single_batch_uses_base_name() {
        let list = vec![sample_data("https://doi.org/10.1/a")];
        let entries = write_archive(&list, "commonmeta", "out.json", 100_000).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "out.json");
    }

    #[test]
    fn test_write_archive_numbered_batches() {
        let list = vec![
            sample_data("https://doi.org/10.1/a"),
            sample_data("https://doi.org/10.1/b"),
            sample_data("https://doi.org/10.1/c"),
        ];
        let entries = write_archive(&list, "commonmeta", "out.json", 1).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0, "out-00000.json");
        assert_eq!(entries[1].0, "out-00001.json");
        assert_eq!(entries[2].0, "out-00002.json");
    }

    #[test]
    fn test_write_archive_no_extension_base_name() {
        let list = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        let entries = write_archive(&list, "commonmeta", "out", 1).unwrap();
        assert_eq!(entries[0].0, "out-00000");
        assert_eq!(entries[1].0, "out-00001");
    }

    #[test]
    fn test_write_archive_empty_list_errors() {
        assert!(write_archive(&[], "commonmeta", "out.json", 100_000).is_err());
    }

    #[test]
    fn test_fetch_vraix_dump_uses_local_input_path_without_network() {
        let dir = std::env::temp_dir().join("commonmeta_lib_fetch_vraix_dump");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("datacite.sqlite3");
        std::fs::remove_file(&path).ok();

        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT);")
            .unwrap();
        connection
            .execute(
                "INSERT INTO works (pid, source_id, raw_metadata) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "pid-0",
                    1i64,
                    r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#
                ],
            )
            .unwrap();

        let data = fetch_vraix_dump(
            "datacite",
            "2026-06-14",
            Some(path.to_str().unwrap()),
            None,
            0,
            std::time::Duration::from_secs(30 * 24 * 60 * 60),
        )
        .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.5678/b");

        std::fs::remove_dir_all(&dir).ok();
    }
}
