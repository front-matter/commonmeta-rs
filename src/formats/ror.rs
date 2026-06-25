#![allow(dead_code)]

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::data::{Data, Identifier, Relation};
use crate::error::{Error, Result};
use crate::utils::{normalize_ror, validate_id, validate_ror};

use crate::formats::ror_countries::ROR_COUNTRIES;

/// The live ROR API sometimes sends explicit JSON `null` for optional string
/// fields (e.g. `external_ids[].preferred`) rather than omitting them, which
/// a plain `String` field can't deserialize directly.
fn null_as_empty<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

// ── ROR API structs ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AdminEntry {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub date: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub schema_version: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Admin {
    #[serde(default)]
    pub created: AdminEntry,
    #[serde(default)]
    pub last_modified: AdminEntry,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Ror {
    #[serde(default)]
    pub admin: Option<Admin>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub id: String,
    #[serde(default)]
    pub established: Option<i32>,
    #[serde(default)]
    pub external_ids: Vec<ExternalId>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub locations: Vec<Location>,
    #[serde(default)]
    pub names: Vec<Name>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub status: String,
    #[serde(rename = "types", default)]
    pub types: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ExternalId {
    #[serde(rename = "type", default, deserialize_with = "null_as_empty")]
    pub type_: String,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub preferred: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Link {
    #[serde(rename = "type", default, deserialize_with = "null_as_empty")]
    pub type_: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub value: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Location {
    #[serde(default)]
    pub geonames_id: i64,
    #[serde(default)]
    pub geonames_details: GeonamesDetails,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GeonamesDetails {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub country_code: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub country_name: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub country_subdivision_code: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub country_subdivision_name: String,
    #[serde(default)]
    pub lat: f64,
    #[serde(default)]
    pub lng: f64,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Name {
    #[serde(default, deserialize_with = "null_as_empty")]
    pub value: String,
    #[serde(rename = "types", default)]
    pub types: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub lang: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Relationship {
    #[serde(rename = "type", default, deserialize_with = "null_as_empty")]
    pub type_: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub label: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub id: String,
}

// List response for query endpoint
#[derive(Debug, Deserialize)]
struct RorListResponse {
    #[serde(default)]
    number_of_results: i32,
    #[serde(default)]
    items: Vec<Ror>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Returns the display name from a ROR names list (type "ror_display").
pub fn get_display_name(ror: &Ror) -> String {
    ror.names
        .iter()
        .find(|n| n.types.iter().any(|t| t == "ror_display"))
        .map(|n| n.value.clone())
        .unwrap_or_default()
}

/// Returns the website URL (link type "website").
fn get_website_url(ror: &Ror) -> String {
    ror.links
        .iter()
        .find(|l| l.type_ == "website")
        .map(|l| l.value.clone())
        .unwrap_or_default()
}

/// Maps an external ID type from ROR to a commonmeta identifier type string.
fn external_id_type(ror_type: &str) -> &'static str {
    match ror_type {
        "GRID" => "GRID",
        "Wikidata" => "Wikidata",
        "FundRef" => "Crossref Funder ID",
        "ISNI" => "ISNI",
        _ => "",
    }
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn from_ror(ror: Ror) -> Data {
    // ID: normalize the ROR URL
    let id = normalize_ror(&ror.id);

    // Type: use first entry from `types`, defaulting to "Organization"
    let type_ = "Organization".to_string();

    // Title: display name (ror_display type in names list)
    let title = get_display_name(&ror);

    // URL: website link
    let url = get_website_url(&ror);

    // Date: established year
    let date_published = ror.established.map(|y| y.to_string()).unwrap_or_default();

    // Identifiers: from external_ids
    let identifiers: Vec<Identifier> = ror
        .external_ids
        .iter()
        .filter_map(|ext| {
            let id_type = external_id_type(&ext.type_);
            if id_type.is_empty() {
                return None;
            }
            // Use preferred if set, else first in all
            let value = if !ext.preferred.is_empty() {
                ext.preferred.clone()
            } else {
                ext.all.first().cloned().unwrap_or_default()
            };
            if value.is_empty() {
                return None;
            }
            Some(Identifier {
                identifier: value,
                identifier_type: id_type.to_string(),
                ..Default::default()
            })
        })
        .collect();

    // Relations: from relationships
    let relations: Vec<Relation> = ror
        .relationships
        .iter()
        .filter(|r| !r.id.is_empty())
        .map(|r| {
            let rel_type = match r.type_.as_str() {
                "parent" => "IsPartOf",
                "child" => "HasPart",
                "related" => "References",
                other => other,
            };
            Relation {
                id: normalize_ror(&r.id),
                type_: rel_type.to_string(),
                ..Default::default()
            }
        })
        .collect();

    Data {
        id,
        type_,
        url,
        title,
        date_published,
        identifiers,
        relations,
        provider: "ROR".to_string(),
        ..Data::default()
    }
}

// ── Writer output structs ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct OutInvenioRdm {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    acronym: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    country: String,
    identifiers: Vec<OutIdentifier>,
    title: HashMap<String, String>,
}

#[derive(Serialize)]
struct OutIdentifier {
    identifier: String,
    scheme: String,
}

/// Maps a commonmeta identifier type to an InvenioRDM scheme string.
fn cm_to_inveniordm_scheme(cm_type: &str) -> &'static str {
    match cm_type {
        "GRID" => "grid",
        "Wikidata" => "wikidata",
        "ISNI" => "isni",
        "Crossref Funder ID" => "fundref",
        _ => "",
    }
}

/// Maps a commonmeta identifier type to a ROR external_ids type string.
fn cm_to_ror_ext_type(cm_type: &str) -> &'static str {
    match cm_type {
        "GRID" => "GRID",
        "Wikidata" => "Wikidata",
        "ISNI" => "ISNI",
        "Crossref Funder ID" => "FundRef",
        _ => "",
    }
}

// ── Writers ───────────────────────────────────────────────────────────────────

/// Write Data as InvenioRDM organization vocabulary YAML.
pub fn write(data: &Data) -> Result<Vec<u8>> {
    let bare_id = validate_ror(&data.id)
        .ok_or_else(|| Error::InvalidId(format!("not a valid ROR ID: {}", data.id)))?;

    // Build multilingual title map from primary title
    let mut title: HashMap<String, String> = HashMap::new();
    if !data.title.is_empty() {
        let lang = if data.language.is_empty() { "en".to_string() } else { data.language.clone() };
        title.insert(lang, data.title.clone());
    }

    let name = data.title.clone();

    // Identifiers: ROR first, then external IDs
    let mut identifiers = vec![OutIdentifier {
        identifier: bare_id.clone(),
        scheme: "ror".to_string(),
    }];
    for id in &data.identifiers {
        let scheme = cm_to_inveniordm_scheme(&id.identifier_type);
        if !scheme.is_empty() {
            identifiers.push(OutIdentifier {
                identifier: id.identifier.clone(),
                scheme: scheme.to_string(),
            });
        }
    }

    let out = OutInvenioRdm {
        id: bare_id,
        name,
        acronym: String::new(),
        country: String::new(),
        identifiers,
        title,
    };

    serde_yaml::to_string(&out)
        .map(|s| s.into_bytes())
        .map_err(|e| Error::Serialize(e.to_string()))
}

/// Write Data as minimal ROR JSON.
fn convert_json(data: &Data) -> serde_json::Value {
    use serde_json::{Map, Value, json};

    let ror_id = normalize_ror(&data.id);
    let name = data.title.as_str();

    let mut names: Vec<Value> = Vec::new();
    if !name.is_empty() {
        let lang = if data.language.is_empty() { "en" } else { &data.language };
        names.push(json!({
            "value": name,
            "types": ["ror_display"],
            "lang": lang
        }));
    }

    let links: Vec<Value> = if !data.url.is_empty() {
        vec![json!({"type": "website", "value": data.url})]
    } else {
        vec![]
    };

    let external_ids: Vec<Value> = data
        .identifiers
        .iter()
        .filter_map(|id| {
            let ext_type = cm_to_ror_ext_type(&id.identifier_type);
            if ext_type.is_empty() {
                return None;
            }
            Some(json!({
                "type": ext_type,
                "all": [id.identifier],
                "preferred": id.identifier
            }))
        })
        .collect();

    let relationships: Vec<Value> = data
        .relations
        .iter()
        .map(|r| {
            let rel_type = match r.type_.as_str() {
                "IsPartOf" => "parent",
                "HasPart" => "child",
                _ => "related",
            };
            json!({"type": rel_type, "id": r.id, "label": ""})
        })
        .collect();

    let mut obj = Map::new();
    obj.insert("id".to_string(), Value::String(ror_id));
    obj.insert("names".to_string(), Value::Array(names));
    obj.insert("links".to_string(), Value::Array(links));
    obj.insert("external_ids".to_string(), Value::Array(external_ids));
    obj.insert("relationships".to_string(), Value::Array(relationships));
    obj.insert("status".to_string(), Value::String("active".to_string()));
    obj.insert("types".to_string(), Value::Array(vec![]));
    if let Ok(year) = data.date_published.parse::<i64>() {
        obj.insert("established".to_string(), Value::Number(year.into()));
    }

    Value::Object(obj)
}

pub fn write_json(data: &Data) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(&convert_json(data)).map_err(|e| Error::Serialize(e.to_string()))
}

pub fn write_json_all(list: &[Data]) -> Result<Vec<u8>> {
    let values: Vec<serde_json::Value> = list.iter().map(convert_json).collect();
    serde_json::to_vec_pretty(&values).map_err(|e| Error::Serialize(e.to_string()))
}

// ── Bulk writers (catalog dumps) ──────────────────────────────────────────────
//
// These operate directly on the raw `Ror` API struct (not the lossy `Data`
// model) since formats like CSV/Parquet need fields (locations, raw
// external_ids) that the commonmeta `Data` model does not retain.
// Mirrors Go's `WriteAll` / `ConvertRORCSV` in ror/writer.go.

/// A flattened, lossy CSV/Parquet-friendly view of a ROR record.
/// Mirrors Go's `RORCSV` struct.
#[derive(Debug, Default, Clone, Serialize, Deserialize, parquet_derive::ParquetRecordWriter)]
pub struct RorCsv {
    pub id: String,
    pub name: String,
    pub types: String,
    pub status: String,
    pub links: String,
    pub aliases: String,
    pub labels: String,
    pub acronyms: String,
    pub wikipedia_url: String,
    pub established: String,
    pub latitude: String,
    pub longitude: String,
    pub place: String,
    pub geonames_id: String,
    pub country_subdivision_name: String,
    pub country_subdivision_code: String,
    pub country_code: String,
    pub country_name: String,
    pub external_ids_grid_preferred: String,
    pub external_ids_grid_all: String,
    pub external_ids_isni_preferred: String,
    pub external_ids_isni_all: String,
    pub external_ids_fundref_preferred: String,
    pub external_ids_fundref_all: String,
    pub external_ids_wikidata_preferred: String,
    pub external_ids_wikidata_all: String,
    pub relationships: String,
}

/// Convert a raw ROR record into its flattened CSV/Parquet representation.
/// Mirrors Go's `ConvertRORCSV`.
pub fn convert_ror_csv(ror: &Ror) -> RorCsv {
    let mut out = RorCsv {
        id: ror.id.clone(),
        status: ror.status.clone(),
        ..Default::default()
    };

    let mut acronyms = Vec::new();
    let mut aliases = Vec::new();
    let mut labels = Vec::new();

    for name in &ror.names {
        if name.types.iter().any(|t| t == "ror_display") {
            out.name = name.value.clone();
        } else if name.types.iter().any(|t| t == "acronym") && !name.value.is_empty() {
            acronyms.push(name.value.clone());
        } else if name.types.iter().any(|t| t == "alias") {
            aliases.push(name.value.clone());
        } else if name.types.iter().any(|t| t == "label") {
            if !name.lang.is_empty() {
                labels.push(format!("{}: {}", name.lang, name.value));
            } else {
                labels.push(name.value.clone());
            }
        }
    }

    // Compact: drop consecutive duplicate types, matching Go's slices.Compact
    let mut compacted_types: Vec<&str> = Vec::new();
    for t in &ror.types {
        if compacted_types
            .last()
            .map(|last| *last != t.as_str())
            .unwrap_or(true)
        {
            compacted_types.push(t.as_str());
        }
    }
    out.types = compacted_types.join("; ");

    for link in &ror.links {
        if link.type_ == "website" {
            out.links = link.value.clone();
        } else if link.type_ == "wikipedia" {
            out.wikipedia_url = link.value.clone();
        }
    }

    out.aliases = aliases.join("; ");
    out.labels = labels.join("; ");
    out.acronyms = acronyms.join("; ");

    if let Some(year) = ror.established
        && year != 0
    {
        out.established = year.to_string();
    }

    if let Some(loc) = ror.locations.first() {
        out.latitude = format!("{:.6}", loc.geonames_details.lat);
        out.longitude = format!("{:.6}", loc.geonames_details.lng);
        out.place = loc.geonames_details.name.clone();
        out.geonames_id = loc.geonames_id.to_string();
        out.country_subdivision_name = loc.geonames_details.country_subdivision_name.clone();
        out.country_subdivision_code = loc.geonames_details.country_subdivision_code.clone();
        out.country_code = loc.geonames_details.country_code.clone();
        out.country_name = loc.geonames_details.country_name.clone();
    }

    for ext in &ror.external_ids {
        match ext.type_.to_lowercase().as_str() {
            "grid" => {
                out.external_ids_grid_preferred = ext.preferred.clone();
                out.external_ids_grid_all = ext.all.join(";");
            }
            "isni" => {
                out.external_ids_isni_preferred = ext.preferred.clone();
                out.external_ids_isni_all = ext.all.join(";");
            }
            "fundref" => {
                out.external_ids_fundref_preferred = ext.preferred.clone();
                out.external_ids_fundref_all = ext.all.join(";");
            }
            "wikidata" => {
                out.external_ids_wikidata_preferred = ext.preferred.clone();
                out.external_ids_wikidata_all = ext.all.join(";");
            }
            _ => {}
        }
    }

    let mut child = Vec::new();
    let mut parent = Vec::new();
    let mut related = Vec::new();
    for rel in &ror.relationships {
        match rel.type_.as_str() {
            "child" => child.push(rel.id.clone()),
            "parent" => parent.push(rel.id.clone()),
            "related" => related.push(rel.id.clone()),
            _ => {}
        }
    }
    let mut groups = Vec::new();
    if !child.is_empty() {
        groups.push(format!("Child: {}", child.join(", ")));
    }
    if !parent.is_empty() {
        groups.push(format!("Parent: {}", parent.join(", ")));
    }
    if !related.is_empty() {
        groups.push(format!("Related: {}", related.join(", ")));
    }
    // Go's WriteAll concatenates these groups with no separator; we join with
    // "; " for readability since that appears to be an oversight upstream.
    out.relationships = groups.join("; ");

    out
}

/// Write a list of ROR records as CSV using the flattened `RorCsv` schema.
pub fn write_csv(list: &[Ror]) -> Result<Vec<u8>> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    for ror in list {
        writer
            .serialize(convert_ror_csv(ror))
            .map_err(|e| Error::Serialize(e.to_string()))?;
    }
    writer
        .into_inner()
        .map_err(|e| Error::Serialize(e.to_string()))
}

/// Write a list of ROR records as Parquet using the flattened `RorCsv` schema.
pub fn write_parquet(list: &[Ror]) -> Result<Vec<u8>> {
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::record::RecordWriter;

    let rows: Vec<RorCsv> = list.iter().map(convert_ror_csv).collect();
    let schema = rows
        .as_slice()
        .schema()
        .map_err(|e| Error::Serialize(e.to_string()))?;
    let props = std::sync::Arc::new(WriterProperties::builder().build());

    let buffer: Vec<u8> = Vec::new();
    let mut writer = SerializedFileWriter::new(buffer, schema, props)
        .map_err(|e| Error::Serialize(e.to_string()))?;

    let mut row_group = writer
        .next_row_group()
        .map_err(|e| Error::Serialize(e.to_string()))?;
    rows.as_slice()
        .write_to_row_group(&mut row_group)
        .map_err(|e| Error::Serialize(e.to_string()))?;
    row_group
        .close()
        .map_err(|e| Error::Serialize(e.to_string()))?;

    writer
        .into_inner()
        .map_err(|e| Error::Serialize(e.to_string()))
}

const ROR_SQLITE_DDL: &str = r#"PRAGMA synchronous=NORMAL;
CREATE TABLE IF NOT EXISTS settings (
    "key"   TEXT PRIMARY KEY NOT NULL,
    "value" TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS organizations (
    "id"           TEXT PRIMARY KEY NOT NULL,
    "name"         TEXT NOT NULL DEFAULT '',
    "status"       TEXT NOT NULL DEFAULT 'active',
    "types"        TEXT NOT NULL DEFAULT '[]',
    "locations"    TEXT NOT NULL DEFAULT '[]',
    "names"        TEXT NOT NULL DEFAULT '[]',
    "external_ids" TEXT NOT NULL DEFAULT '[]',
    "date_updated" TEXT NOT NULL DEFAULT '',
    "names_flat"   TEXT NOT NULL DEFAULT '',
    "metadata"     BLOB NOT NULL DEFAULT x''
);
CREATE INDEX IF NOT EXISTS organizations_status ON organizations("status");
CREATE INDEX IF NOT EXISTS organizations_date_updated ON organizations("date_updated");"#;

// FTS5 virtual table — created as a content table so the full text lives in
// `organizations` and FTS5 only stores the inverted index. Rebuilt in one
// bulk pass after all rows are inserted (much faster than per-row Tantivy).
const ROR_SQLITE_FTS5_DDL: &str =
    "CREATE VIRTUAL TABLE organizations_fts USING fts5(\
        name, names_flat, \
        content=\"organizations\", \
        content_rowid=\"rowid\", \
        tokenize=\"unicode61 remove_diacritics 1\"\
    )";

const ROR_SQLITE_INSERT: &str = r#"INSERT OR REPLACE INTO organizations (
    "id", "name", "status", "types", "locations", "names", "external_ids",
    "date_updated", "names_flat", "metadata"
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#;

struct RorRow {
    id: String,
    name: String,
    names_flat: String,
    status: String,
    types: String,
    locations: String,
    names: String,
    external_ids: String,
    date_updated: String,
    metadata: Vec<u8>,
}

fn serialize_ror_to_row(ror: &Ror) -> RorRow {
    let name = get_display_name(ror);
    let names_flat = ror
        .names
        .iter()
        .map(|n| n.value.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let types = serde_json::to_string(&ror.types).unwrap_or_default();
    let locations = serde_json::to_string(&ror.locations).unwrap_or_default();
    let names = serde_json::to_string(&ror.names).unwrap_or_default();
    let external_ids = serde_json::to_string(&ror.external_ids).unwrap_or_default();
    let date_updated = ror
        .admin
        .as_ref()
        .map(|a| a.last_modified.date.clone())
        .unwrap_or_default();
    let json = serde_json::to_string(ror).unwrap_or_default();
    let metadata =
        zstd::encode_all(json.as_bytes(), 0).unwrap_or_else(|_| json.into_bytes());
    RorRow {
        id: ror.id.clone(),
        name,
        names_flat,
        status: ror.status.clone(),
        types,
        locations,
        names,
        external_ids,
        date_updated,
        metadata,
    }
}

/// Write ROR records into the `organizations` table of the SQLite3 database at
/// `path`. The database is created if it does not exist; if it does exist (e.g.
/// it already contains a `works` table), only the `organizations` and
/// `organizations_fts` tables are replaced — other tables are untouched.
///
/// The `metadata` column stores the complete ROR JSON as a zstd-compressed BLOB
/// for lossless round-trips; the other columns hold denormalized lookup fields.
pub fn write_sqlite(list: &[Ror], path: &Path, version: Option<&str>, date: Option<&str>) -> Result<()> {
    use rusqlite::{params, Connection};

    // Create the parent directory if needed (e.g. first-time install).
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Parse(format!(
                    "failed to create directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }
    }

    let conn = Connection::open(path)
        .map_err(|e| Error::Parse(format!("failed to open sqlite '{}': {}", path.display(), e)))?;
    let _: String = conn.query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| Error::Parse(format!("failed to set WAL mode: {}", e)))?;
    conn.execute_batch(ROR_SQLITE_DDL)
        .map_err(|e| Error::Parse(format!("failed to create organizations table: {}", e)))?;
    // Migrate: add names_flat column if missing (pre-existing databases).
    let _ = conn.execute(
        "ALTER TABLE organizations ADD COLUMN \"names_flat\" TEXT NOT NULL DEFAULT ''",
        [],
    );
    // Clear version so a crash mid-install forces a re-run.
    let _ = conn.execute(
        "DELETE FROM settings WHERE key IN ('ror_version', 'ror_date')",
        [],
    );
    // Drop FTS5 virtual table before re-inserting (reinstall path).
    let _ = conn.execute("DROP TABLE IF EXISTS organizations_fts", []);
    // Plain BTree delete — no FTS active, so no per-row index overhead.
    conn.execute("DELETE FROM organizations", [])
        .map_err(|e| Error::Parse(format!("failed to clear organizations: {}", e)))?;

    // Bulk insert all rows in a single transaction with a prepared statement.
    // The statement is compiled once and reused for every row.
    let rows: Vec<RorRow> = list.iter().map(serialize_ror_to_row).collect();
    let bar = crate::progress::count_bar("writing", rows.len() as u64);
    {
        let tx = conn.unchecked_transaction()
            .map_err(|e| Error::Parse(format!("failed to begin transaction: {}", e)))?;
        {
            let mut stmt = tx.prepare(ROR_SQLITE_INSERT)
                .map_err(|e| Error::Parse(format!("failed to prepare insert: {}", e)))?;
            for row in &rows {
                stmt.execute(params![
                    row.id,
                    row.name,
                    row.status,
                    row.types,
                    row.locations,
                    row.names,
                    row.external_ids,
                    row.date_updated,
                    row.names_flat,
                    row.metadata,
                ])
                .map_err(|e| Error::Parse(format!("failed to insert organization: {}", e)))?;
                bar.inc(1);
            }
        }
        if let Some(v) = version {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('ror_version', ?1)",
                params![v],
            )
            .map_err(|e| Error::Parse(format!("failed to store ror_version: {}", e)))?;
        }
        if let Some(d) = date {
            tx.execute(
                "INSERT OR REPLACE INTO settings (key, value) VALUES ('ror_date', ?1)",
                params![d],
            )
            .map_err(|e| Error::Parse(format!("failed to store ror_date: {}", e)))?;
        }
        tx.commit()
            .map_err(|e| Error::Parse(format!("failed to commit transaction: {}", e)))?;
    }
    bar.finish_and_clear();

    // Build FTS5 index in one bulk pass over the content table.
    eprintln!("Building FTS index...");
    conn.execute_batch(ROR_SQLITE_FTS5_DDL)
        .map_err(|e| Error::Parse(format!("failed to create FTS5 table: {}", e)))?;
    conn.execute(
        "INSERT INTO organizations_fts(organizations_fts) VALUES('rebuild')",
        [],
    )
    .map_err(|e| Error::Parse(format!("failed to rebuild FTS5 index: {}", e)))?;
    let _ = conn.execute("PRAGMA wal_checkpoint(PASSIVE)", []);
    Ok(())
}

/// Read the ROR version stored in the local database, or `None` if the
/// database does not exist or no version has been recorded yet.
pub fn fetch_installed_ror_version(db_path: &Path) -> Result<Option<String>> {
    use rusqlite::{Connection, Error as SqliteError};

    if !db_path.exists() {
        return Ok(None);
    }
    let conn = Connection::open(db_path)
        .map_err(|e| Error::Parse(format!("failed to open sqlite: {}", e)))?;
    // The settings table may not exist in older databases — treat that as "no version recorded".
    match conn.query_row(
        "SELECT value FROM settings WHERE key = 'ror_version' LIMIT 1",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(v) => Ok(Some(v)),
        Err(SqliteError::QueryReturnedNoRows) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// Write a list of ROR records, dispatching by file extension
/// (".json", ".yaml", ".jsonl", ".csv", ".parquet"). Mirrors Go's `WriteAll`.
pub fn write_all(list: &[Ror], extension: &str) -> Result<Vec<u8>> {
    match extension {
        ".yaml" => serde_yaml::to_string(list)
            .map(|s| s.into_bytes())
            .map_err(|e| Error::Serialize(e.to_string())),
        ".json" => serde_json::to_vec(list).map_err(|e| Error::Serialize(e.to_string())),
        ".jsonl" => {
            let mut out = Vec::new();
            for item in list {
                serde_json::to_writer(&mut out, item)
                    .map_err(|e| Error::Serialize(e.to_string()))?;
                out.push(b'\n');
            }
            Ok(out)
        }
        ".csv" => write_csv(list),
        ".parquet" => write_parquet(list),
        ".sqlite3" => {
            let tmp = std::env::temp_dir()
                .join(format!("ror-{}.sqlite3", std::process::id()));
            write_sqlite(list, &tmp, None, None)?;
            let bytes = std::fs::read(&tmp)
                .map_err(|e| Error::Serialize(format!("failed to read temp sqlite: {}", e)))?;
            let _ = std::fs::remove_file(&tmp);
            Ok(bytes)
        }
        other => Err(Error::UnsupportedFormat(other.to_string())),
    }
}

// ── Matching utilities ────────────────────────────────────────────────────────

const SPECIAL_CHARS: &str = r"[+\-=|><!()\\\{\}\[\]^~*?:/.,;]";

/// Remove special search characters, postal codes (5 digits), and normalise whitespace.
pub fn clean_search_string(s: &str) -> String {
    // Replace special characters with spaces
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(
            ch,
            '+' | '-'
                | '='
                | '|'
                | '>'
                | '<'
                | '!'
                | '('
                | ')'
                | '\\'
                | '{'
                | '}'
                | '['
                | ']'
                | '^'
                | '"'
                | '~'
                | '*'
                | '?'
                | ':'
                | '/'
                | '.'
                | ','
                | ';'
        ) {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    // Remove 5-digit postal codes
    let mut result = String::with_capacity(out.len());
    let mut chars = out.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            let mut digits = String::new();
            digits.push(ch);
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    digits.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if digits.len() != 5 {
                result.push_str(&digits);
            } else {
                result.push(' ');
            }
        } else {
            result.push(ch);
        }
    }
    // Collapse whitespace
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Map a country code to its ROR region group.
pub fn to_region(code: &str) -> &'static str {
    match code {
        "GB" | "UK" => "GB-UK",
        "CN" | "HK" | "TW" => "CN-HK-TW",
        "PR" | "US" => "US-PR",
        _ => {
            // Return the code itself via a static string fallback.
            // For non-special codes we just return the input reference.
            // Since we need &'static str, callers that need the code itself
            // can use the code directly.
            ""
        }
    }
}

/// Extract ISO-3166-1 alpha-2 country codes from a free-text affiliation string.
///
/// Strategy for matching each ROR country name variant:
/// - 2-char names: must appear as an isolated uppercase token in the input
/// - single-word names: must appear as an isolated lowercase token
/// - multi-word names: must appear as a lowercase substring
pub fn get_country_codes(s: &str) -> Vec<String> {
    let lower: String = s.to_lowercase();

    // Lowercase alpha-only version for single-word matching
    let alpha_lower: String = lower
        .chars()
        .map(|c| if c.is_ascii_alphabetic() { c } else { ' ' })
        .collect();
    let alpha_lower = alpha_lower.split_whitespace().collect::<Vec<_>>().join(" ");
    let alpha_tokens: Vec<&str> = alpha_lower.split_whitespace().collect();

    // Original tokens for 2-letter code matching
    let orig_tokens: Vec<String> = s
        .split_whitespace()
        .map(|t| {
            t.chars()
                .filter(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_uppercase()
        })
        .filter(|t| !t.is_empty())
        .collect();

    let mut codes: HashSet<String> = HashSet::new();

    'outer: for &(code, names) in ROR_COUNTRIES {
        for &name in names {
            let name_chars: Vec<char> = name.chars().collect();
            let name_len = name_chars.len();

            let matched = if name_len == 2 && name.chars().all(|c| c.is_ascii_alphabetic()) {
                // 2-letter code: exact isolated uppercase token match
                orig_tokens.iter().any(|t| t == &name.to_uppercase())
            } else {
                // Check for non-alphabetic chars → substring match in full lowercase
                let has_special = name.chars().any(|c| !c.is_ascii_alphabetic() && c != ' ');
                if has_special {
                    lower.contains(name)
                } else if name.contains(' ') {
                    // Multi-word: substring match in alpha-only lowercase
                    alpha_lower.contains(name)
                } else {
                    // Single-word: isolated token match
                    alpha_tokens.contains(&name)
                }
            };

            if matched {
                codes.insert(code.to_string());
                continue 'outer;
            }
        }
    }

    let mut result: Vec<String> = codes.into_iter().collect();
    result.sort();
    result
}

/// Return region codes for country codes found in `s`.
pub fn get_countries(s: &str) -> Vec<String> {
    let codes = get_country_codes(s);
    let mut regions: HashSet<String> = HashSet::new();
    for code in &codes {
        let region = to_region(code);
        if region.is_empty() {
            regions.insert(code.clone());
        } else {
            regions.insert(region.to_string());
        }
    }
    let mut result: Vec<String> = regions.into_iter().collect();
    result.sort();
    result
}

// ── Affiliation matching via ROR API ──────────────────────────────────────────

/// A single match result from the ROR affiliation API.
#[derive(Debug, Deserialize)]
pub struct AffiliationMatch {
    /// The substring of the input that matched.
    pub substring: String,
    /// Confidence score (0–1).
    pub score: f64,
    /// Matching strategy used (e.g. "PHRASE", "COMMON TERMS", "FUZZY").
    pub matching_type: String,
    /// Whether this match was selected as the best result.
    pub chosen: bool,
    /// The matched ROR organization, converted to `Data`.
    #[serde(skip)]
    pub organization: Data,
    #[serde(rename = "organization")]
    organization_raw: Ror,
}

#[derive(Debug, Deserialize)]
struct AffiliationResponse {
    #[serde(default)]
    number_of_results: i32,
    #[serde(default)]
    items: Vec<AffiliationMatch>,
}

/// Match a free-text affiliation string against ROR organizations using the
/// ROR v2 affiliation endpoint.
///
/// The input is cleaned with `clean_search_string` before querying.
/// Returns matches sorted by score descending; `chosen` is set on the best result.
pub fn match_affiliation(affiliation: &str) -> Result<Vec<AffiliationMatch>> {
    let cleaned = clean_search_string(affiliation);
    if cleaned.is_empty() {
        return Ok(vec![]);
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let encoded: String = url::form_urlencoded::byte_serialize(cleaned.as_bytes()).collect();
    let api_url = format!(
        "https://api.ror.org/v2/organizations?affiliation={}",
        encoded
    );

    let text = client
        .get(&api_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;

    let mut resp: AffiliationResponse =
        serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;

    // Convert the raw Ror struct to Data for each item
    for item in &mut resp.items {
        let ror = std::mem::take(&mut item.organization_raw);
        item.organization = from_ror(ror);
    }

    Ok(resp.items)
}

/// Match a free-text affiliation string against a local ROR SQLite database
/// (produced by [`write_sqlite`]). Uses Turso's Tantivy-backed FTS index for
/// full-text matching across all organization name variants (`name` + `names_flat`).
///
/// Results are returned in relevance order (best match first) with `chosen`
/// set on the top result.
pub fn match_affiliation_sqlite(affiliation: &str, db_path: &Path) -> Result<Vec<AffiliationMatch>> {
    let cleaned = clean_search_string(affiliation);
    if cleaned.is_empty() {
        return Ok(vec![]);
    }

    // FTS5 implicit AND: space-separated terms all must appear in a document.
    // Strip leading/trailing punctuation so FTS5 special chars (", *, ^) don't
    // leak into the query string.
    let fts_query = cleaned
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    if fts_query.is_empty() {
        return Ok(vec![]);
    }

    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| Error::Parse(format!("failed to open sqlite: {}", e)))?;

    // FTS5 content-table JOIN: results are ranked by BM25 relevance.
    // Only active orgs are returned; limit to 10 results.
    let mut stmt = conn
        .prepare(
            "SELECT o.id, o.name, o.metadata \
             FROM organizations_fts \
             JOIN organizations AS o ON o.rowid = organizations_fts.rowid \
             WHERE organizations_fts MATCH ?1 \
               AND o.status = 'active' \
             ORDER BY organizations_fts.rank \
             LIMIT 10",
        )
        .map_err(|e| Error::Parse(format!("failed to prepare FTS query: {}", e)))?;

    let mut matches: Vec<AffiliationMatch> = Vec::new();
    let mut rows = stmt
        .query(rusqlite::params![fts_query])
        .map_err(|e| Error::Parse(format!("FTS query failed: {}", e)))?;

    while let Some(row) = rows.next().map_err(|e| Error::Parse(e.to_string()))? {
        let id: String = row.get(0).map_err(|e| Error::Parse(format!("read id: {}", e)))?;
        let name: String = row.get(1).map_err(|e| Error::Parse(format!("read name: {}", e)))?;
        let blob: Vec<u8> = row.get(2).map_err(|e| Error::Parse(format!("read metadata '{}': {}", id, e)))?;
        let decompressed = zstd::decode_all(std::io::Cursor::new(&blob))
            .map_err(|e| Error::Parse(format!("decompress '{}': {}", id, e)))?;
        let ror: Ror = serde_json::from_slice(&decompressed)
            .map_err(|e| Error::Parse(format!("deserialize '{}': {}", id, e)))?;
        let organization = from_ror(ror);
        matches.push(AffiliationMatch {
            substring: name,
            score: 0.0,
            matching_type: "LOCAL".to_string(),
            chosen: false,
            organization,
            organization_raw: Ror::default(),
        });
    }

    let n = matches.len();
    for (i, m) in matches.iter_mut().enumerate() {
        m.score = if n <= 1 { 1.0 } else { 1.0 - (i as f64 / n as f64) };
        m.chosen = i == 0;
    }

    Ok(matches)
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let ror: Ror = serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_ror(ror))
}

/// Fetch an organization from the ROR API by ROR ID or other organization identifier.
pub fn fetch(input: &str) -> Result<Data> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let (id, id_type) = validate_id(input);

    let ror = if id_type == "ROR" {
        // Direct lookup by ROR ID
        let ror_id = validate_ror(&id).unwrap_or(id.clone());
        let api_url = format!("https://api.ror.org/v2/organizations/{}", ror_id);
        let text = client
            .get(&api_url)
            .send()
            .map_err(|e| Error::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Http(e.to_string()))?
            .text()
            .map_err(|e| Error::Http(e.to_string()))?;
        serde_json::from_str::<Ror>(&text).map_err(|e| Error::Parse(e.to_string()))?
    } else {
        // Query by other org identifier (Crossref Funder ID, GRID, Wikidata, ISNI)
        let org_types = ["ROR", "Crossref Funder ID", "GRID", "Wikidata", "ISNI"];
        if !org_types.contains(&id_type) {
            return Err(Error::Parse(format!(
                "Not a supported organization identifier: {}",
                input
            )));
        }
        let encoded: String = url::form_urlencoded::byte_serialize(id.as_bytes()).collect();
        let api_url = format!("https://api.ror.org/v2/organizations?query={}", encoded);
        let text = client
            .get(&api_url)
            .send()
            .map_err(|e| Error::Http(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Http(e.to_string()))?
            .text()
            .map_err(|e| Error::Http(e.to_string()))?;
        let list: RorListResponse =
            serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        if list.number_of_results != 1 {
            return Err(Error::Parse(format!(
                "Expected 1 result from ROR query, got {}",
                list.number_of_results
            )));
        }
        list.items
            .into_iter()
            .next()
            .ok_or_else(|| Error::Parse("No items in ROR query response".to_string()))?
    };

    Ok(from_ror(ror))
}

/// Look up a ROR organization by its full URL (e.g. `https://ror.org/012xzy7a9`)
/// from a local SQLite database written by [`write_sqlite`]. Returns the record
/// converted to `Data`, or an error when the ID is not found.
pub fn fetch_sqlite(id: &str, db_path: &Path) -> Result<Data> {
    use rusqlite::{params, Connection};

    let conn = Connection::open(db_path)
        .map_err(|e| Error::Parse(format!("failed to open sqlite '{}': {}", db_path.display(), e)))?;
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT metadata FROM organizations WHERE id = ?1 LIMIT 1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                Error::Parse(format!("organization '{}' not found in sqlite", id))
            } else {
                Error::Parse(format!("sqlite query failed: {}", e))
            }
        })?;
    let decompressed = zstd::decode_all(std::io::Cursor::new(&blob))
        .map_err(|e| Error::Parse(format!("decompress metadata: {}", e)))?;
    let ror: Ror = serde_json::from_slice(&decompressed)
        .map_err(|e| Error::Parse(format!("deserialize: {}", e)))?;
    Ok(from_ror(ror))
}

// ── Zenodo release helpers ────────────────────────────────────────────────────

/// Concept record ID for the ROR data archive on Zenodo.
/// DOI: `10.5281/zenodo.6347574` always resolves to the latest version.
const ROR_ZENODO_CONCEPT_ID: &str = "6347574";

/// Metadata about a ROR data release published on Zenodo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RorRelease {
    /// ROR version tag, e.g. `"v2.9"`.
    pub version: String,
    /// Release date in ISO 8601 format, e.g. `"2026-06-23"`.
    pub date: String,
    /// Zenodo record ID for this specific version, e.g. `"20818161"`.
    pub zenodo_id: String,
    /// Filename of the zip archive, e.g. `"v2.9-2026-06-23-ror-data.zip"`.
    pub filename: String,
    /// Direct download URL for the zip archive.
    pub download_url: String,
}

