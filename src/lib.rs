//! commonmeta — a Rust port of front-matter/commonmeta.
//!
//! Convert scholarly metadata between formats. The native model is [`Data`];
//! format modules read into it and write out of it.

pub mod author_utils;
pub mod constants;
pub mod crockford;
pub mod data;
pub mod doi_utils;
pub mod error;
pub mod file_utils;
mod formats;
pub mod progress;
pub mod schema_utils;
pub mod spdx;
pub mod utils;
pub mod vocabularies;

pub use data::Data;
pub use error::{Error, Result};
pub use formats::crossref;
pub use formats::inveniordm::PushResult;
pub use formats::ror::AffiliationMatch;
pub use formats::ror::RorRelease;

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

/// Like [`write`], but forwards `style` and `locale` to the citation writer.
/// For non-`"citation"` formats both parameters are ignored.
pub fn write_with_style(
    to: &str,
    data: &Data,
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<u8>> {
    formats::write_citation(to, data, style, locale)
}

/// Write a ROR-derived record as raw ROR-shaped JSON (as opposed to
/// `write("ror", data)`, which produces InvenioRDM vocabulary YAML).
pub fn write_ror_json(data: &Data) -> Result<Vec<u8>> {
    formats::ror::write_json(data)
}

/// Fetch a ROR organization by its ROR URL or other organization identifier
/// from the ROR API. Returns the record converted to the commonmeta `Data` model.
pub fn fetch_ror(id: &str) -> Result<Data> {
    formats::ror::fetch(id)
}

/// Fetch metadata for the latest ROR data release from Zenodo (InvenioRDM)
/// without downloading the full archive. Returns the version tag, release date,
/// Zenodo record ID, zip filename, and direct download URL.
pub fn fetch_latest_ror_release() -> Result<RorRelease> {
    formats::ror::fetch_latest_ror_release()
}

/// Download and parse the zip archive described by `release`. The zip is
/// cached locally for 30 days so repeat installs of the same version skip the
/// network round-trip. Returns `(records, from_cache)`.
pub fn download_ror_release(release: &RorRelease) -> Result<(Vec<formats::ror::Ror>, bool)> {
    formats::ror::download_release(release)
}

/// Convenience: fetch the latest release metadata then immediately download
/// and parse the dump. Returns `(RorRelease, Vec<Ror>, from_cache)`.
pub fn download_ror_all() -> Result<(RorRelease, Vec<formats::ror::Ror>, bool)> {
    formats::ror::download_all()
}

/// Look up a ROR organization by its full URL (e.g. `https://ror.org/012xzy7a9`)
/// from a local SQLite database written by [`write_ror_sqlite`]. Returns the
/// record converted to the commonmeta `Data` model, or an error when not found.
pub fn fetch_ror_sqlite(
    id: &str,
    db_path: &std::path::Path,
) -> Result<Data> {
    formats::ror::fetch_sqlite(id, db_path)
}

/// Write a list of ROR records to a SQLite3 database at `path` with an
/// `organizations` table. Existing file is deleted first. JSON array columns
/// (`types`, `locations`, `names`, `external_ids`) are queryable via SQLite's
/// `json_each()`. The `metadata` column stores the full ROR JSON as a
/// zstd-compressed BLOB for lossless round-trips.
///
/// Pass `version` and `date` (e.g. `"v2.9"`, `"2026-06-23"`) to record the
/// installed release in the `settings` table; pass `None` for both when writing
/// a standalone file where version tracking is not needed.
pub fn write_ror_sqlite(
    list: &[formats::ror::Ror],
    path: &std::path::Path,
    version: Option<&str>,
    date: Option<&str>,
) -> Result<()> {
    formats::ror::write_sqlite(list, path, version, date)
}

/// Return the ROR version string stored in the local database's `settings`
/// table, or `None` when the database does not exist or no version has been
/// recorded yet.
pub fn fetch_installed_ror_version(db_path: &std::path::Path) -> Result<Option<String>> {
    formats::ror::fetch_installed_ror_version(db_path)
}

/// Return the `vraix_date` (pidbox install date, `YYYY-MM-DD`) stored in the
/// local works database's `settings` table, or `None` when the database does
/// not exist or no date has been recorded yet.
pub fn fetch_installed_vraix_date(db_path: &std::path::Path) -> Result<Option<String>> {
    formats::vraix::fetch_installed_vraix_date(db_path)
}

/// Match a free-text affiliation string against ROR organizations using the
/// ROR v2 affiliation endpoint.
pub fn match_ror_affiliation(affiliation: &str) -> Result<Vec<AffiliationMatch>> {
    formats::ror::match_affiliation(affiliation)
}

/// Match a free-text affiliation string against a local ROR SQLite database
/// written by [`write_ror_sqlite`]. Uses Turso's Tantivy-backed FTS index for
/// full-text search across all organization name variants. Returns results in
/// relevance order with `chosen` set on the top result.
pub fn match_ror_affiliation_sqlite(
    affiliation: &str,
    db_path: &std::path::Path,
) -> Result<Vec<AffiliationMatch>> {
    formats::ror::match_affiliation_sqlite(affiliation, db_path)
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

/// Write `list` as a SQLite3 database with a `works` table whose columns
/// mirror the commonmeta v1.0 schema. Simple string fields are stored as
/// TEXT; complex fields are stored as compact JSON TEXT.
/// Any existing file at `path` is deleted first.
pub fn write_sqlite(list: &[Data], path: &std::path::Path) -> Result<()> {
    formats::commonmeta::write_sqlite(list, path)
}

/// Like [`write_sqlite`] but opens an existing database instead of recreating
/// it. Rows whose `id` already exists are replaced; new rows are inserted.
pub fn upsert_sqlite(list: &[Data], path: &std::path::Path) -> Result<()> {
    formats::commonmeta::upsert_sqlite(list, path)
}

/// Return the total number of rows in the `works` table of a commonmeta SQLite
/// database — useful for reporting the cumulative count after an upsert.
pub fn count_sqlite_works(path: &std::path::Path) -> Result<usize> {
    formats::commonmeta::count_sqlite_works(path)
}

/// Read records from a commonmeta SQLite database written by [`write_sqlite`].
pub fn read_sqlite_commonmeta(
    path: &std::path::Path,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<Data>> {
    formats::commonmeta::read_sqlite_commonmeta(path, limit, offset)
}

/// Stream a VRAIX daily dump at `input_path` directly to a commonmeta SQLite
/// database at `output_path` in batches of 10 000 rows, converting with
/// `from`-specific parser and writing each batch in a single transaction.
/// `limit` caps total records written; pass `0` for all rows.
/// When `update` is false the output file is deleted and recreated (default).
/// When `update` is true the existing file is kept and rows are upserted by
/// their `id` primary key — new rows are inserted, existing rows are replaced.
/// Returns the number of records written. No `Vec<Data>` is held for the
/// whole file — peak memory is proportional to one batch, not the whole dump.
pub fn stream_vraix_to_sqlite(
    input_path: &std::path::Path,
    from: &str,
    output_path: &std::path::Path,
    limit: usize,
    update: bool,
) -> Result<usize> {
    formats::vraix::stream_dump_to_sqlite(input_path, from, output_path, limit, !update)
}

/// Stream the pidbox dump (a mixed-source VRAIX SQLite file containing crossref,
/// datacite, and ROR rows) directly to a commonmeta SQLite database. Each row
/// is routed to the appropriate parser by its `source_id`; ROR rows are
/// skipped. When `update` is false the output file is recreated; when true
/// rows are upserted by `id`. Returns the number of records written.
pub fn stream_pidbox_to_sqlite(
    input_path: &std::path::Path,
    output_path: &std::path::Path,
    limit: usize,
    update: bool,
) -> Result<usize> {
    formats::vraix::stream_pidbox_to_sqlite(input_path, output_path, limit, !update)
}

/// Render a list of records to `to` format as a single buffer: a JSON array
/// for object-shaped formats (`commonmeta`, `csl`, `datacite`, `inveniordm`,
/// `schemaorg`, `ror`), or newline-joined output for line/document-shaped
/// formats (e.g. `bibtex`, `ris`, `crossref_xml`).
pub fn write_list(list: &[Data], to: &str) -> Result<Vec<u8>> {
    write_list_citation(list, to, None, None)
}

/// Like `write_list`, but passes CSL `style`/`locale` through to the
/// citation writer when `to == "citation"` (ignored for every other format,
/// same as `convert_citation`/`write_citation`).
pub fn write_list_citation(
    list: &[Data],
    to: &str,
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<u8>> {
    let bar = progress::count_bar("rendering", list.len() as u64);

    if matches!(
        to,
        "commonmeta"
            | "csl"
            | "datacite"
            | "inveniordm"
            | "schemaorg"
            | "ror"
            | "citation"
            | "crossref_xml"
    ) {
        let bytes = formats::write_all_citation(to, list, style, locale)?;
        bar.finish_and_clear();
        return Ok(bytes);
    }

    let mut output = String::new();
    for (idx, item) in list.iter().enumerate() {
        let rendered = formats::write_citation(to, item, style, locale)?;
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
    write_archive_citation(list, to, base_name, batch_size, None, None)
}

/// Like `write_archive`, but passes CSL `style`/`locale` through to the
/// citation writer when `to == "citation"`.
pub fn write_archive_citation(
    list: &[Data],
    to: &str,
    base_name: &str,
    batch_size: usize,
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<(String, Vec<u8>)>> {
    if list.is_empty() {
        return Err(Error::Serialize("no records to write".to_string()));
    }
    let chunks: Vec<&[Data]> = list.chunks(batch_size.max(1)).collect();
    let multi = chunks.len() > 1;

    let mut entries = Vec::with_capacity(chunks.len());
    for (idx, chunk) in chunks.into_iter().enumerate() {
        let bytes = write_list_citation(chunk, to, style, locale)?;
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
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
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
pub fn read_vraix_sqlite(
    sqlite_path: &str,
    from: &str,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<Data>> {
    formats::vraix::read_dump(sqlite_path, from, limit, offset)
}

/// Write a VRAIX dump's transport table (e.g. `pid_records`) to a single
/// Parquet file's bytes, using its raw columns (`pid`, `source_id`,
/// `raw_metadata`, ...) as-is — *not* converted to commonmeta `Data` the way
/// [`read_vraix_sqlite`] is. For analytics over the dump itself (e.g. via
/// DataFusion/Polars/DuckDB), not for ingesting it as commonmeta records.
/// `batch_size` controls how many rows land in each internal Parquet row
/// group (see [`formats::commonmeta::write_parquet_all`]'s analogous
/// `ROW_GROUP_SIZE` for why this matters for large dumps).
pub fn write_vraix_table_parquet(sqlite_path: &str, batch_size: usize) -> Result<Vec<u8>> {
    formats::vraix::write_table_parquet(sqlite_path, batch_size)
}

/// Fetch commonmeta records from a VRAIX daily dump for `from` ("crossref"
/// or "datacite") and `date` (YYYY-MM-DD).
///
/// With `input_path`, the local SQLite file at that path is read directly
/// via [`read_vraix_sqlite`] (e.g. an already-downloaded dump); otherwise
/// `{from}-{date}.sqlite3.zst` is downloaded from metadata.vraix.org via
/// [`file_utils::ensure_cached_path`] and decompressed into a temp file.
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
    let (zst_path, _from_cache) =
        file_utils::ensure_cached_path(&url, "vraix", &cache_key, cache_ttl)
            .map_err(|e| Error::Http(format!("failed to download '{}': {}", url, e)))?;

    let tmp_path = std::env::temp_dir().join(format!(
        "commonmeta-vraix-{}-{}-{}.sqlite3",
        from,
        date,
        std::process::id()
    ));
    file_utils::decompress_zst_file(&zst_path, &tmp_path)
        .map_err(|e| Error::Parse(format!("failed to decompress '{}': {}", url, e)))?;

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
        Data {
            id: id.to_string(),
            type_: "JournalArticle".to_string(),
            ..Data::default()
        }
    }

    #[test]
    fn test_write_list_json_array_formats() {
        let list = vec![
            sample_data("https://doi.org/10.1/a"),
            sample_data("https://doi.org/10.1/b"),
        ];
        let bytes = write_list(&list, "commonmeta").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_write_list_newline_joined_formats() {
        let list = vec![
            sample_data("https://doi.org/10.1/a"),
            sample_data("https://doi.org/10.1/b"),
        ];
        let bytes = write_list(&list, "ris").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        // Two records, newline-joined rather than a JSON array.
        assert_eq!(text.lines().filter(|l| l.starts_with("TY  -")).count(), 2);
    }

    #[test]
    fn test_write_list_crossref_xml_batches_into_one_doi_batch() {
        let list = vec![
            sample_data("https://doi.org/10.1/a"),
            sample_data("https://doi.org/10.1/b"),
        ];
        let bytes = write_list(&list, "crossref_xml").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(text.matches("<doi_batch xmlns=").count(), 1);
        assert_eq!(text.matches("<journal_article").count(), 2);
    }

    #[test]
    fn test_write_list_ror_uses_json_array_batch_writer() {
        let mut a = sample_data("https://ror.org/0342dzm54");
        a.title = "Org A".to_string();
        let mut b = sample_data("https://ror.org/0521rfr06");
        b.title = "Org B".to_string();

        let bytes = write_list(&[a, b], "ror").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_write_list_citation_renders_each_record() {
        let mut a = sample_data("https://doi.org/10.1/a");
        a.title = "Title A".to_string();
        a.date_published = "2020".to_string();
        let mut b = sample_data("https://doi.org/10.1/b");
        b.title = "Title B".to_string();
        b.date_published = "2021".to_string();

        let bytes = write_list(&[a, b], "citation").unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("Title A"));
        assert!(lines[1].contains("Title B"));
    }

    #[test]
    fn test_write_list_citation_respects_style() {
        let mut a = sample_data("https://doi.org/10.1/a");
        a.title = "Title A".to_string();
        a.date_published = "2020".to_string();

        let apa = write_list_citation(&[a.clone()], "citation", None, None).unwrap();
        let chicago =
            write_list_citation(&[a], "citation", Some("chicago-author-date"), None).unwrap();
        assert_ne!(apa, chicago);
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
        let list = vec![
            sample_data("https://doi.org/10.1/a"),
            sample_data("https://doi.org/10.1/b"),
        ];
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

        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT);")
                .unwrap();
            conn.execute(
                "INSERT INTO works (pid, source_id, raw_metadata) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    "pid-0",
                    1i64,
                    r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#
                ],
            )
            .unwrap();
        }

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
