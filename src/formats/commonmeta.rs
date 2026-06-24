use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::data::Data;
use crate::error::{Error, Result};
use crate::schema_utils::json_schema_errors;
use crate::utils::normalize_ror;

const COMMONMETA_V1_SCHEMA_URL: &str = "https://commonmeta.org/commonmeta_v1.0.json";

/// Parse v1.0-shaped commonmeta JSON directly into `Data`, since `Data`'s
/// fields already match the schema 1:1.
pub fn read(json: &str) -> Result<Data> {
    let value: Value = serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;

    if !looks_like_v1(&value) {
        return Err(Error::Parse(
            "commonmeta input is not schema v1.0 shaped".to_string(),
        ));
    }

    serde_json::from_value(value).map_err(|e| Error::Parse(e.to_string()))
}

/// Stamp `schema_version`, strip non-v1.0 reference fields, and clear
/// non-ROR ids from organization/publisher (schema requires ROR for those).
fn prepare(data: &Data) -> Data {
    let mut out = data.clone();
    out.schema_version = COMMONMETA_V1_SCHEMA_URL.to_string();
    // v1.0 references schema only defines: key, id, type, reference
    for r in &mut out.references {
        r.publisher.clear();
        r.publication_year.clear();
        r.volume.clear();
        r.issue.clear();
        r.first_page.clear();
        r.last_page.clear();
        r.unstructured.clear();
        r.asserted_by.clear();
    }
    // organization.id must be a ROR URL per the v1.0 schema
    if !out.publisher.id.is_empty() && normalize_ror(&out.publisher.id).is_empty() {
        out.publisher.id.clear();
    }
    for c in &mut out.contributors {
        if let Some(p) = &mut c.person {
            for aff in &mut p.affiliations {
                if !aff.id.is_empty() && normalize_ror(&aff.id).is_empty() {
                    aff.id.clear();
                }
            }
        }
        if let Some(org) = &mut c.organization {
            if !org.id.is_empty() && normalize_ror(&org.id).is_empty() {
                org.id.clear();
            }
        }
    }
    out
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let out = prepare(data);
    let bytes = serde_json::to_vec(&out).map_err(|e| Error::Serialize(e.to_string()))?;
    json_schema_errors(&bytes, Some("commonmeta"))?;
    Ok(bytes)
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let prepared: Vec<Data> = list.iter().map(prepare).collect();
    let bytes =
        serde_json::to_vec_pretty(&prepared).map_err(|e| Error::Serialize(e.to_string()))?;
    json_schema_errors(&bytes, Some("commonmeta"))?;
    Ok(bytes)
}

fn looks_like_v1(value: &Value) -> bool {
    let Some(obj) = value.as_object() else {
        return false;
    };

    obj.get("schema_version").and_then(Value::as_str) == Some(COMMONMETA_V1_SCHEMA_URL)
        || obj.contains_key("date_published")
        || obj.contains_key("additional_titles")
        || obj.contains_key("additional_descriptions")
        || obj
            .get("identifiers")
            .and_then(Value::as_array)
            .and_then(|ids| ids.first())
            .and_then(Value::as_object)
            .is_some_and(|id_obj| id_obj.contains_key("identifier_type"))
        || obj
            .get("contributors")
            .and_then(Value::as_array)
            .and_then(|contributors| contributors.first())
            .and_then(Value::as_object)
            .is_some_and(|contributor| {
                contributor.contains_key("person") || contributor.contains_key("organization")
            })
}

// ── Bulk Parquet writer (catalog dumps) ───────────────────────────────────────
//
// Parquet needs a flat, scalar schema, but `Data` is deeply nested
// (contributors, identifiers, etc. are all lists). `CommonmetaRow` flattens
// the fields most useful for analysis/filtering (e.g. in DuckDB) without
// needing to parse JSON, in the same spirit as the `RorCsv` flattening in
// ror.rs — but unlike that one, it also carries a `json` column with the
// complete record's JSON serialization, so `read_parquet_all` can
// reconstruct the original `Data` exactly rather than just the flattened
// subset. The other columns are a queryable convenience layer on top of
// that, not the source of truth.

