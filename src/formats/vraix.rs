use crate::data::Data;
use crate::error::{Error, Result};
use crate::formats::{crossref, datacite, ror};

use arrow::array::{
    Int64Array, LargeStringArray, PrimitiveDictionaryBuilder, StringArray, StringDictionaryBuilder,
    TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Int8Type, Int32Type, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct VraixReadInput {
    sqlite_path: String,
    pid: String,
}

#[derive(Debug)]
struct TransportRow {
    source_id: i64,
    raw_metadata: String,
}

/// Read a VRAIX transport input and convert to commonmeta `Data`.
///
/// Expected input JSON:
/// `{ "sqlite_path": "...sqlite3", "pid": "10...." }`
pub fn read(input: &str) -> Result<Data> {
    let request: VraixReadInput =
        serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;

    read_sqlite(&request.sqlite_path, &request.pid)
}

/// Read one VRAIX row by `pid` from sqlite and route `raw_metadata`
/// to the source-specific parser based on `source_id`.
pub fn read_sqlite<P: AsRef<Path>>(sqlite_path: P, pid: &str) -> Result<Data> {
    let connection = Connection::open(sqlite_path).map_err(|e| Error::Parse(e.to_string()))?;
    let Some(table_name) =
        find_transport_table(&connection).map_err(|e| Error::Parse(e.to_string()))?
    else {
        return Err(Error::Parse("no VRAIX transport table found".to_string()));
    };

    let row = read_transport_row(&connection, &table_name, pid)
        .map_err(|e| Error::Parse(e.to_string()))?
        .ok_or_else(|| Error::InvalidId(format!("pid not found in VRAIX snapshot: {pid}")))?;

    route_raw_metadata(row.source_id, &row.raw_metadata)
}

fn find_transport_table(connection: &Connection) -> rusqlite::Result<Option<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let table_names = statement.query_map([], |row| row.get::<_, String>(0))?;

    for table_name in table_names {
        let table_name = table_name?;
        if table_has_transport_columns(connection, &table_name)? {
            return Ok(Some(table_name));
        }
    }

    Ok(None)
}

fn table_has_transport_columns(
    connection: &Connection,
    table_name: &str,
) -> rusqlite::Result<bool> {
    let pragma_query = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut statement = connection.prepare(&pragma_query)?;
    let column_names = statement.query_map([], |row| row.get::<_, String>(1))?;
    let mut has_pid = false;
    let mut has_source_id = false;
    let mut has_raw_metadata = false;

    for column_name in column_names {
        let column_name = column_name?;
        if column_name.eq_ignore_ascii_case("pid") {
            has_pid = true;
        }
        if column_name.eq_ignore_ascii_case("source_id") {
            has_source_id = true;
        }
        if column_name.eq_ignore_ascii_case("raw_metadata") {
            has_raw_metadata = true;
        }
    }

    Ok(has_pid && has_source_id && has_raw_metadata)
}

fn read_transport_row(
    connection: &Connection,
    table_name: &str,
    pid: &str,
) -> rusqlite::Result<Option<TransportRow>> {
    let query = format!(
        "SELECT source_id, raw_metadata FROM {} WHERE pid = ?1 LIMIT 1",
        quote_identifier(table_name)
    );
    let mut statement = connection.prepare(&query)?;

    statement
        .query_row([pid], |row| {
            let source_id: i64 = row.get(0)?;
            let raw_metadata: String = row.get(1)?;
            Ok(TransportRow {
                source_id,
                raw_metadata,
            })
        })
        .optional()
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn source_name_from_id(source_id: i64) -> Option<&'static str> {
    match source_id {
        1 => Some("crossref"),
        2 => Some("datacite"),
        3 => Some("ror"),
        _ => None,
    }
}

fn route_raw_metadata(source_id: i64, raw_metadata: &str) -> Result<Data> {
    match source_name_from_id(source_id) {
        Some("crossref") => read_crossref_row(raw_metadata),
        Some("datacite") => datacite::read_json(raw_metadata),
        Some("ror") => ror::read_json(raw_metadata),
        Some(_) | None => Err(Error::UnsupportedFormat(format!(
            "unsupported VRAIX source_id: {source_id}"
        ))),
    }
}

