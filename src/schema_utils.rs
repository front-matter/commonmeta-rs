//! JSON Schema validation utilities.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

pub const SCHEMA_VERSION: &str = "commonmeta_v0.18";

const SCHEMATA: &[&str] = &[
    SCHEMA_VERSION,
    "datacite-v4.5",
    "crossref-v0.2",
    "csl-data",
    "cff_v1.2.0",
    "invenio-rdm-v0.1",
];

/// Return the list of schema names accepted by `json_schema_errors`.
pub fn known_schemata() -> &'static [&'static str] {
    SCHEMATA
}

/// Validate a JSON document against one of the bundled schema names.
///
/// If `schema` is `None`, the default `SCHEMA_VERSION` is used.
pub fn json_schema_errors(document: &[u8], schema: Option<&str>) -> Result<()> {
    let schema_name = schema.unwrap_or(SCHEMA_VERSION);

    if !SCHEMATA.contains(&schema_name) {
        return Err(Error::UnsupportedFormat(format!(
            "schema '{schema_name}' not found"
        )));
    }

    let schema_text = load_schema(schema_name)?;

    let schema_json: Value =
        serde_json::from_str(&schema_text).map_err(|e| Error::Parse(e.to_string()))?;
    let document_json: Value =
        serde_json::from_slice(document).map_err(|e| Error::Parse(e.to_string()))?;

    let validation_schema = effective_validation_schema(&schema_json);

    let compiled = jsonschema::validator_for(&validation_schema)
        .map_err(|e| Error::Parse(e.to_string()))?;

    let validation_errors: Vec<String> = match compiled.validate(&document_json) {
        Ok(()) => Vec::new(),
        Err(_) => compiled.iter_errors(&document_json).map(|e| e.to_string()).collect(),
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

fn load_schema(schema_name: &str) -> Result<String> {
    if schema_name == SCHEMA_VERSION {
        return Ok(include_str!("../resources/commonmeta_v0.18.json").to_string());
    }

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join(format!("{schema_name}.json"));

    fs::read_to_string(&path).map_err(|e| {
        Error::Parse(format!(
            "failed to load schema '{}' from '{}': {}",
            schema_name,
            path.display(),
            e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::{SCHEMA_VERSION, json_schema_errors, known_schemata};

    #[test]
    fn validates_commonmeta_document_with_default_schema() {
        let doc = include_bytes!("../tests/fixtures/commonmeta/journal_article.json");
        let result = json_schema_errors(doc, None);
        assert!(result.is_ok(), "expected schema validation to pass: {result:?}");
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
        assert!(known_schemata().contains(&SCHEMA_VERSION));
    }
}