// Minimal serde structs for the Zenodo API response.
#[derive(Deserialize)]
struct ZenodoRecord {
    id: u64,
    metadata: ZenodoMeta,
    files: Vec<ZenodoFile>,
}

#[derive(Deserialize)]
struct ZenodoMeta {
    version: String,
    publication_date: String,
}

#[derive(Deserialize)]
struct ZenodoFile {
    key: String,
    links: ZenodoFileLinks,
}

#[derive(Deserialize)]
struct ZenodoFileLinks {
    #[serde(rename = "self")]
    self_: String,
}

/// Fetch metadata for the latest ROR data release from Zenodo without
/// downloading the full archive. Uses the Zenodo `/versions/latest` endpoint
/// against the ROR concept record (DOI `10.5281/zenodo.6347574`).
///
/// Returns a [`RorRelease`] containing the version tag, release date, Zenodo
/// record ID, zip filename, and direct download URL.
pub fn fetch_latest_ror_release() -> Result<RorRelease> {
    let url = format!(
        "https://zenodo.org/api/records/{}/versions/latest",
        ROR_ZENODO_CONCEPT_ID
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let text = client
        .get(&url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;

    let record: ZenodoRecord =
        serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;

    // Pick the zip file (the only file in ROR Zenodo releases).
    let zip = record
        .files
        .into_iter()
        .find(|f| f.key.ends_with(".zip"))
        .ok_or_else(|| Error::Parse("no zip file found in Zenodo release".into()))?;

    Ok(RorRelease {
        version: record.metadata.version,
        date: record.metadata.publication_date,
        zenodo_id: record.id.to_string(),
        filename: zip.key,
        download_url: zip.links.self_,
    })
}

/// Download the zip archive described by `release`, extract the first `.json`
/// file, and parse it into a list of [`Ror`] records. Use this after calling
/// [`fetch_latest_ror_release`] so you can log the release metadata before
/// starting the (slower) zip download.
/// Download and parse a ROR data release zip. The zip is cached locally for
/// 30 days under `cache_dir("ror")/{filename}` so repeat installs of the same
/// version avoid a 33 MB network round-trip.
///
/// Returns `(records, from_cache)` where `from_cache` is `true` when the zip
/// was served from the local cache rather than downloaded.
pub fn download_release(release: &RorRelease) -> Result<(Vec<Ror>, bool)> {
    let ttl = std::time::Duration::from_secs(30 * 24 * 60 * 60);
    let (zip_bytes, from_cache) = crate::file_utils::download_file_cached(
        &release.download_url,
        "ror",
        &release.filename,
        ttl,
    )
    .map_err(|e| Error::Http(e.to_string()))?;
    let json_bytes = crate::file_utils::unzip_first_json(&zip_bytes)
        .map_err(|e| Error::Parse(e.to_string()))?;
    let list: Vec<Ror> = serde_json::from_slice(&json_bytes)
        .map_err(|e| Error::Parse(format!("parsing ROR JSON: {}", e)))?;
    Ok((list, from_cache))
}

/// Convenience wrapper: fetch the latest release metadata from Zenodo and
/// immediately download and parse the data dump.
/// Returns `(RorRelease, Vec<Ror>, from_cache)`.
pub fn download_all() -> Result<(RorRelease, Vec<Ror>, bool)> {
    let release = fetch_latest_ror_release()?;
    let (list, from_cache) = download_release(&release)?;
    Ok((release, list, from_cache))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROR_ORG: &str = r#"{
  "id": "https://ror.org/02nr0ka47",
  "established": 2013,
  "external_ids": [
    {
      "type": "Wikidata",
      "all": ["Q19341888"],
      "preferred": ""
    },
    {
      "type": "GRID",
      "all": ["grid.465570.2"],
      "preferred": "grid.465570.2"
    },
    {
      "type": "FundRef",
      "all": ["100012611"],
      "preferred": "100012611"
    }
  ],
  "links": [
    {"type": "website", "value": "https://impactstory.org"}
  ],
  "locations": [
    {
      "geonames_id": 4774183,
      "geonames_details": {
        "country_code": "US",
        "country_name": "United States",
        "name": "Williamsburg"
      }
    }
  ],
  "names": [
    {"value": "Impactstory", "types": ["ror_display", "label"], "lang": "en"},
    {"value": "IS", "types": ["acronym"], "lang": ""}
  ],
  "relationships": [
    {"type": "related", "label": "Our Society", "id": "https://ror.org/045gyfv07"}
  ],
  "status": "active",
  "types": ["nonprofit"]
}"#;

    #[test]
    fn test_read_ror_basic() {
        let data = read_json(ROR_ORG).unwrap();

        assert_eq!(data.id, "https://ror.org/02nr0ka47");
        assert_eq!(data.type_, "Organization");
        assert_eq!(data.provider, "ROR");
        assert_eq!(data.url, "https://impactstory.org");
    }

    #[test]
    fn test_ror_display_name() {
        let data = read_json(ROR_ORG).unwrap();
        assert_eq!(data.title, "Impactstory");
    }

    #[test]
    fn test_ror_date() {
        let data = read_json(ROR_ORG).unwrap();
        assert_eq!(data.date_published, "2013");
    }

    #[test]
    fn test_ror_identifiers() {
        let data = read_json(ROR_ORG).unwrap();

        // Wikidata: preferred is empty → uses first in all
        let wikidata = data
            .identifiers
            .iter()
            .find(|i| i.identifier_type == "Wikidata")
            .unwrap();
        assert_eq!(wikidata.identifier, "Q19341888");

        // GRID: preferred set
        let grid = data
            .identifiers
            .iter()
            .find(|i| i.identifier_type == "GRID")
            .unwrap();
        assert_eq!(grid.identifier, "grid.465570.2");

        // FundRef → Crossref Funder ID
        let fundref = data
            .identifiers
            .iter()
            .find(|i| i.identifier_type == "Crossref Funder ID")
            .unwrap();
        assert_eq!(fundref.identifier, "100012611");
    }

    #[test]
    fn test_ror_relations() {
        let data = read_json(ROR_ORG).unwrap();
        assert_eq!(data.relations.len(), 1);
        assert_eq!(data.relations[0].id, "https://ror.org/045gyfv07");
        assert_eq!(data.relations[0].type_, "References");
    }

    #[test]
    fn test_ror_no_established() {
        let json = r#"{
          "id": "https://ror.org/01234abc",
          "names": [{"value": "Test Org", "types": ["ror_display"]}],
          "links": [],
          "external_ids": [],
          "relationships": [],
          "status": "active",
          "types": ["education"]
        }"#;
        let data = read_json(json).unwrap();
        assert!(data.date_published.is_empty());
    }

    #[test]
    fn test_get_display_name() {
        let ror: Ror = serde_json::from_str(ROR_ORG).unwrap();
        assert_eq!(get_display_name(&ror), "Impactstory");
    }

    #[test]
    fn test_ror_no_display_name() {
        let json = r#"{
          "id": "https://ror.org/abc123",
          "names": [{"value": "Other Name", "types": ["label"]}],
          "links": [],
          "external_ids": [],
          "relationships": [],
          "status": "active",
          "types": []
        }"#;
        let data = read_json(json).unwrap();
        assert!(data.title.is_empty());
    }

    #[test]
    fn test_write_inveniordm_yaml() {
        let data = read_json(ROR_ORG).unwrap();
        let bytes = write(&data).unwrap();
        let yaml = String::from_utf8(bytes).unwrap();

        // Bare ROR ID (not full URL)
        assert!(
            yaml.contains("id: 02nr0ka47"),
            "expected bare ROR id, got:\n{yaml}"
        );
        assert!(yaml.contains("name: Impactstory"));
        assert!(yaml.contains("scheme: ror"));
        assert!(yaml.contains("scheme: wikidata"));
        assert!(yaml.contains("scheme: grid"));
        assert!(yaml.contains("scheme: fundref"));
        // Title map should have English entry
        assert!(yaml.contains("en: Impactstory"));
    }

    #[test]
    fn test_write_ror_json() {
        let data = read_json(ROR_ORG).unwrap();
        let bytes = write_json(&data).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(v["id"].as_str().unwrap(), "https://ror.org/02nr0ka47");
        assert_eq!(v["names"][0]["value"].as_str().unwrap(), "Impactstory");
        assert_eq!(v["links"][0]["type"].as_str().unwrap(), "website");
        assert_eq!(
            v["links"][0]["value"].as_str().unwrap(),
            "https://impactstory.org"
        );
        assert_eq!(v["established"].as_i64().unwrap(), 2013);
        assert_eq!(v["status"].as_str().unwrap(), "active");

        // External IDs
        let ext_ids = v["external_ids"].as_array().unwrap();
        assert!(ext_ids.iter().any(|e| e["type"] == "Wikidata"));
        assert!(ext_ids.iter().any(|e| e["type"] == "GRID"));
        assert!(ext_ids.iter().any(|e| e["type"] == "FundRef"));

        // Relationships
        let rels = v["relationships"].as_array().unwrap();
        assert_eq!(rels[0]["type"].as_str().unwrap(), "related");
        assert_eq!(rels[0]["id"].as_str().unwrap(), "https://ror.org/045gyfv07");
    }

    #[test]
    fn test_write_invalid_ror_id() {
        use crate::data::Data;
        let data = Data {
            id: "https://doi.org/10.1234/not-a-ror".to_string(),
            ..Data::default()
        };
        assert!(write(&data).is_err());
    }

    // ── Matcher tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_clean_search_string_special_chars() {
        assert_eq!(
            clean_search_string("Dept. of Biology, MIT; USA"),
            "Dept of Biology MIT USA"
        );
    }

    #[test]
    fn test_clean_search_string_postal_code() {
        // 5-digit postal code should be removed
        assert_eq!(
            clean_search_string("Stanford University 94305 CA"),
            "Stanford University CA"
        );
        // 4-digit numbers should be kept
        assert_eq!(clean_search_string("Lab 2024 report"), "Lab 2024 report");
    }

    #[test]
    fn test_clean_search_string_collapses_whitespace() {
        assert_eq!(
            clean_search_string("  University   of   Cambridge  "),
            "University of Cambridge"
        );
    }

    #[test]
    fn test_clean_search_string_empty() {
        assert_eq!(clean_search_string(""), "");
        assert_eq!(clean_search_string("...;;;"), "");
    }

    #[test]
    fn test_get_country_codes_full_name() {
        let codes = get_country_codes("University of California, United States");
        assert!(
            codes.contains(&"US".to_string()),
            "expected US in {:?}",
            codes
        );
    }

    #[test]
    fn test_get_country_codes_abbreviation() {
        let codes = get_country_codes("Max Planck Institute, Germany");
        assert!(
            codes.contains(&"DE".to_string()),
            "expected DE in {:?}",
            codes
        );
    }

    #[test]
    fn test_get_country_codes_uk() {
        let codes = get_country_codes("University of Oxford, United Kingdom");
        assert!(
            codes.contains(&"UK".to_string()),
            "expected UK in {:?}",
            codes
        );
    }

    #[test]
    fn test_get_country_codes_no_match() {
        // A string with no recognisable country
        let codes = get_country_codes("Department of Zoology");
        // May or may not match, but must not panic
        let _ = codes;
    }

    #[test]
    fn test_get_countries_region_mapping() {
        // "US" should map to "US-PR" region
        let regions = get_countries("Harvard University, USA");
        assert!(
            regions.contains(&"US-PR".to_string()),
            "expected US-PR in {:?}",
            regions
        );
    }

    #[test]
    fn test_get_countries_uk_region() {
        let regions = get_countries("Imperial College London, UK");
        assert!(
            regions.contains(&"GB-UK".to_string()),
            "expected GB-UK in {:?}",
            regions
        );
    }

    #[test]
    fn test_to_region_special() {
        assert_eq!(to_region("US"), "US-PR");
        assert_eq!(to_region("PR"), "US-PR");
        assert_eq!(to_region("GB"), "GB-UK");
        assert_eq!(to_region("UK"), "GB-UK");
        assert_eq!(to_region("CN"), "CN-HK-TW");
        assert_eq!(to_region("HK"), "CN-HK-TW");
        assert_eq!(to_region("TW"), "CN-HK-TW");
    }

    #[test]
    fn test_to_region_passthrough() {
        // Non-special codes return empty string; callers use the code directly
        assert_eq!(to_region("DE"), "");
        assert_eq!(to_region("FR"), "");
    }

    #[test]
    fn test_get_country_codes_france() {
        let codes = get_country_codes("CNRS, France");
        assert!(
            codes.contains(&"FR".to_string()),
            "expected FR in {:?}",
            codes
        );
    }

    #[test]
    fn test_get_country_codes_japan() {
        let codes = get_country_codes("University of Tokyo, Japan");
        assert!(
            codes.contains(&"JP".to_string()),
            "expected JP in {:?}",
            codes
        );
    }

    // ── Bulk writer tests ────────────────────────────────────────────────────

    fn sample_ror() -> Ror {
        serde_json::from_str(ROR_ORG).unwrap()
    }

    #[test]
    fn test_convert_ror_csv_basic() {
        let ror = sample_ror();
        let row = convert_ror_csv(&ror);

        assert_eq!(row.id, "https://ror.org/02nr0ka47");
        assert_eq!(row.name, "Impactstory");
        assert_eq!(row.types, "nonprofit");
        assert_eq!(row.status, "active");
        assert_eq!(row.links, "https://impactstory.org");
        assert_eq!(row.established, "2013");
        assert_eq!(row.country_code, "US");
        assert_eq!(row.place, "Williamsburg");
    }

    #[test]
    fn test_convert_ror_csv_external_ids() {
        let ror = sample_ror();
        let row = convert_ror_csv(&ror);

        assert_eq!(row.external_ids_wikidata_all, "Q19341888");
        assert_eq!(row.external_ids_grid_preferred, "grid.465570.2");
        assert_eq!(row.external_ids_fundref_preferred, "100012611");
    }

    #[test]
    fn test_convert_ror_csv_relationships() {
        let ror = sample_ror();
        let row = convert_ror_csv(&ror);
        assert_eq!(row.relationships, "Related: https://ror.org/045gyfv07");
    }

    #[test]
    fn test_convert_ror_csv_no_established() {
        let mut ror = sample_ror();
        ror.established = None;
        let row = convert_ror_csv(&ror);
        assert!(row.established.is_empty());

        ror.established = Some(0);
        let row = convert_ror_csv(&ror);
        assert!(row.established.is_empty());
    }

    #[test]
    fn test_convert_ror_csv_no_locations() {
        let mut ror = sample_ror();
        ror.locations.clear();
        let row = convert_ror_csv(&ror);
        assert!(row.country_code.is_empty());
        assert!(row.place.is_empty());
    }

    #[test]
    fn test_write_csv_roundtrip() {
        let ror = sample_ror();
        let bytes = write_csv(std::slice::from_ref(&ror)).unwrap();
        let text = String::from_utf8(bytes).unwrap();

        let mut reader = csv::Reader::from_reader(text.as_bytes());
        let records: Vec<RorCsv> = reader.deserialize().map(|r| r.unwrap()).collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "Impactstory");
        assert_eq!(records[0].id, "https://ror.org/02nr0ka47");
    }

    #[test]
    fn test_write_parquet_roundtrip() {
        let ror = sample_ror();
        let bytes = write_parquet(std::slice::from_ref(&ror)).unwrap();
        assert!(!bytes.is_empty());

        // Parquet files start with the magic bytes "PAR1"
        assert_eq!(&bytes[0..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
    }

    #[test]
    fn test_write_all_dispatch() {
        let ror = sample_ror();
        let list = vec![ror];

        let json = write_all(&list, ".json").unwrap();
        assert!(String::from_utf8(json).unwrap().contains("Impactstory"));

        let yaml = write_all(&list, ".yaml").unwrap();
        assert!(String::from_utf8(yaml).unwrap().contains("Impactstory"));

        let jsonl = write_all(&list, ".jsonl").unwrap();
        let jsonl_text = String::from_utf8(jsonl).unwrap();
        assert_eq!(jsonl_text.lines().count(), 1);

        let csv_bytes = write_all(&list, ".csv").unwrap();
        assert!(
            String::from_utf8(csv_bytes)
                .unwrap()
                .contains("Impactstory")
        );

        let parquet_bytes = write_all(&list, ".parquet").unwrap();
        assert_eq!(&parquet_bytes[0..4], b"PAR1");

        assert!(write_all(&list, ".sql").is_err());
    }

    #[test]
    fn test_write_all_empty_list() {
        let list: Vec<Ror> = vec![];
        assert!(write_all(&list, ".json").unwrap() == b"[]");
        let csv_bytes = write_all(&list, ".csv").unwrap();
        assert!(csv_bytes.is_empty());
    }

    #[test]
    #[ignore = "network"]
    fn test_fetch_latest_ror_release() {
        let release = fetch_latest_ror_release().unwrap();
        // Version must match "vMAJOR.MINOR" pattern
        assert!(
            release.version.starts_with('v'),
            "version should start with 'v': {}",
            release.version
        );
        // Date must be ISO 8601
        assert_eq!(release.date.len(), 10, "date should be YYYY-MM-DD: {}", release.date);
        assert!(!release.zenodo_id.is_empty());
        assert!(release.filename.ends_with(".zip"));
        assert!(release.download_url.contains("zenodo.org"));
        // Confirm this is genuinely the latest (v2.9 as of 2026-06-23)
        assert_eq!(release.version, "v2.9");
        assert_eq!(release.date, "2026-06-23");
    }
}