fn read_crossref_row(raw_metadata: &str) -> Result<Data> {
    let value: Value =
        serde_json::from_str(raw_metadata).map_err(|e| Error::Parse(e.to_string()))?;
    let json_text = if value.get("message").is_some() {
        raw_metadata.to_string()
    } else {
        json!({ "message": value }).to_string()
    };
    crossref::read_json(&json_text)
}

fn read_datacite_row(raw_metadata: &str) -> Result<Data> {
    let value: Value =
        serde_json::from_str(raw_metadata).map_err(|e| Error::Parse(e.to_string()))?;
    let json_text = if value.get("data").is_some() {
        raw_metadata.to_string()
    } else {
        json!({ "data": value }).to_string()
    };
    datacite::read_json(&json_text)
}

/// Read all (or a windowed slice of) rows from a VRAIX daily dump file.
///
/// Unlike [`read_sqlite`], which routes each row by its own `source_id` (for
/// ad hoc single-PID lookups against a transport table that may mix
/// sources), bulk VRAIX daily dumps are single-source per file — the
/// filename (`{from}-{date}.sqlite3.zst`) determines the source for every
/// row, so every row here is parsed using `from` rather than its
/// `source_id` column.
///
/// `limit: None` reads every row; `Some(n)` reads `n` rows starting at
/// `offset`.
pub fn read_dump<P: AsRef<Path>>(
    sqlite_path: P,
    from: &str,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<Data>> {
    let connection = Connection::open(sqlite_path).map_err(|e| Error::Parse(e.to_string()))?;
    let Some(table_name) =
        find_transport_table(&connection).map_err(|e| Error::Parse(e.to_string()))?
    else {
        return Err(Error::Parse(
            "no VRAIX table with pid/source_id/raw_metadata found".to_string(),
        ));
    };

    let convert_row: fn(&str) -> Result<Data> = match from {
        "crossref" => read_crossref_row,
        "datacite" => read_datacite_row,
        other => {
            return Err(Error::UnsupportedFormat(format!(
                "VRAIX dump source '{}' is not supported",
                other
            )));
        }
    };

    let total: u64 = match limit {
        Some(n) => n as u64,
        None => connection
            .query_row(
                &format!("SELECT COUNT(*) FROM {}", quote_identifier(&table_name)),
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64,
    };
    let bar = crate::progress::count_bar("converting", total);

    let query = match limit {
        Some(_) => format!(
            "SELECT raw_metadata FROM {} LIMIT ?1 OFFSET ?2",
            quote_identifier(&table_name)
        ),
        None => format!("SELECT raw_metadata FROM {}", quote_identifier(&table_name)),
    };
    let mut statement = connection
        .prepare(&query)
        .map_err(|e| Error::Parse(e.to_string()))?;

    let mut out = Vec::new();
    match limit {
        Some(n) => {
            let rows = statement
                .query_map([n as i64, offset as i64], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Parse(e.to_string()))?;
            for row in rows {
                let raw_metadata = row.map_err(|e| Error::Parse(e.to_string()))?;
                out.push(convert_row(&raw_metadata)?);
                bar.inc(1);
            }
        }
        None => {
            let rows = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| Error::Parse(e.to_string()))?;
            for row in rows {
                let raw_metadata = row.map_err(|e| Error::Parse(e.to_string()))?;
                out.push(convert_row(&raw_metadata)?);
                bar.inc(1);
            }
        }
    }
    bar.finish_and_clear();

    Ok(out)
}

// ── Arrow ingestion ─────────────────────────────────────────────────────────
//
// Reads a VRAIX transport table (e.g. `pid_records`) as Arrow `RecordBatch`es
// of its *raw* columns — unlike `read_dump`, this does not parse
// `raw_metadata` into commonmeta `Data`; it's for analytics/inspection over
// the dump itself (e.g. via DataFusion/Polars), not for conversion.

/// Arrow schema for a VRAIX transport table. `pid_type`/`source_id`/
/// `raw_metadata_type` are dictionary-encoded: every row in a given dump
/// shares the same source, so each dictionary ends up with one entry
/// repeated across the whole batch.
pub fn vraix_table_schema() -> Schema {
    Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("pid", DataType::Utf8, false),
        Field::new(
            "pid_type",
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Int32)),
            true,
        ),
        Field::new(
            "source_id",
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
            true,
        ),
        Field::new("resource_url", DataType::Utf8, true),
        Field::new(
            "last_modified",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            true,
        ),
        Field::new(
            "last_fetched",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            true,
        ),
        Field::new("raw_metadata", DataType::LargeUtf8, true),
        Field::new(
            "raw_metadata_type",
            DataType::Dictionary(Box::new(DataType::Int8), Box::new(DataType::Utf8)),
            true,
        ),
    ])
}