/// A flattened, Parquet-friendly view of a single commonmeta `Data` record.
#[derive(
    Debug,
    Default,
    Clone,
    Serialize,
    parquet_derive::ParquetRecordWriter,
    parquet_derive::ParquetRecordReader,
)]
pub struct CommonmetaRow {
    pub id: String,
    pub record_type: String,
    pub title: String,
    pub url: String,
    pub doi: String,
    pub publisher: String,
    pub language: String,
    pub version: String,
    pub license: String,
    pub container_title: String,
    pub container_type: String,
    pub volume: String,
    pub issue: String,
    pub first_page: String,
    pub last_page: String,
    pub date_published: String,
    pub date_created: String,
    pub date_updated: String,
    pub contributor_count: i32,
    pub first_author_name: String,
    pub first_author_orcid: String,
    pub subjects: String,
    pub description: String,
    pub provider: String,
    pub additional_type: String,
    /// Complete JSON serialization of the original `Data` record. The
    /// authoritative source for `read_parquet_all`; the columns above exist
    /// for filtering/analysis without needing to parse this.
    pub json: String,
}

/// Flatten a `Data` record into its tabular `CommonmetaRow` representation.
fn flatten_row(data: &Data) -> CommonmetaRow {
    let doi = data
        .identifiers
        .iter()
        .find(|i| i.identifier_type == "DOI")
        .map(|i| i.identifier.clone())
        .unwrap_or_else(|| {
            if data.id.contains("doi.org") {
                data.id.clone()
            } else {
                String::new()
            }
        });

    let (first_author_name, first_author_orcid) = data
        .contributors
        .first()
        .map(|c| (c.name(), c.id().to_string()))
        .unwrap_or_default();

    let subjects = data
        .subjects
        .iter()
        .map(|s| s.subject.as_str())
        .collect::<Vec<_>>()
        .join("; ");

    let json = serde_json::to_string(data).unwrap_or_default();

    CommonmetaRow {
        id: data.id.clone(),
        record_type: data.type_.clone(),
        title: data.title.clone(),
        url: data.url.clone(),
        doi,
        publisher: data.publisher.name.clone(),
        language: data.language.clone(),
        version: data.version.clone(),
        license: data.license.id.clone(),
        container_title: data.container.title.clone(),
        container_type: data.container.type_.clone(),
        volume: data.container.volume.clone(),
        issue: data.container.issue.clone(),
        first_page: data.container.first_page.clone(),
        last_page: data.container.last_page.clone(),
        date_published: data.date_published.clone(),
        date_created: data.dates.created.clone(),
        date_updated: data.date_updated.clone(),
        contributor_count: data.contributors.len() as i32,
        first_author_name,
        first_author_orcid,
        subjects,
        description: data.description.clone(),
        provider: data.provider.clone(),
        additional_type: data.additional_type.clone(),
        json,
    }
}

/// Write a list of commonmeta records as Parquet using the flattened
/// `CommonmetaRow` schema.
/// Records per Parquet row group. `flatten_row` is the CPU-heavy step here
/// (each row's `json` column is a full JSON serialization of the original
/// record), so it's parallelized across chunks of this size; the resulting
/// row groups are then written into the output sequentially, since Parquet
/// row-group data has to land in the underlying buffer in order. A single
/// file with multiple row groups is normal Parquet practice, not a
/// workaround — unlike writing one row group per output *file*, which is
/// what `cmd::list` used to do before merging this batching in here.
const ROW_GROUP_SIZE: usize = 100_000;

pub fn write_parquet_all(list: &[Data]) -> Result<Vec<u8>> {
    write_parquet_chunked(list, ROW_GROUP_SIZE)
}

