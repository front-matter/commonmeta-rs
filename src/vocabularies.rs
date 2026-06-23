//! Embedded controlled vocabulary data files.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::error::{Error, Result};

/// The bundled SPDX license list (`licenses.json`).
pub const SPDX_LICENSES: &str = include_str!("vocabularies/licenses.json");

/// Loads a vocabulary's raw contents by name.
pub fn load_vocabulary(name: &str) -> Result<&'static str> {
    match name {
        "SPDX.Licenses" => Ok(SPDX_LICENSES),
        other => Err(Error::Parse(format!("unsupported vocabulary: {other}"))),
    }
}

// ── OpenAlex subjects ─────────────────────────────────────────────────────────

const SUBJECTS_OPENALEX_YAML: &str = include_str!("vocabularies/subjects_openalex.yaml");

#[derive(Deserialize)]
struct OaEntry {
    id: String,
    subject: String,
}

/// Returns the OpenAlex subjects map: numeric ID string → (full URL, subject name).
///
/// Keys after last slash:
/// - 1 digit  → Domain
/// - 2 digits → Field
/// - 4 digits → Subfield
/// - "T" + 5 digits → Topic
fn openalex_vocab() -> &'static HashMap<String, (String, String)> {
    static VOCAB: OnceLock<HashMap<String, (String, String)>> = OnceLock::new();
    VOCAB.get_or_init(|| {
        let entries: Vec<OaEntry> =
            serde_yaml::from_str(SUBJECTS_OPENALEX_YAML).unwrap_or_default();
        entries
            .into_iter()
            .map(|e| {
                let key = e.id.rsplit('/').next().unwrap_or("").to_string();
                (key, (e.id, e.subject))
            })
            .collect()
    })
}

/// Look up a single OpenAlex subject by its numeric ID (the part after the last
/// URL slash, e.g. `"1702"`) and return `(full_url, subject_name)`, or `None`
/// if the ID is not found in the vocabulary.
pub fn lookup_openalex_subject(id: &str) -> Option<(String, String)> {
    if id.is_empty() {
        return None;
    }
    openalex_vocab()
        .get(id)
        .map(|(url, name)| (url.clone(), name.clone()))
}
