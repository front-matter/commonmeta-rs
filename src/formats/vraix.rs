use crate::data::Data;
use crate::error::{Error, Result};
use crate::formats::{crossref, datacite, ror};

use arrow::array::{
    Int64Array, LargeStringArray, PrimitiveDictionaryBuilder, StringArray, StringDictionaryBuilder,
    TimestampMicrosecondArray,
};
use arrow::datatypes::{DataType, Field, Int8Type, Int32Type, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
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
    let pid = pid.to_string();
    let path = sqlite_path.as_ref().to_path_buf();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let db = turso::Builder::new_local(&path.to_string_lossy())
                .build()
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;
            let connection = db.connect().map_err(|e| Error::Parse(e.to_string()))?;

            let Some(table_name) = find_transport_table(&connection).await? else {
                return Err(Error::Parse("no VRAIX transport table found".to_string()));
            };

            let row = read_transport_row(&connection, &table_name, &pid)
                .await?
                .ok_or_else(|| Error::InvalidId(format!("pid not found in VRAIX snapshot: {pid}")))?;

            route_raw_metadata(row.source_id, &row.raw_metadata)
        })
}

async fn find_transport_table(connection: &turso::Connection) -> crate::error::Result<Option<String>> {
    let mut rows = connection
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            (),
        )
        .await
        .map_err(|e| crate::error::Error::Parse(e.to_string()))?;

    while let Some(row) = rows.next().await.map_err(|e| crate::error::Error::Parse(e.to_string()))? {
        let table_name: String = row.get(0).map_err(|e| crate::error::Error::Parse(e.to_string()))?;
        if table_has_transport_columns(connection, &table_name).await? {
            return Ok(Some(table_name));
        }
    }

    Ok(None)
}

async fn table_has_transport_columns(
    connection: &turso::Connection,
    table_name: &str,
) -> crate::error::Result<bool> {
    let pragma_query = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut rows = connection
        .query(&pragma_query, ())
        .await
        .map_err(|e| crate::error::Error::Parse(e.to_string()))?;

    let mut has_pid = false;
    let mut has_source_id = false;
    let mut has_raw_metadata = false;

    while let Some(row) = rows.next().await.map_err(|e| crate::error::Error::Parse(e.to_string()))? {
        let column_name: String = row.get(1).map_err(|e| crate::error::Error::Parse(e.to_string()))?;
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

async fn read_transport_row(
    connection: &turso::Connection,
    table_name: &str,
    pid: &str,
) -> crate::error::Result<Option<TransportRow>> {
    let query = format!(
        "SELECT source_id, raw_metadata FROM {} WHERE pid = ?1 LIMIT 1",
        quote_identifier(table_name)
    );
    let mut rows = connection
        .query(&query, turso::params![pid])
        .await
        .map_err(|e| crate::error::Error::Parse(e.to_string()))?;

    match rows.next().await.map_err(|e| crate::error::Error::Parse(e.to_string()))? {
        Some(row) => Ok(Some(TransportRow {
            source_id: row.get(0).map_err(|e| crate::error::Error::Parse(e.to_string()))?,
            raw_metadata: row.get(1).map_err(|e| crate::error::Error::Parse(e.to_string()))?,
        })),
        None => Ok(None),
    }
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
    let path = sqlite_path.as_ref().to_path_buf();
    let from = from.to_string();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let db = turso::Builder::new_local(&path.to_string_lossy())
                .build()
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;
            let connection = db.connect().map_err(|e| Error::Parse(e.to_string()))?;

            let Some(table_name) = find_transport_table(&connection).await? else {
                return Err(Error::Parse(
                    "no VRAIX table with pid/source_id/raw_metadata found".to_string(),
                ));
            };

            let convert_row: fn(&str) -> Result<Data> = match from.as_str() {
                "crossref" => read_crossref_row,
                "datacite" => read_datacite_row,
                other => {
                    return Err(Error::UnsupportedFormat(format!(
                        "VRAIX dump source '{}' is not supported",
                        other
                    )));
                }
            };

            let quoted = quote_identifier(&table_name);

            // Count for progress bar
            let total: u64 = match limit {
                Some(n) => n as u64,
                None => {
                    let mut rows = connection
                        .query(&format!("SELECT COUNT(*) FROM {quoted}"), ())
                        .await
                        .unwrap_or_else(|_| unreachable!());
                    if let Ok(Some(row)) = rows.next().await {
                        row.get::<i64>(0).unwrap_or(0).max(0) as u64
                    } else {
                        0
                    }
                }
            };
            let bar = crate::progress::count_bar("converting", total);

            let query = match limit {
                Some(_) => format!("SELECT raw_metadata FROM {quoted} LIMIT ?1 OFFSET ?2"),
                None => format!("SELECT raw_metadata FROM {quoted}"),
            };

            let mut row_iter = match limit {
                Some(n) => {
                    connection
                        .query(&query, turso::params![n as i64, offset as i64])
                        .await
                        .map_err(|e| Error::Parse(e.to_string()))?
                }
                None => {
                    connection
                        .query(&query, ())
                        .await
                        .map_err(|e| Error::Parse(e.to_string()))?
                }
            };

            let mut out = Vec::new();
            while let Some(row) = row_iter.next().await.map_err(|e| Error::Parse(e.to_string()))? {
                let raw_metadata: String =
                    row.get(0).map_err(|e| Error::Parse(e.to_string()))?;
                out.push(convert_row(&raw_metadata)?);
                bar.inc(1);
            }
            bar.finish_and_clear();

            Ok(out)
        })
}