/// `write_parquet_all`, parameterized over the row-group size so tests can
/// force multiple row groups without constructing 100,000+ records.
fn write_parquet_chunked(list: &[Data], row_group_size: usize) -> Result<Vec<u8>> {
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::record::RecordWriter;

    let chunks: Vec<&[Data]> = if list.is_empty() {
        vec![&[][..]]
    } else {
        list.chunks(row_group_size).collect()
    };

    let row_chunks: Vec<Vec<CommonmetaRow>> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || chunk.iter().map(flatten_row).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| Error::Serialize("parquet flatten thread panicked".to_string()))
            })
            .collect::<Result<Vec<_>>>()
    })?;

    let schema = row_chunks[0]
        .as_slice()
        .schema()
        .map_err(|e| Error::Serialize(e.to_string()))?;
    let props = std::sync::Arc::new(WriterProperties::builder().build());

    let buffer: Vec<u8> = Vec::new();
    let mut writer = SerializedFileWriter::new(buffer, schema, props)
        .map_err(|e| Error::Serialize(e.to_string()))?;

    for rows in &row_chunks {
        let mut row_group = writer
            .next_row_group()
            .map_err(|e| Error::Serialize(e.to_string()))?;
        rows.as_slice()
            .write_to_row_group(&mut row_group)
            .map_err(|e| Error::Serialize(e.to_string()))?;
        row_group
            .close()
            .map_err(|e| Error::Serialize(e.to_string()))?;
    }

    writer
        .into_inner()
        .map_err(|e| Error::Serialize(e.to_string()))
}

/// Reconstruct a `Data` record from a `CommonmetaRow`.
///
/// Prefers the `json` column, which holds the complete original record, so
/// the round trip through Parquet is lossless. Falls back to rebuilding from
/// the flattened columns (the inverse of `flatten_row`, lossy in the same
/// direction: only the fields captured there, e.g. the first author, are
/// restored) for Parquet files written before the `json` column existed, or
/// if it's somehow empty/invalid.
fn unflatten_row(row: &CommonmetaRow) -> Data {
    if !row.json.is_empty()
        && let Ok(data) = serde_json::from_str::<Data>(&row.json)
    {
        return data;
    }
    unflatten_row_lossy(row)
}

fn unflatten_row_lossy(row: &CommonmetaRow) -> Data {
    Data {
        id: row.id.clone(),
        type_: row.record_type.clone(),
        additional_type: row.additional_type.clone(),
        title: row.title.clone(),
        url: row.url.clone(),
        identifiers: if row.doi.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Identifier {
                identifier: row.doi.clone(),
                identifier_type: "DOI".to_string(),
            }]
        },
        publisher: crate::data::Publisher {
            name: row.publisher.clone(),
            ..Default::default()
        },
        language: row.language.clone(),
        version: row.version.clone(),
        license: crate::data::License {
            id: row.license.clone(),
            ..Default::default()
        },
        container: crate::data::Container {
            title: row.container_title.clone(),
            type_: row.container_type.clone(),
            volume: row.volume.clone(),
            issue: row.issue.clone(),
            first_page: row.first_page.clone(),
            last_page: row.last_page.clone(),
            ..Default::default()
        },
        date_published: row.date_published.clone(),
        date_updated: row.date_updated.clone(),
        dates: crate::data::Dates {
            created: row.date_created.clone(),
            ..Default::default()
        },
        contributors: if row.first_author_name.is_empty() && row.first_author_orcid.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Contributor::person(
                crate::data::Person {
                    id: row.first_author_orcid.clone(),
                    ..Default::default()
                },
                Vec::new(),
            )]
        },
        subjects: row
            .subjects
            .split("; ")
            .filter(|s| !s.is_empty())
            .map(|s| crate::data::Subject {
                subject: s.to_string(),
                ..Default::default()
            })
            .collect(),
        description: row.description.clone(),
        provider: row.provider.clone(),
        ..Default::default()
    }
}

const SQLITE_DDL: &str = r#"PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS works (
    "id"               TEXT PRIMARY KEY NOT NULL,
    "type"             TEXT NOT NULL DEFAULT '',
    "url"              TEXT NOT NULL DEFAULT '',
    "title"            TEXT NOT NULL DEFAULT '',
    "additional_titles" TEXT NOT NULL DEFAULT '[]',
    "contributors"     TEXT NOT NULL DEFAULT '[]',
    "date_published"   TEXT NOT NULL DEFAULT '',
    "date_updated"     TEXT NOT NULL DEFAULT '',
    "dates"            TEXT NOT NULL DEFAULT '{}',
    "publisher"        TEXT NOT NULL DEFAULT '{}',
    "container"        TEXT NOT NULL DEFAULT '{}',
    "description"      TEXT NOT NULL DEFAULT '',
    "license"          TEXT NOT NULL DEFAULT '{}',
    "version"          TEXT NOT NULL DEFAULT '',
    "language"         TEXT NOT NULL DEFAULT '',
    "subjects"         TEXT NOT NULL DEFAULT '[]',
    "identifiers"      TEXT NOT NULL DEFAULT '[]',
    "relations"        TEXT NOT NULL DEFAULT '[]',
    "references"       TEXT NOT NULL DEFAULT '[]',
    "funding_references" TEXT NOT NULL DEFAULT '[]',
    "geo_locations"    TEXT NOT NULL DEFAULT '[]',
    "files"            TEXT NOT NULL DEFAULT '[]',
    "archive_locations" TEXT NOT NULL DEFAULT '[]',
    "provider"         TEXT NOT NULL DEFAULT ''
);"#;

