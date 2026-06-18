use crate::data::Data;
use crate::error::{Error, Result};
use crate::formats::{crossref, datacite, ror};

use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;

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

fn table_has_transport_columns(connection: &Connection, table_name: &str) -> rusqlite::Result<bool> {
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
    let value: Value = serde_json::from_str(raw_metadata).map_err(|e| Error::Parse(e.to_string()))?;
    let json_text = if value.get("message").is_some() {
        raw_metadata.to_string()
    } else {
        json!({ "message": value }).to_string()
    };
    crossref::read_json(&json_text)
}

fn read_datacite_row(raw_metadata: &str) -> Result<Data> {
    let value: Value = serde_json::from_str(raw_metadata).map_err(|e| Error::Parse(e.to_string()))?;
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
        return Err(Error::Parse("no VRAIX table with pid/source_id/raw_metadata found".to_string()));
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
    let mut statement = connection.prepare(&query).map_err(|e| Error::Parse(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::{read_dump, read_sqlite};
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
        write_dump_sqlite(&path, &[r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#]);

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
}