// ── Streaming VRAIX → commonmeta SQLite ────────────────────────────────────

/// Number of rows per batch in [`stream_dump_to_sqlite`].
/// Larger batches amortise transaction overhead; 50 K keeps peak RAM under ~500 MB
/// for typical Crossref record sizes (~5-10 KB JSON each).
const STREAM_BATCH_SIZE: usize = 50_000;

/// Convert raw-metadata strings to fully serialized [`PreparedRow`]s in
/// parallel, splitting work across logical CPUs with `std::thread::scope`.
///
/// Each thread runs the full pipeline for its chunk:
///   raw JSON → `Data` (parse) → `PreparedRow` (prepare + JSON-serialize fields)
///
/// This means the expensive work (16 `serde_json::to_string` calls per record
/// and the v1.0 field-stripping pass) happens in parallel rather than on the
/// single-threaded write path, which only needs to bind pre-serialized strings.
fn parallel_convert_and_prepare(
    raw: &[String],
    convert: fn(&str) -> crate::error::Result<Data>,
) -> Vec<crate::formats::commonmeta::PreparedRow> {
    use crate::formats::commonmeta::serialize_to_row;
    if raw.is_empty() {
        return Vec::new();
    }
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    let chunk_size = (raw.len() / ncpu).max(1);

    std::thread::scope(|scope| {
        let handles: Vec<_> = raw
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|s| convert(s).ok().map(serialize_to_row))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

/// Stream a VRAIX daily dump to a commonmeta SQLite database without
/// holding the whole dataset in memory. Records are read from
/// `input_path`, converted in parallel per batch, and written to
/// `output_path` one batch per SQLite transaction.
///
/// `limit` caps the total number of records written. Pass `0` to write
/// every row in the dump (equivalent to `--number 0` on the CLI).
///
/// Returns the number of records written.
pub fn stream_dump_to_sqlite(
    input_path: &Path,
    from: &str,
    output_path: &Path,
    limit: usize,
    overwrite: bool,
) -> crate::error::Result<usize> {
    use crate::formats::commonmeta::{init_sqlite_writer_async, write_sqlite_batch_rows_async};
    use crate::error::Error;

    let convert: fn(&str) -> crate::error::Result<Data> = match from {
        "crossref" => read_crossref_row,
        "datacite" => read_datacite_row,
        other => {
            return Err(Error::UnsupportedFormat(format!(
                "VRAIX dump source '{}' is not supported",
                other
            )));
        }
    };

    let input_path = input_path.to_path_buf();
    let output_path = output_path.to_path_buf();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let in_db = turso::Builder::new_local(&input_path.to_string_lossy())
                .build()
                .await
                .map_err(|e| Error::Parse(format!("failed to open '{}': {}", input_path.display(), e)))?;
            let in_conn = in_db.connect()
                .map_err(|e| Error::Parse(format!("failed to connect '{}': {}", input_path.display(), e)))?;

            // 256 MB page cache + mmap — same perf pragmas as before.
            in_conn
                .execute_batch(
                    "PRAGMA cache_size=-262144;\
                     PRAGMA mmap_size=17179869184;",
                )
                .await
                .ok();

            let Some(table_name) = find_transport_table(&in_conn).await? else {
                return Err(Error::Parse(
                    "no VRAIX table with pid/source_id/raw_metadata found".to_string(),
                ));
            };
            let quoted = quote_identifier(&table_name);

            // Row count for the progress bar.
            let row_count: u64 = {
                let mut rows = in_conn
                    .query(&format!("SELECT COUNT(*) FROM {quoted}"), ())
                    .await
                    .unwrap_or_else(|_| unreachable!());
                if let Ok(Some(row)) = rows.next().await {
                    row.get::<i64>(0).unwrap_or(0).max(0) as u64
                } else {
                    0
                }
            };
            let total = if limit == 0 { row_count } else { row_count.min(limit as u64) };
            let bar = crate::progress::count_bar("converting", total);

            // Rowid cursor — O(N) instead of LIMIT+OFFSET O(N²).
            let cursor_sql = format!(
                "SELECT rowid, raw_metadata FROM {quoted} WHERE rowid > ?1 ORDER BY rowid LIMIT ?2"
            );

            let out_conn = init_sqlite_writer_async(&output_path, overwrite).await?;
            let mut written = 0usize;
            let mut last_rowid: i64 = 0;

            loop {
                let remaining = if limit == 0 {
                    STREAM_BATCH_SIZE
                } else {
                    limit.saturating_sub(written)
                };
                if remaining == 0 {
                    break;
                }
                let batch_size = STREAM_BATCH_SIZE.min(remaining);

                let mut row_iter = in_conn
                    .query(&cursor_sql, turso::params![last_rowid, batch_size as i64])
                    .await
                    .map_err(|e| Error::Parse(e.to_string()))?;

                let mut raw_batch: Vec<(i64, String)> = Vec::with_capacity(batch_size);
                while let Some(row) = row_iter.next().await.map_err(|e| Error::Parse(e.to_string()))? {
                    let rowid: i64 = row.get(0).map_err(|e| Error::Parse(e.to_string()))?;
                    let raw: String = row.get(1).map_err(|e| Error::Parse(e.to_string()))?;
                    raw_batch.push((rowid, raw));
                }

                if raw_batch.is_empty() {
                    break;
                }
                let batch_len = raw_batch.len();
                last_rowid = raw_batch.last().unwrap().0;

                let raw: Vec<String> = raw_batch.into_iter().map(|(_, s)| s).collect();
                let rows_prepared = parallel_convert_and_prepare(&raw, convert);
                let batch_written = rows_prepared.len();
                write_sqlite_batch_rows_async(&out_conn, rows_prepared).await?;

                bar.inc(batch_len as u64);
                written += batch_written;

                if batch_len < batch_size {
                    break;
                }
            }
            bar.finish_and_clear();

            Ok(written)
        })
}