const SQLITE_INSERT: &str = r#"INSERT OR REPLACE INTO works (
    "id", "type", "url", "title", "additional_titles",
    "contributors", "date_published", "date_updated", "dates", "publisher",
    "container", "description", "license",
    "version", "language", "subjects", "identifiers", "relations", "references",
    "funding_references", "geo_locations", "files",
    "archive_locations", "provider"
) VALUES (
    ?1, ?2, ?3, ?4, ?5,
    ?6, ?7, ?8, ?9, ?10,
    ?11, ?12, ?13,
    ?14, ?15, ?16, ?17, ?18, ?19,
    ?20, ?21, ?22,
    ?23, ?24
)"#;

// ── Streaming-optimised write path ────────────────────────────────────────────

/// A single record fully prepared and JSON-serialized, ready to bind directly
/// to the SQLite INSERT statement without any further allocation.
pub struct PreparedRow {
    pub id: String,
    pub type_: String,
    pub url: String,
    pub title: String,
    pub additional_titles: String,
    pub contributors: String,
    pub date_published: String,
    pub date_updated: String,
    pub dates: String,
    pub publisher: String,
    pub container: String,
    pub description: String,
    pub license: String,
    pub version: String,
    pub language: String,
    pub subjects: String,
    pub identifiers: String,
    pub relations: String,
    pub references: String,
    pub funding_references: String,
    pub geo_locations: String,
    pub files: String,
    pub archive_locations: String,
    pub provider: String,
}

/// Apply v1.0 preparation (schema_version stamp, reference field stripping,
/// funder-ID validation) and serialize all complex fields to JSON strings,
/// consuming `data` so no clone is required.
pub fn serialize_to_row(mut data: Data) -> PreparedRow {
    for r in &mut data.references {
        r.publisher.clear();
        r.publication_year.clear();
        r.volume.clear();
        r.issue.clear();
        r.first_page.clear();
        r.last_page.clear();
        r.unstructured.clear();
        r.asserted_by.clear();
    }
    macro_rules! js {
        ($v:expr) => {
            serde_json::to_string(&$v).unwrap_or_default()
        };
    }
    PreparedRow {
        id: data.id,
        type_: data.type_,
        url: data.url,
        title: data.title,
        additional_titles: js!(data.additional_titles),
        contributors: js!(data.contributors),
        date_published: data.date_published,
        date_updated: data.date_updated,
        dates: js!(data.dates),
        publisher: js!(data.publisher),
        container: js!(data.container),
        description: data.description,
        license: js!(data.license),
        version: data.version,
        language: data.language,
        subjects: js!(data.subjects),
        identifiers: js!(data.identifiers),
        relations: js!(data.relations),
        references: js!(data.references),
        funding_references: js!(data.funding_references),
        geo_locations: js!(data.geo_locations),
        files: js!(data.files),
        archive_locations: js!(data.archive_locations),
        provider: data.provider,
    }
}

