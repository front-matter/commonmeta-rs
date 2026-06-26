//! JSON Schema and XSD validation utilities.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use fastxml::schema::fetcher::{FetchResult, FileFetcher, SchemaFetcher};
use fastxml::schema::{Schema, Validator};
use fastxml::schema::fetcher::error::FetchError;
use serde_json::Value;

use crate::error::{Error, Result};

pub const SCHEMA_VERSION: &str = "commonmeta_v1.0";
pub const DEFAULT_SCHEMA: &str = "commonmeta";

// Public schema names aligned with commonmeta-py.
const SCHEMATA: &[&str] = &[
    DEFAULT_SCHEMA,
    "cff",
    "crossref_xml",
    "csl",
    "datacite",
    "inveniordm",
    "schema_org",
];

/// Return the list of schema names accepted by `json_schema_errors`.
pub fn known_schemata() -> &'static [&'static str] {
    SCHEMATA
}

/// Validate a JSON document against one of the bundled schema names.
///
/// If `schema` is `None`, the default `DEFAULT_SCHEMA` (`commonmeta`) is used.
pub fn json_schema_errors(document: &[u8], schema: Option<&str>) -> Result<()> {
    let schema_name = schema.unwrap_or(DEFAULT_SCHEMA);
    let Some(schema_file) = schema_file_name(schema_name) else {
        return Err(Error::UnsupportedFormat(format!(
            "schema '{schema_name}' not found"
        )));
    };

    let schema_text = load_schema(schema_file)?;

    let schema_json: Value = serde_json::from_str(&schema_text)
        .map_err(|_| Error::Parse(format!("invalid JSON in schema file: {schema_file}.json")))?;
    let document_json: Value =
        serde_json::from_slice(document).map_err(|e| Error::Parse(e.to_string()))?;

    let validation_schema = effective_validation_schema(&schema_json);

    let compiled =
        jsonschema::validator_for(&validation_schema).map_err(|e| Error::Parse(e.to_string()))?;

    let validation_errors: Vec<String> = match compiled.validate(&document_json) {
        Ok(()) => Vec::new(),
        Err(_) => compiled
            .iter_errors(&document_json)
            .map(|e| e.to_string())
            .collect(),
    };

    if validation_errors.is_empty() {
        return Ok(());
    }

    Err(Error::Parse(format!(
        "json schema validation failed ({} errors): {}",
        validation_errors.len(),
        validation_errors.join("; ")
    )))
}

// ── XSD validation ─────────────────────────────────────────────────────────────

/// Validate an XML document against a bundled XSD schema.
///
/// Supported schema names: `"crossref_xml"` (aliases `"crossref"`,
/// `"crossref-v5.4.0"`), `"datacite_xml"` (alias `"datacite-v4.7"`).
/// The compiled schema is built once and reused across calls.
pub fn xml_schema_errors(xml: &[u8], schema: Option<&str>) -> Result<()> {
    let schema_name = schema.unwrap_or("crossref_xml");

    let compiled = match schema_name {
        "crossref_xml" | "crossref" | "crossref-v5.4.0" => crossref_xsd_schema()?,
        "datacite_xml" | "datacite-v4.7"                => datacite_xsd_schema()?,
        other => {
            return Err(Error::UnsupportedFormat(format!(
                "XSD schema '{other}' not supported"
            )));
        }
    };

    let report = Validator::from(xml)
        .schema(compiled)
        .run()
        .map_err(|e| Error::Parse(e.to_string()))?;

    if report.is_valid() {
        return Ok(());
    }

    let errors: Vec<String> = report.errors().iter().map(|e| e.to_string()).collect();
    Err(Error::Parse(format!(
        "XSD validation failed ({} errors): {}",
        errors.len(),
        errors.join("; ")
    )))
}

/// Lazy-compiled Crossref 5.4.0 XSD schema.
///
/// Built once per process; subsequent calls share the `Arc<Schema>`.
fn crossref_xsd_schema() -> Result<Arc<Schema>> {
    static SCHEMA: OnceLock<std::result::Result<Arc<Schema>, String>> = OnceLock::new();

    SCHEMA
        .get_or_init(build_crossref_schema)
        .as_ref()
        .map(Arc::clone)
        .map_err(|e| Error::Parse(e.clone()))
}