// ── Pidbox (mixed-source) streaming ────────────────────────────────────────

/// Like [`parallel_convert_and_prepare`] but routes each record by its
/// `source_id` rather than a fixed converter. ROR rows (`source_id == 3`)
/// and any unrecognised source are silently skipped.
fn parallel_convert_and_prepare_mixed(
    raw: &[(i64, String)],
) -> Vec<crate::formats::commonmeta::PreparedRow> {
    use crate::formats::commonmeta::serialize_to_row;
    if raw.is_empty() {
        return Vec::new();
    }
    let ncpu = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(1);
    let chunk_size = (raw.len() / ncpu).max(1);

    std::thread::scope(|scope| {
        let handles: Vec<_> = raw
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .filter_map(|(source_id, s)| {
                            let data = match source_id {
                                1 => read_crossref_row(s).ok(),
                                2 => read_datacite_row(s).ok(),
                                _ => None,
                            }?;
                            Some(serialize_to_row(data))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    })
}

/// Stream the pidbox dump (a mixed-source VRAIX SQLite file) to a commonmeta
/// SQLite database. Each row is routed to the crossref or datacite parser by
/// its `source_id`; ROR rows (`source_id == 3`) and any other unrecognised
/// source are skipped. Conversion is parallelised; writes are batched.
pub fn stream_pidbox_to_sqlite(
    input_path: &Path,
    output_path: &Path,
    limit: usize,
    overwrite: bool,
) -> crate::error::Result<usize> {
    use crate::formats::commonmeta::{init_sqlite_writer_async, write_sqlite_batch_rows_async};
    use crate::error::Error;

    let input_path = input_path.to_path_buf();
    let output_path = output_path.to_path_buf();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let in_db = turso::Builder::new_local(&input_path.to_string_lossy())
                .build()
                .await
                .map_err(|e| Error::Parse(format!("failed to open '{}': {}", input_path.display(), e)))?;
            let in_conn = in_db.connect()
                .map_err(|e| Error::Parse(format!("failed to connect '{}': {}", input_path.display(), e)))?;

            in_conn
                .execute_batch(
                    "PRAGMA cache_size=-262144;\
                     PRAGMA mmap_size=17179869184;",
                )
                .await
                .ok();

            let Some(table_name) = find_transport_table(&in_conn).await? else {
                return Err(Error::Parse(
                    "no VRAIX table with pid/source_id/raw_metadata found".to_string(),
                ));
            };
            let quoted = quote_identifier(&table_name);

            let row_count: u64 = {
                let mut rows = in_conn
                    .query(
                        &format!("SELECT COUNT(*) FROM {quoted} WHERE source_id != 3"),
                        (),
                    )
                    .await
                    .unwrap_or_else(|_| unreachable!());
                if let Ok(Some(row)) = rows.next().await {
                    row.get::<i64>(0).unwrap_or(0).max(0) as u64
                } else {
                    0
                }
            };
            let total = if limit == 0 { row_count } else { row_count.min(limit as u64) };
            let bar = crate::progress::count_bar("converting", total);

            let cursor_sql = format!(
                "SELECT rowid, source_id, raw_metadata FROM {quoted} \
                 WHERE rowid > ?1 AND source_id != 3 \
                 ORDER BY rowid LIMIT ?2"
            );

            let out_conn = init_sqlite_writer_async(&output_path, overwrite).await?;
            let mut written = 0usize;
            let mut last_rowid: i64 = 0;

            loop {
                let remaining = if limit == 0 {
                    STREAM_BATCH_SIZE
                } else {
                    limit.saturating_sub(written)
                };
                if remaining == 0 {
                    break;
                }
                let batch_size = STREAM_BATCH_SIZE.min(remaining);

                let mut row_iter = in_conn
                    .query(&cursor_sql, turso::params![last_rowid, batch_size as i64])
                    .await
                    .map_err(|e| Error::Parse(e.to_string()))?;

                let mut raw_batch: Vec<(i64, i64, String)> = Vec::with_capacity(batch_size);
                while let Some(row) =
                    row_iter.next().await.map_err(|e| Error::Parse(e.to_string()))?
                {
                    let rowid: i64 = row.get(0).map_err(|e| Error::Parse(e.to_string()))?;
                    let source_id: i64 = row.get(1).map_err(|e| Error::Parse(e.to_string()))?;
                    let raw: String = row.get(2).map_err(|e| Error::Parse(e.to_string()))?;
                    raw_batch.push((rowid, source_id, raw));
                }

                if raw_batch.is_empty() {
                    break;
                }
                let batch_len = raw_batch.len();
                last_rowid = raw_batch.last().unwrap().0;

                let pairs: Vec<(i64, String)> =
                    raw_batch.into_iter().map(|(_, sid, s)| (sid, s)).collect();
                let rows_prepared = parallel_convert_and_prepare_mixed(&pairs);
                let batch_written = rows_prepared.len();
                write_sqlite_batch_rows_async(&out_conn, rows_prepared).await?;

                bar.inc(batch_len as u64);
                written += batch_written;

                if batch_len < batch_size {
                    break;
                }
            }
            bar.finish_and_clear();

            Ok(written)
        })
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
    let path = sqlite_path.as_ref().to_path_buf();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let db = turso::Builder::new_local(&path.to_string_lossy())
                .build()
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;
            let connection = db.connect().map_err(|e| Error::Parse(e.to_string()))?;

            let table = find_transport_table(&connection)
                .await?
                .ok_or_else(|| {
                    Error::Parse(
                        "no VRAIX table with pid/source_id/raw_metadata found".to_string(),
                    )
                })?;

            let existing = table_columns(&connection, &table).await?;
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

            // Collect all rows in a single async pass to avoid re-polling a
            // depleted `Rows` iterator (libsql's Rows::next hangs after None).
            type RawRow = (i64, String, Option<i32>, Option<String>, Option<String>,
                           Option<String>, Option<String>, Option<String>, Option<String>);

            let mut all_raw: Vec<RawRow> = Vec::new();
            let mut row_iter = connection
                .query(&select, ())
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;

            while let Some(row) = row_iter.next().await.map_err(|e| Error::Parse(e.to_string()))? {
                all_raw.push((
                    row.get::<Option<i64>>(0).map_err(|e| Error::Parse(e.to_string()))?.unwrap_or_default(),
                    row.get::<Option<String>>(1).map_err(|e| Error::Parse(e.to_string()))?.unwrap_or_default(),
                    row.get::<Option<i64>>(2).map_err(|e| Error::Parse(e.to_string()))?.map(|v| v as i32),
                    row.get::<Option<i64>>(3).map_err(|e| Error::Parse(e.to_string()))?.and_then(|v| source_name_from_id(v).map(str::to_string)),
                    row.get::<Option<String>>(4).map_err(|e| Error::Parse(e.to_string()))?,
                    row.get::<Option<String>>(5).map_err(|e| Error::Parse(e.to_string()))?.and_then(|s| parse_timestamp_micros(&s).map(|_| s)),
                    row.get::<Option<String>>(6).map_err(|e| Error::Parse(e.to_string()))?.and_then(|s| parse_timestamp_micros(&s).map(|_| s)),
                    row.get::<Option<String>>(7).map_err(|e| Error::Parse(e.to_string()))?,
                    row.get::<Option<String>>(8).map_err(|e| Error::Parse(e.to_string()))?,
                ));
            }

            let batch_size = batch_size.max(1);
            let mut batches = Vec::new();
            for chunk in all_raw.chunks(batch_size) {
                let ids: Vec<i64>               = chunk.iter().map(|r| r.0).collect();
                let pids: Vec<String>           = chunk.iter().map(|r| r.1.clone()).collect();
                let pid_types: Vec<Option<i32>> = chunk.iter().map(|r| r.2).collect();
                let source_ids: Vec<Option<String>> = chunk.iter().map(|r| r.3.clone()).collect();
                let resource_urls: Vec<Option<String>> = chunk.iter().map(|r| r.4.clone()).collect();
                let last_modifieds: Vec<Option<i64>> = chunk.iter()
                    .map(|r| r.5.as_deref().and_then(parse_timestamp_micros))
                    .collect();
                let last_fetcheds: Vec<Option<i64>> = chunk.iter()
                    .map(|r| r.6.as_deref().and_then(parse_timestamp_micros))
                    .collect();
                let raw_metadatas: Vec<Option<String>>      = chunk.iter().map(|r| r.7.clone()).collect();
                let raw_metadata_types: Vec<Option<String>> = chunk.iter().map(|r| r.8.clone()).collect();

                batches.push(build_record_batch(
                    ids, pids, pid_types, source_ids, resource_urls,
                    last_modifieds, last_fetcheds, raw_metadatas, raw_metadata_types,
                )?);
            }

            Ok(batches)
        })
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