/// Open (or create) a SQLite3 database at `path` and initialise the `works`
/// table. When `overwrite` is true any existing file is deleted first (fresh
/// DB). When false the existing file is kept and the table is created only if
/// it does not exist yet — callers use `INSERT OR REPLACE` so rows with the
/// same `id` are updated in place.
pub(crate) async fn init_sqlite_writer_async(path: &Path, overwrite: bool) -> Result<libsql::Connection> {
    if overwrite && path.exists() {
        std::fs::remove_file(path)
            .map_err(|e| Error::Parse(format!("failed to remove '{}': {}", path.display(), e)))?;
    }
    let db = libsql::Builder::new_local(path)
        .build()
        .await
        .map_err(|e| Error::Parse(format!("failed to open sqlite '{}': {}", path.display(), e)))?;
    let conn = db
        .connect()
        .map_err(|e| Error::Parse(format!("failed to connect sqlite '{}': {}", path.display(), e)))?;
    conn.execute_batch(SQLITE_DDL)
        .await
        .map_err(|e| Error::Parse(format!("failed to create works table: {}", e)))?;
    Ok(conn)
}

/// Write pre-serialized rows in a single transaction. Takes ownership so no
/// cloning is needed — the caller (typically [`stream_dump_to_sqlite`]) already
/// produced the rows in parallel via [`serialize_to_row`].
pub(crate) async fn write_sqlite_batch_rows_async(
    conn: &libsql::Connection,
    rows: Vec<PreparedRow>,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let tx = conn
        .transaction()
        .await
        .map_err(|e| Error::Parse(format!("failed to begin transaction: {}", e)))?;
    for row in rows {
        let id_for_err = row.id.clone();
        tx.execute(
            SQLITE_INSERT,
            libsql::params![
                row.id, row.type_, row.url, row.title,
                row.additional_titles, row.contributors, row.date_published,
                row.date_updated, row.dates, row.publisher, row.container,
                row.description, row.license,
                row.version, row.language, row.subjects, row.identifiers,
                row.relations, row.references,
                row.funding_references, row.geo_locations, row.files,
                row.archive_locations, row.provider,
            ],
        )
        .await
        .map_err(|e| Error::Parse(format!("failed to insert '{}': {}", id_for_err, e)))?;
    }
    tx.commit()
        .await
        .map_err(|e| Error::Parse(format!("failed to commit transaction: {}", e)))?;
    Ok(())
}

/// Write `data` as a SQLite3 database at `path` with a `works` table whose
/// columns map 1:1 to the commonmeta v1.0 top-level fields. Simple string
/// fields are stored as TEXT; complex fields (objects, arrays) are stored as
/// compact JSON TEXT so every record round-trips losslessly.
/// Any existing file at `path` is deleted first.
pub fn write_sqlite(data: &[Data], path: &Path) -> Result<()> {
    write_sqlite_impl(data, path, true)
}

/// Like [`write_sqlite`] but opens an existing database instead of recreating
/// it. Rows whose `id` already exists are replaced; new rows are inserted.
pub fn upsert_sqlite(data: &[Data], path: &Path) -> Result<()> {
    write_sqlite_impl(data, path, false)
}

fn write_sqlite_impl(data: &[Data], path: &Path, overwrite: bool) -> Result<()> {
    let rows: Vec<PreparedRow> = data.iter().map(|d| serialize_to_row(d.clone())).collect();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let conn = init_sqlite_writer_async(path, overwrite).await?;
            write_sqlite_batch_rows_async(&conn, rows).await
        })
}

/// Return the total number of rows in the `works` table of a commonmeta SQLite
/// database. Used to report the cumulative count after an upsert.
pub fn count_sqlite_works(path: &Path) -> Result<usize> {
    let path = path.to_path_buf();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let db = libsql::Builder::new_local(&path)
                .build()
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;
            let conn = db.connect().map_err(|e| Error::Parse(e.to_string()))?;
            let mut rows = conn
                .query("SELECT COUNT(*) FROM works", ())
                .await
                .map_err(|e| Error::Parse(e.to_string()))?;
            let n: i64 = rows
                .next()
                .await
                .map_err(|e| Error::Parse(e.to_string()))?
                .ok_or_else(|| Error::Parse("empty COUNT result".into()))?
                .get(0)
                .map_err(|e| Error::Parse(e.to_string()))?;
            Ok(n.max(0) as usize)
        })
}

const SQLITE_SELECT: &str = r#"SELECT
    "id", "type", "url", "title", "additional_titles",
    "contributors", "date_published", "date_updated", "dates", "publisher",
    "container", "description", "license",
    "version", "language", "subjects", "identifiers", "relations", "references",
    "funding_references", "geo_locations", "files",
    "archive_locations", "provider"