fn build_crossref_schema() -> std::result::Result<Arc<Schema>, String> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("crossref");

    let main_xsd_path = base_dir.join("crossref5.4.0.xsd");
    let main_xsd = fs::read(&main_xsd_path)
        .map_err(|e| format!("could not read crossref5.4.0.xsd: {e}"))?;

    // SandboxFetcher resolves imports from the local resources/crossref/
    // directory.  For HTTP/HTTPS URLs that cannot be satisfied locally it
    // returns an empty stub schema rather than making network requests — the
    // same behaviour as xmlschema's allow="sandbox" in Python.
    let fetcher = SandboxFetcher { base: FileFetcher::with_base_dir(&base_dir) };

    // The builder requires an absolute URI as the schema's base URI so that
    // relative imports inside the XSD can be resolved.  We use the canonical
    // Crossref URL even though we are serving the file locally — the fetcher
    // intercepts all import requests and rewrites them to local lookups.
    Schema::builder()
        .add(
            "https://www.crossref.org/schemas/crossref5.4.0.xsd",
            main_xsd,
        )
        .resolve_with(&fetcher)
        .map(Arc::new)
        .map_err(|e| format!("failed to compile Crossref XSD schema: {e}"))
}

/// Lazy-compiled DataCite 4.7 XSD schema.
fn datacite_xsd_schema() -> Result<Arc<Schema>> {
    static SCHEMA: OnceLock<std::result::Result<Arc<Schema>, String>> = OnceLock::new();
    SCHEMA
        .get_or_init(build_datacite_schema)
        .as_ref()
        .map(Arc::clone)
        .map_err(|e| Error::Parse(e.clone()))
}

fn build_datacite_schema() -> std::result::Result<Arc<Schema>, String> {
    let base_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("datacite");

    let main_xsd_path = base_dir.join("datacite-v4.xsd");
    let main_xsd = fs::read(&main_xsd_path)
        .map_err(|e| format!("could not read datacite-v4.xsd: {e}"))?;

    let fetcher = SandboxFetcher { base: FileFetcher::with_base_dir(&base_dir) };

    Schema::builder()
        .add(
            "https://schema.datacite.org/meta/kernel-4.7/metadata.xsd",
            main_xsd,
        )
        .resolve_with(&fetcher)
        .map(Arc::new)
        .map_err(|e| format!("failed to compile DataCite XSD schema: {e}"))
}

/// A schema fetcher that resolves imports from a local directory and returns
/// empty stub schemas for remote URLs (preventing any network access).
struct SandboxFetcher {
    base: FileFetcher,
}

impl SchemaFetcher for SandboxFetcher {
    fn fetch(&self, url: &str) -> fastxml::error::Result<FetchResult> {
        // Try the local file fetcher first (handles relative paths and
        // file:// URLs against the base directory).
        if let Ok(result) = self.base.fetch(url) {
            return Ok(result);
        }

        // For absolute HTTP/HTTPS URLs: try extracting just the filename and
        // look for it in the base directory (e.g. xml.xsd, mathml3.xsd).
        if url.starts_with("http://") || url.starts_with("https://") {
            if let Some(filename) = url.rsplit('/').next() {
                if let Ok(result) = self.base.fetch(filename) {
                    return Ok(result);
                }
            }
            // Return an empty stub schema so compilation can proceed without
            // the remote schema (types from that namespace won't be validated).
            let stub = r#"<?xml version="1.0" encoding="UTF-8"?><xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"/>"#;
            return Ok(FetchResult {
                content: stub.as_bytes().to_vec(),
                final_url: url.to_string(),
                redirected: false,
            });
        }

        // All other unresolvable URLs: propagate the error.
        Err(FetchError::RequestFailed {
            url: url.to_string(),
            message: "schema not found locally".to_string(),
        }
        .into())
    }
}

// ── Private helpers ────────────────────────────────────────────────────────────

fn effective_validation_schema(schema_json: &Value) -> Value {
    // commonmeta schema files wrap the actual document schema under
    // a `commonmeta` key together with shared `definitions`.
    let Some(commonmeta_root) = schema_json.get("commonmeta") else {
        return schema_json.clone();
    };

    let mut merged = serde_json::Map::new();

    if let Some(v) = schema_json.get("$schema") {
        merged.insert("$schema".to_string(), v.clone());
    }
    if let Some(v) = schema_json.get("$id") {
        merged.insert("$id".to_string(), v.clone());
    }
    if let Some(v) = schema_json.get("definitions") {
        merged.insert("definitions".to_string(), v.clone());
    }

    if let Value::Object(obj) = commonmeta_root {
        for (key, value) in obj {
            merged.insert(key.clone(), value.clone());
        }
        return Value::Object(merged);
    }

    schema_json.clone()
}