/// Read a VRAIX transport table into Arrow `RecordBatch`es of at most
/// `batch_size` rows each, using [`vraix_table_schema`]. Columns the table
/// doesn't have (e.g. the minimal `pid`/`source_id`/`raw_metadata`-only
/// fixtures used in this module's tests) come back as all-null rather than
/// erroring, so this works against any table `find_transport_table` finds.
pub fn read_table_arrow<P: AsRef<Path>>(
    sqlite_path: P,
    batch_size: usize,
) -> Result<Vec<RecordBatch>> {
    let connection = Connection::open(sqlite_path).map_err(|e| Error::Parse(e.to_string()))?;
    let table = find_transport_table(&connection)
        .map_err(|e| Error::Parse(e.to_string()))?
        .ok_or_else(|| {
            Error::Parse("no VRAIX table with pid/source_id/raw_metadata found".to_string())
        })?;

    let existing = table_columns(&connection, &table).map_err(|e| Error::Parse(e.to_string()))?;
    let has = |name: &str| existing.iter().any(|c| c.eq_ignore_ascii_case(name));
    let select_expr = |name: &str| -> String {
        if has(name) {
            quote_identifier(name)
        } else {
            format!("NULL AS {}", quote_identifier(name))
        }
    };

    const COLUMNS: [&str; 9] = [
        "id",
        "pid",
        "pid_type",
        "source_id",
        "resource_url",
        "last_modified",
        "last_fetched",
        "raw_metadata",
        "raw_metadata_type",
    ];
    let select = format!(
        "SELECT {} FROM {}",
        COLUMNS
            .iter()
            .map(|c| select_expr(c))
            .collect::<Vec<_>>()
            .join(", "),
        quote_identifier(&table)
    );

    let mut statement = connection
        .prepare(&select)
        .map_err(|e| Error::Parse(e.to_string()))?;
    let mut rows = statement
        .query([])
        .map_err(|e| Error::Parse(e.to_string()))?;
    let batch_size = batch_size.max(1);

    let mut batches = Vec::new();
    loop {
        let mut ids = Vec::with_capacity(batch_size);
        let mut pids = Vec::with_capacity(batch_size);
        let mut pid_types = Vec::with_capacity(batch_size);
        let mut source_ids = Vec::with_capacity(batch_size);
        let mut resource_urls = Vec::with_capacity(batch_size);
        let mut last_modifieds = Vec::with_capacity(batch_size);
        let mut last_fetcheds = Vec::with_capacity(batch_size);
        let mut raw_metadatas = Vec::with_capacity(batch_size);
        let mut raw_metadata_types = Vec::with_capacity(batch_size);

        let mut n = 0;
        while n < batch_size {
            let Some(row) = rows.next().map_err(|e| Error::Parse(e.to_string()))? else {
                break;
            };
            ids.push(
                row.get::<_, Option<i64>>(0)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .unwrap_or_default(),
            );
            pids.push(
                row.get::<_, Option<String>>(1)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .unwrap_or_default(),
            );
            pid_types.push(
                row.get::<_, Option<i64>>(2)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .map(|v| v as i32),
            );
            source_ids.push(
                row.get::<_, Option<i64>>(3)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .and_then(|v| source_name_from_id(v).map(str::to_string)),
            );
            resource_urls.push(
                row.get::<_, Option<String>>(4)
                    .map_err(|e| Error::Parse(e.to_string()))?,
            );
            last_modifieds.push(
                row.get::<_, Option<String>>(5)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .and_then(|s| parse_timestamp_micros(&s)),
            );
            last_fetcheds.push(
                row.get::<_, Option<String>>(6)
                    .map_err(|e| Error::Parse(e.to_string()))?
                    .and_then(|s| parse_timestamp_micros(&s)),
            );
            raw_metadatas.push(
                row.get::<_, Option<String>>(7)
                    .map_err(|e| Error::Parse(e.to_string()))?,
            );
            raw_metadata_types.push(
                row.get::<_, Option<String>>(8)
                    .map_err(|e| Error::Parse(e.to_string()))?,
            );
            n += 1;
        }
        if n == 0 {
            break;
        }

        batches.push(build_record_batch(
            ids,
            pids,
            pid_types,
            source_ids,
            resource_urls,
            last_modifieds,
            last_fetcheds,
            raw_metadatas,
            raw_metadata_types,
        )?);
    }

    Ok(batches)
}