FROM works ORDER BY rowid"#;

/// Inverse of `serialize_to_row`: deserialises all columns back to `Data`.
async fn read_sqlite_rows_async(
    conn: &libsql::Connection,
    limit: Option<usize>,
    offset: usize,
) -> Result<Vec<Data>> {
    let sql = match (limit, offset) {
        (Some(n), o) => format!("{} LIMIT {} OFFSET {}", SQLITE_SELECT, n, o),
        (None, o) if o > 0 => format!("{} LIMIT -1 OFFSET {}", SQLITE_SELECT, o),
        _ => SQLITE_SELECT.to_string(),
    };

    let mut rows = conn
        .query(&sql, ())
        .await
        .map_err(|e| Error::Parse(e.to_string()))?;

    // Helper: parse JSON TEXT column; returns Default if the string is empty or unparseable.
    fn col_json<T: serde::de::DeserializeOwned + Default>(s: String) -> T {
        if s.is_empty() {
            T::default()
        } else {
            serde_json::from_str(&s).unwrap_or_default()
        }
    }

    let mut results = Vec::new();
    while let Some(row) = rows.next().await.map_err(|e| Error::Parse(e.to_string()))? {
        macro_rules! s {
            ($i:literal) => {
                row.get::<String>($i).unwrap_or_default()
            };
        }
        let data = Data {
            id: s!(0),
            type_: s!(1),
            url: s!(2),
            title: s!(3),
            additional_titles: col_json(s!(4)),
            contributors: col_json(s!(5)),
            date_published: s!(6),
            date_updated: s!(7),
            dates: col_json(s!(8)),
            publisher: col_json(s!(9)),
            container: col_json(s!(10)),
            description: s!(11),
            license: col_json(s!(12)),
            version: s!(13),
            language: s!(14),
            subjects: col_json(s!(15)),
            identifiers: col_json(s!(16)),
            relations: col_json(s!(17)),
            references: col_json(s!(18)),
            funding_references: col_json(s!(19)),
            geo_locations: col_json(s!(20)),
            files: col_json(s!(21)),
            archive_locations: col_json(s!(22)),
            provider: s!(23),
            ..Data::default()
        };
        results.push(data);
    }
    Ok(results)
}

/// Read records from a commonmeta SQLite database written by [`write_sqlite`].
/// Pass `limit = None` to load all rows; `offset` can be used for pagination.
pub fn read_sqlite_commonmeta(path: &Path, limit: Option<usize>, offset: usize) -> Result<Vec<Data>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?
        .block_on(async {
            let db = libsql::Builder::new_local(path)
                .build()
                .await
                .map_err(|e| Error::Parse(format!("failed to open '{}': {}", path.display(), e)))?;
            let conn = db
                .connect()
                .map_err(|e| Error::Parse(format!("failed to connect '{}': {}", path.display(), e)))?;
            read_sqlite_rows_async(&conn, limit, offset).await
        })
}