fn schema_file_name(schema_name: &str) -> Option<&'static str> {
    match schema_name {
        "commonmeta" | SCHEMA_VERSION => Some(SCHEMA_VERSION),
        "cff" | "cff_v1.2.0" => Some("cff_v1.2.0"),
        "crossref_xml" | "crossref-v5.4.0" | "crossref-v0.2" => Some("crossref-v5.4.0"),
        "csl" | "csl-data" => Some("csl-data"),
        "datacite" | "datacite-v4.5" => Some("datacite-v4.5"),
        "inveniordm" | "inveniordm-v0.1" | "invenio-rdm-v0.1" => Some("inveniordm-v0.1"),
        "schema_org" | "schema_org-v0.1" => Some("schema_org-v0.1"),
        _ => None,
    }
}

fn load_schema(schema_file: &str) -> Result<String> {
    if schema_file == SCHEMA_VERSION {
        return Ok(include_str!("../resources/commonmeta_v1.0.json").to_string());
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(format!("{schema_file}.json"));

    fs::read_to_string(&path)
        .map_err(|_| Error::Parse(format!("schema file not found: {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SCHEMA, SCHEMA_VERSION, json_schema_errors, known_schemata, schema_file_name,
        xml_schema_errors,
    };

    #[test]
    fn validates_commonmeta_document_with_default_schema() {
        let doc = include_bytes!("../tests/fixtures/commonmeta/journal_article.json");
        let result = json_schema_errors(doc, None);
        assert!(
            result.is_ok(),
            "expected schema validation to pass: {result:?}"
        );
    }

    #[test]
    fn rejects_invalid_commonmeta_document() {
        let result = json_schema_errors(br#"{}"#, None);
        assert!(result.is_err(), "expected validation to fail");
        let message = result.expect_err("validation should fail").to_string();
        assert!(
            message.contains("validation failed") || message.contains("required"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn rejects_unknown_schema_name() {
        let result = json_schema_errors(br#"{}"#, Some("does-not-exist"));
        assert!(result.is_err(), "expected unknown schema to fail");
        let message = result.expect_err("unknown schema should fail").to_string();
        assert!(message.contains("schema 'does-not-exist' not found"));
    }

    #[test]
    fn includes_default_schema_in_known_list() {
        assert!(known_schemata().contains(&DEFAULT_SCHEMA));
    }

    #[test]
    fn supports_python_schema_aliases() {
        assert_eq!(schema_file_name("commonmeta"), Some(SCHEMA_VERSION));
        assert_eq!(schema_file_name("commonmeta_v0.18"), None);
        assert_eq!(schema_file_name("datacite"), Some("datacite-v4.5"));
        assert_eq!(schema_file_name("crossref_xml"), Some("crossref-v5.4.0"));
    }

    #[test]
    fn xsd_rejects_unknown_schema_name() {
        let result = xml_schema_errors(b"<foo/>", Some("unknown"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("not supported"), "unexpected: {msg}");
    }

    // fastxml 0.9.0 has a known bug: <xsd:choice minOccurs="0"> groups are
    // treated as required, causing false failures on any JATS mixed-content
    // element (title, jats:p, institution_name, etc.).  The test below only
    // verifies that the Crossref XSD schema loads and compiles successfully,
    // not that full document validation passes.
    #[test]
    fn xsd_crossref_schema_compiles() {
        // Calling xml_schema_errors forces the OnceLock schema to be built.
        // We expect either Ok (valid) or an Err that does NOT contain "failed
        // to compile" (which would indicate a schema-load failure rather than
        // a document-validation failure).
        let xml = include_bytes!("../tests/fixtures/crossref_xml/journal_article.xml");
        let result = xml_schema_errors(xml, Some("crossref_xml"));
        if let Err(ref e) = result {
            assert!(
                !e.to_string().contains("failed to compile"),
                "Crossref XSD schema failed to compile: {e}"
            );
        }
    }
}
