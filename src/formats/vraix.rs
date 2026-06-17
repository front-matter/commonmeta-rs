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
        Some("crossref") => {
            let value: Value =
                serde_json::from_str(raw_metadata).map_err(|e| Error::Parse(e.to_string()))?;
            let json_text = if value.get("message").is_some() {
                raw_metadata.to_string()
            } else {
                json!({ "message": value }).to_string()
            };
            crossref::read_json(&json_text)
        }
        Some("datacite") => datacite::read_json(raw_metadata),
        Some("ror") => ror::read_json(raw_metadata),
        Some(_) | None => Err(Error::UnsupportedFormat(format!(
            "unsupported VRAIX source_id: {source_id}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::read_sqlite;
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
}