/// Write a VRAIX transport table to a single Parquet file's bytes, using
/// [`vraix_table_schema`] — the raw transport columns (`pid`, `source_id`,
/// `raw_metadata`, ...) as-is, *not* converted to commonmeta `Data` the way
/// [`read_dump`] does. For analytics over the dump itself, not conversion.
pub fn write_table_parquet<P: AsRef<Path>>(sqlite_path: P, batch_size: usize) -> Result<Vec<u8>> {
    use parquet::arrow::ArrowWriter;

    let batches = read_table_arrow(sqlite_path, batch_size)?;
    let schema = Arc::new(vraix_table_schema());

    let buffer: Vec<u8> = Vec::new();
    let mut writer =
        ArrowWriter::try_new(buffer, schema, None).map_err(|e| Error::Serialize(e.to_string()))?;
    for batch in &batches {
        writer
            .write(batch)
            .map_err(|e| Error::Serialize(e.to_string()))?;
    }
    writer
        .into_inner()
        .map_err(|e| Error::Serialize(e.to_string()))
}

/// Parse an ISO 8601/RFC 3339 timestamp (e.g.
/// `"2026-06-15T10:27:15.404000+00:00"`) into microseconds since the epoch.
fn parse_timestamp_micros(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_micros())
}

fn table_columns(connection: &Connection, table_name: &str) -> rusqlite::Result<Vec<String>> {
    let pragma = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut statement = connection.prepare(&pragma)?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    columns.collect()
}