/// Read a list of commonmeta records back from the `CommonmetaRow` Parquet
/// schema written by `write_parquet_all`. Lossless: each record is restored
/// from its `json` column, the complete original serialization.
pub fn read_parquet_all(bytes: &[u8]) -> Result<Vec<Data>> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::RecordReader;

    let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes.to_vec()))
        .map_err(|e| Error::Parse(e.to_string()))?;

    let mut rows: Vec<CommonmetaRow> = Vec::new();
    for i in 0..reader.num_row_groups() {
        let mut row_group_reader = reader
            .get_row_group(i)
            .map_err(|e| Error::Parse(e.to_string()))?;
        let num_rows = row_group_reader.metadata().num_rows() as usize;
        rows.read_from_row_group(&mut *row_group_reader, num_rows)
            .map_err(|e| Error::Parse(e.to_string()))?;
    }

    Ok(rows.iter().map(unflatten_row).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Contributor, Identifier, Person};

    fn sample_data() -> Data {
        Data {
            id: "https://doi.org/10.1234/abc".to_string(),
            type_: "JournalArticle".to_string(),
            title: "A Sample Title".to_string(),
            identifiers: vec![Identifier {
                identifier: "10.1234/abc".to_string(),
                identifier_type: "DOI".to_string(),
            }],
            contributors: vec![Contributor::person(
                Person {
                    given_name: "Jane".to_string(),
                    family_name: "Doe".to_string(),
                    id: "https://orcid.org/0000-0002-1825-0097".to_string(),
                    ..Default::default()
                },
                Vec::new(),
            )],
            ..Data::default()
        }
    }

    #[test]
    fn test_flatten_row_basic() {
        let row = flatten_row(&sample_data());
        assert_eq!(row.id, "https://doi.org/10.1234/abc");
        assert_eq!(row.record_type, "JournalArticle");
        assert_eq!(row.title, "A Sample Title");
        assert_eq!(row.doi, "10.1234/abc");
        assert_eq!(row.first_author_name, "Jane Doe");
        assert_eq!(
            row.first_author_orcid,
            "https://orcid.org/0000-0002-1825-0097"
        );
        assert_eq!(row.contributor_count, 1);
    }

    #[test]
    fn test_flatten_row_doi_fallback_from_id() {
        let mut data = sample_data();
        data.identifiers.clear();
        let row = flatten_row(&data);
        assert_eq!(row.doi, "https://doi.org/10.1234/abc");
    }

    #[test]
    fn test_write_parquet_all_roundtrip() {
        let list = vec![sample_data()];
        let bytes = write_parquet_all(&list).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
    }

    #[test]
    fn test_write_parquet_all_empty() {
        let list: Vec<Data> = vec![];
        let bytes = write_parquet_all(&list).unwrap();
        assert_eq!(&bytes[0..4], b"PAR1");
    }

    #[test]
    fn test_write_parquet_all_readable_schema_and_rows() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let list = vec![sample_data(), sample_data()];
        let bytes = write_parquet_all(&list).unwrap();

        let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes)).unwrap();
        let metadata = reader.metadata();
        assert_eq!(metadata.file_metadata().num_rows(), 2);

        let schema = metadata.file_metadata().schema_descr();
        let column_names: Vec<String> = (0..schema.num_columns())
            .map(|i| schema.column(i).name().to_string())
            .collect();
        assert!(column_names.iter().any(|c| c == "id"));
        assert!(column_names.iter().any(|c| c == "record_type"));
        assert!(column_names.iter().any(|c| c == "title"));
        assert!(column_names.iter().any(|c| c == "doi"));
        assert!(column_names.iter().any(|c| c == "first_author_name"));
    }

    #[test]
    fn test_write_parquet_chunked_uses_multiple_row_groups_in_one_file() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let list = vec![sample_data(), sample_data(), sample_data()];
        // row_group_size=1 forces 3 row groups without needing 100,000+ rows.
        let bytes = write_parquet_chunked(&list, 1).unwrap();

        let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes.clone())).unwrap();
        assert_eq!(reader.num_row_groups(), 3);
        assert_eq!(reader.metadata().file_metadata().num_rows(), 3);

        // A multi-row-group file is still a single, fully readable Parquet
        // file: read_parquet_all already loops over every row group.
        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert_eq!(roundtripped.len(), 3);
    }

    #[test]
    fn test_write_read_parquet_roundtrip() {
        let list = vec![sample_data()];
        let bytes = write_parquet_all(&list).unwrap();

        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert_eq!(roundtripped.len(), 1);
        // Lossless: the round-tripped record is byte-for-byte identical to
        // the original, not just the fields the flattened columns capture.
        assert_eq!(roundtripped[0], list[0]);
    }

    #[test]
    fn test_write_read_parquet_roundtrip_preserves_fields_outside_flattened_view() {
        use crate::data::{Affiliation, Description, Subject, Title};

        let mut data = sample_data();
        // Fields the old flattened-only reconstruction dropped: a second
        // title, a second contributor with affiliations, a second
        // identifier, and a second description.
        data.additional_titles.push(Title {
            title: "An Alternative Title".to_string(),
            type_: "TranslatedTitle".to_string(),
            ..Default::default()
        });
        data.contributors.push(Contributor::person(
            Person {
                given_name: "John".to_string(),
                family_name: "Smith".to_string(),
                affiliations: vec![Affiliation {
                    id: "https://ror.org/02catss52".to_string(),
                    name: "Example University".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            },
            Vec::new(),
        ));
        data.identifiers.push(Identifier {
            identifier: "1234-5678".to_string(),
            identifier_type: "ISSN".to_string(),
        });
        data.additional_descriptions.push(Description {
            description: "A second description".to_string(),
            type_: "TechnicalInfo".to_string(),
            ..Default::default()
        });
        data.subjects = vec![
            Subject {
                subject: "Biology".to_string(),
                ..Default::default()
            },
            Subject {
                subject: "Chemistry".to_string(),
                ..Default::default()
            },
        ];

        let bytes = write_parquet_all(&[data.clone()]).unwrap();
        let roundtripped = read_parquet_all(&bytes).unwrap();

        assert_eq!(roundtripped.len(), 1);
        assert_eq!(roundtripped[0], data);
        assert_eq!(roundtripped[0].additional_titles.len(), 1);
        assert_eq!(roundtripped[0].contributors.len(), 2);
        assert_eq!(
            roundtripped[0].contributors[1].affiliations()[0].name,
            "Example University"
        );
        assert_eq!(roundtripped[0].identifiers.len(), 2);
        assert_eq!(roundtripped[0].additional_descriptions.len(), 1);
        assert_eq!(roundtripped[0].subjects.len(), 2);
    }

    #[test]
    fn test_read_parquet_all_empty() {
        let bytes = write_parquet_all(&[]).unwrap();
        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert!(roundtripped.is_empty());
    }

    #[test]
    fn test_write_sqlite_creates_works_table() {
        let dir = std::env::temp_dir().join("commonmeta_sqlite_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.sqlite3");

        let list = vec![sample_data()];
        write_sqlite(&list, &path).unwrap();

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = libsql::Builder::new_local(&path).build().await.unwrap();
                let conn = db.connect().unwrap();

                let mut rows = conn.query("SELECT COUNT(*) FROM works", ()).await.unwrap();
                let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
                assert_eq!(count, 1);

                let mut rows = conn
                    .query(r#"SELECT "id", "title", "type" FROM works"#, ())
                    .await
                    .unwrap();
                let row = rows.next().await.unwrap().unwrap();
                let id: String = row.get(0).unwrap();
                let title: String = row.get(1).unwrap();
                let type_: String = row.get(2).unwrap();
                assert_eq!(id, "https://doi.org/10.1234/abc");
                assert_eq!(title, "A Sample Title");
                assert_eq!(type_, "JournalArticle");

                let mut rows = conn
                    .query("SELECT contributors FROM works", ())
                    .await
                    .unwrap();
                let contributors: String =
                    rows.next().await.unwrap().unwrap().get(0).unwrap();
                let parsed: serde_json::Value =
                    serde_json::from_str(&contributors).unwrap();
                assert!(parsed.is_array());
                assert_eq!(parsed.as_array().unwrap().len(), 1);
            });

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_sqlite_roundtrip_provider() {
        let dir = std::env::temp_dir().join("commonmeta_sqlite_test_sv");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.sqlite3");

        write_sqlite(&[sample_data()], &path).unwrap();

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = libsql::Builder::new_local(&path).build().await.unwrap();
                let conn = db.connect().unwrap();
                let mut rows = conn
                    .query("SELECT provider FROM works", ())
                    .await
                    .unwrap();
                let provider: String = rows.next().await.unwrap().unwrap().get(0).unwrap();
                assert_eq!(provider, sample_data().provider);
            });

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_sqlite_replaces_existing_file() {
        let dir = std::env::temp_dir().join("commonmeta_sqlite_test_replace");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.sqlite3");

        // Write twice with the same record — should still have 1 row.
        write_sqlite(&[sample_data()], &path).unwrap();
        write_sqlite(&[sample_data()], &path).unwrap();

        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = libsql::Builder::new_local(&path).build().await.unwrap();
                let conn = db.connect().unwrap();
                let mut rows = conn.query("SELECT COUNT(*) FROM works", ()).await.unwrap();
                let count: i64 = rows.next().await.unwrap().unwrap().get(0).unwrap();
                assert_eq!(count, 1);
            });

        std::fs::remove_dir_all(&dir).ok();
    }
}