async fn table_columns(connection: &turso::Connection, table_name: &str) -> crate::error::Result<Vec<String>> {
    let pragma = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut rows = connection
        .query(&pragma, ())
        .await
        .map_err(|e| crate::error::Error::Parse(e.to_string()))?;
    let mut columns = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| crate::error::Error::Parse(e.to_string()))? {
        columns.push(row.get::<String>(1).map_err(|e| crate::error::Error::Parse(e.to_string()))?);
    }
    Ok(columns)
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
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = turso::Builder::new_local(&path.to_string_lossy()).build().await.unwrap();
                let conn = db.connect().unwrap();
                conn.execute_batch(
                    "CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT);",
                )
                .await
                .unwrap();
                for (i, raw_metadata) in rows.iter().enumerate() {
                    conn.execute(
                        "INSERT INTO works (pid, source_id, raw_metadata) VALUES (?1, ?2, ?3)",
                        turso::params![format!("pid-{i}"), 1i64, *raw_metadata],
                    )
                    .await
                    .unwrap();
                }
            });
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
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = turso::Builder::new_local(&path.to_string_lossy()).build().await.unwrap();
                let conn = db.connect().unwrap();
                conn.execute_batch(
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
                .await
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
                    conn.execute(
                        "INSERT INTO pid_records
                            (pid, pid_type, source_id, resource_url, last_modified, last_fetched, raw_metadata, raw_metadata_type)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        turso::params![
                            *pid, *pid_type, *source_id, *resource_url,
                            *last_modified, *last_fetched, *raw_metadata, *raw_metadata_type
                        ],
                    )
                    .await
                    .unwrap();
                }
            });
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