#[allow(clippy::too_many_arguments)]
fn build_record_batch(
    ids: Vec<i64>,
    pids: Vec<String>,
    pid_types: Vec<Option<i32>>,
    source_ids: Vec<Option<String>>,
    resource_urls: Vec<Option<String>>,
    last_modifieds: Vec<Option<i64>>,
    last_fetcheds: Vec<Option<i64>>,
    raw_metadatas: Vec<Option<String>>,
    raw_metadata_types: Vec<Option<String>>,
) -> Result<RecordBatch> {
    let id_array = Int64Array::from(ids);
    let pid_array = StringArray::from(pids);

    let mut pid_type_builder = PrimitiveDictionaryBuilder::<Int8Type, Int32Type>::new();
    for v in pid_types {
        match v {
            Some(x) => pid_type_builder.append_value(x),
            None => pid_type_builder.append_null(),
        }
    }
    let pid_type_array = pid_type_builder.finish();

    let mut source_id_builder = StringDictionaryBuilder::<Int8Type>::new();
    for v in source_ids {
        match v {
            Some(ref s) => source_id_builder.append_value(s),
            None => source_id_builder.append_null(),
        }
    }
    let source_id_array = source_id_builder.finish();

    let resource_url_array: StringArray = resource_urls.into_iter().collect();
    // `.with_timezone_utc()` stamps the literal offset "+00:00", which
    // doesn't match the schema's "UTC" timezone name (RecordBatch::try_new
    // checks for an exact string match) — so set it explicitly instead.
    let last_modified_array = TimestampMicrosecondArray::from(last_modifieds).with_timezone("UTC");
    let last_fetched_array = TimestampMicrosecondArray::from(last_fetcheds).with_timezone("UTC");
    let raw_metadata_array: LargeStringArray = raw_metadatas.into_iter().collect();

    let mut raw_metadata_type_builder = StringDictionaryBuilder::<Int8Type>::new();
    for v in raw_metadata_types {
        match v {
            Some(s) => raw_metadata_type_builder.append_value(s),
            None => raw_metadata_type_builder.append_null(),
        }
    }
    let raw_metadata_type_array = raw_metadata_type_builder.finish();

    let schema = Arc::new(vraix_table_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(id_array),
            Arc::new(pid_array),
            Arc::new(pid_type_array),
            Arc::new(source_id_array),
            Arc::new(resource_url_array),
            Arc::new(last_modified_array),
            Arc::new(last_fetched_array),
            Arc::new(raw_metadata_array),
            Arc::new(raw_metadata_type_array),
        ],
    )
    .map_err(|e| Error::Serialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn reads_known_crossref_pid_if_fixture_exists() {
        let path = Path::new("crossref-2026-06-15.sqlite3");
        if !path.exists() {
            return;
        }

        let data = read_sqlite(path, "10.1088/1402-4896/ac1a50")
            .expect("reading sqlite fixture should not fail");
        assert_eq!(data.id, "https://doi.org/10.1088/1402-4896/ac1a50");
    }

    /// Bulk VRAIX dumps are single-source per file: every row's
    /// `raw_metadata` is parsed using the caller's `from`, not a per-row
    /// `source_id` column (unlike `read_sqlite`'s transport-table lookup).
    fn write_dump_sqlite(path: &Path, rows: &[&str]) {
        std::fs::remove_file(path).ok();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute_batch("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT);")
            .unwrap();
        for (i, raw_metadata) in rows.iter().enumerate() {
            connection
                .execute(
                    "INSERT INTO works (pid, source_id, raw_metadata) VALUES (?1, ?2, ?3)",
                    rusqlite::params![format!("pid-{i}"), 1i64, raw_metadata],
                )
                .unwrap();
        }
    }

    #[test]
    fn test_read_dump_crossref() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_read_dump_crossref");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crossref.sqlite3");
        write_dump_sqlite(
            &path,
            &[r#"{"DOI":"10.1234/a","type":"journal-article","title":["Crossref Row"]}"#],
        );

        let data = read_dump(&path, "crossref", None, 0).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.1234/a");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_dump_datacite() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_read_dump_datacite");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("datacite.sqlite3");
        write_dump_sqlite(
            &path,
            &[r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#],
        );

        let data = read_dump(&path, "datacite", None, 0).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.5678/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_dump_respects_limit_and_offset() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_read_dump_window");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crossref.sqlite3");
        write_dump_sqlite(
            &path,
            &[
                r#"{"DOI":"10.1/a","type":"journal-article","title":["A"]}"#,
                r#"{"DOI":"10.1/b","type":"journal-article","title":["B"]}"#,
                r#"{"DOI":"10.1/c","type":"journal-article","title":["C"]}"#,
            ],
        );

        let data = read_dump(&path, "crossref", Some(1), 1).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_dump_rejects_unsupported_source() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_read_dump_unsupported");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("openalex.sqlite3");
        write_dump_sqlite(&path, &["{}"]);

        let err = read_dump(&path, "openalex", None, 0).unwrap_err();
        assert!(err.to_string().contains("not supported"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[allow(clippy::type_complexity)]
    fn write_full_table_sqlite(
        path: &Path,
        rows: &[(&str, i64, i64, &str, &str, &str, &str, &str)],
    ) {
        std::fs::remove_file(path).ok();
        let connection = rusqlite::Connection::open(path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE pid_records (
                    id INTEGER PRIMARY KEY,
                    pid TEXT NOT NULL,
                    pid_type INTEGER NOT NULL,
                    source_id INTEGER NOT NULL,
                    resource_url TEXT,
                    last_modified TIMESTAMP,
                    last_fetched TIMESTAMP NOT NULL,
                    raw_metadata TEXT,
                    raw_metadata_type TEXT
                );",
            )
            .unwrap();
        for (
            pid,
            pid_type,
            source_id,
            resource_url,
            last_modified,
            last_fetched,
            raw_metadata,
            raw_metadata_type,
        ) in rows
        {
            connection
                .execute(
                    "INSERT INTO pid_records
                        (pid, pid_type, source_id, resource_url, last_modified, last_fetched, raw_metadata, raw_metadata_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        pid,
                        pid_type,
                        source_id,
                        resource_url,
                        last_modified,
                        last_fetched,
                        raw_metadata,
                        raw_metadata_type
                    ],
                )
                .unwrap();
        }
    }

    #[test]
    fn test_read_table_arrow_minimal_fixture_fills_missing_columns_with_null() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_arrow_minimal");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crossref.sqlite3");
        write_dump_sqlite(&path, &[r#"{"DOI":"10.1/a"}"#, r#"{"DOI":"10.1/b"}"#]);

        let batches = read_table_arrow(&path, 100).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 2);
        assert_eq!(batch.schema().as_ref(), &vraix_table_schema());

        // pid/raw_metadata exist in the minimal fixture...
        let pid_col = batch
            .column_by_name("pid")
            .unwrap()
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(pid_col.value(0), "pid-0");
        // ...but pid_type/resource_url don't, so they come back null rather
        // than erroring.
        assert!(batch.column_by_name("resource_url").unwrap().is_null(0));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_table_arrow_full_schema_and_timestamps() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_arrow_full");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pid_records.sqlite3");
        write_full_table_sqlite(
            &path,
            &[(
                "10.1007/s11409-023-09352-z",
                0,
                1,
                "https://link.springer.com/10.1007/s11409-023-09352-z",
                "2026-06-15T10:27:15.404000+00:00",
                "2026-06-15T22:00:58.743345+00:00",
                r#"{"DOI":"10.1007/s11409-023-09352-z"}"#,
                "application/json",
            )],
        );

        let batches = read_table_arrow(&path, 100).unwrap();
        assert_eq!(batches.len(), 1);
        let batch = &batches[0];
        assert_eq!(batch.num_rows(), 1);

        let last_modified = batch
            .column_by_name("last_modified")
            .unwrap()
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        // 2026-06-15T10:27:15.404000 UTC in microseconds since the epoch.
        assert_eq!(last_modified.value(0), 1781519235404000);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_table_arrow_respects_batch_size() {
        let dir = std::env::temp_dir().join("commonmeta_vraix_arrow_batches");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crossref.sqlite3");
        write_dump_sqlite(
            &path,
            &[
                r#"{"DOI":"10.1/a"}"#,
                r#"{"DOI":"10.1/b"}"#,
                r#"{"DOI":"10.1/c"}"#,
            ],
        );

        let batches = read_table_arrow(&path, 2).unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].num_rows(), 2);
        assert_eq!(batches[1].num_rows(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_table_parquet_round_trip() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let dir = std::env::temp_dir().join("commonmeta_vraix_write_parquet");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crossref.sqlite3");
        write_dump_sqlite(&path, &[r#"{"DOI":"10.1/a"}"#, r#"{"DOI":"10.1/b"}"#]);

        let bytes = write_table_parquet(&path, 100).unwrap();
        assert_eq!(&bytes[0..4], b"PAR1");

        let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes)).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 2);
        let schema = reader.metadata().file_metadata().schema_descr();
        let column_names: Vec<String> = (0..schema.num_columns())
            .map(|i| schema.column(i).name().to_string())
            .collect();
        assert_eq!(
            column_names,
            vec![
                "id",
                "pid",
                "pid_type",
                "source_id",
                "resource_url",
                "last_modified",
                "last_fetched",
                "raw_metadata",
                "raw_metadata_type"
            ]
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod temp_real_data_check {
    #[test]
    #[ignore]
    fn check_real_vraix_sample() {
        let bytes = super::write_table_parquet("/tmp/vraix_sample.sqlite3", 100_000).unwrap();
        std::fs::write("/tmp/vraix_sample.parquet", &bytes).unwrap();
        println!("wrote {} bytes to /tmp/vraix_sample.parquet", bytes.len());

        let batches = super::read_table_arrow("/tmp/vraix_sample.sqlite3", 100_000).unwrap();
        for batch in &batches {
            println!("batch: {} rows", batch.num_rows());
        }
    }
}
