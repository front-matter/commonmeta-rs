use serde_json::Value;

use crate::data::{Data, License, Publisher, Subject};
use crate::error::{Error, Result};
use crate::utils::{normalize_id, sanitize};

use crate::constants as C;
use super::schemaorg::{get_contributor, value_to_contributors};

// ── GitHub URL utilities ──────────────────────────────────────────────────────

fn github_from_url(url: &str) -> Option<(String, String, Option<String>, Option<String>)> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !host.ends_with("github.com") && !host.ends_with("githubusercontent.com") {
        return None;
    }
    let words: Vec<&str> = parsed.path().trim_start_matches('/').split('/').collect();
    let owner = words
        .first()
        .copied()
        .filter(|s| !s.is_empty())?
        .to_string();
    let repo = words.get(1).copied().filter(|s| !s.is_empty())?.to_string();
    // GitHub web URLs: owner/repo/tree/<branch>/<path...>
    // words: [0]=owner [1]=repo [2]="tree"|"blob" [3]=branch [4+]=path
    let release = words
        .get(3)
        .copied()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let path = if words.len() > 4 {
        let p = words[4..].join("/");
        if p.is_empty() { None } else { Some(p) }
    } else {
        None
    };
    Some((owner, repo, release, path))
}

/// Convert any GitHub URL to the raw codemeta.json download URL.
/// Defaults to the `main` branch.
fn github_as_codemeta_url(url: &str) -> Option<String> {
    let (owner, repo, release, path) = github_from_url(url)?;
    if let Some(p) = &path
        && p.ends_with("codemeta.json")
    {
        let branch = release.as_deref().unwrap_or("main");
        return Some(format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            owner, repo, branch, p
        ));
    }
    Some(format!(
        "https://raw.githubusercontent.com/{}/{}/main/codemeta.json",
        owner, repo
    ))
}

/// Convert any GitHub-related URL to the canonical repo URL.
fn github_as_repo_url(url: &str) -> Option<String> {
    let (owner, repo, _, _) = github_from_url(url)?;
    Some(format!("https://github.com/{}/{}", owner, repo))
}

// ── JSON value helpers ────────────────────────────────────────────────────────

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|f| f.as_str()).unwrap_or("")
}

fn str_field_owned(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

// ── Publisher parsing ─────────────────────────────────────────────────────────

fn parse_publisher(v: &Value) -> Publisher {
    match v.get("publisher") {
        Some(Value::String(s)) if !s.is_empty() => Publisher {
            name: s.clone(),
            ..Default::default()
        },
        Some(obj) if obj.is_object() => {
            let name = obj
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            Publisher {
                name,
                ..Default::default()
            }
        }
        _ => Publisher::default(),
    }
}

// ── Keyword / subject parsing ─────────────────────────────────────────────────

fn parse_keywords(v: &Value) -> Vec<Subject> {
    match v.get("keywords") {
        Some(Value::String(s)) => s
            .split(',')
            .map(|k| Subject {
                subject: k.trim().to_string(),
                ..Default::default()
            })
            .filter(|s| !s.subject.is_empty())
            .collect(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|k| k.as_str())
            .map(|k| Subject {
                subject: k.trim().to_string(),
                ..Default::default()
            })
            .filter(|s| !s.subject.is_empty())
            .collect(),
        _ => vec![],
    }
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn from_value(doc: &Value) -> Data {
    // ID — from @id or identifier field
    let id_raw = {
        let from_id = str_field(doc, "@id");
        let from_identifier = str_field(doc, "identifier");
        if !from_id.is_empty() {
            from_id
        } else {
            from_identifier
        }
    };
    let id = normalize_id(id_raw);

    // Type — @type via SO_TO_CM_TRANSLATIONS; default "Software"
    let type_raw = str_field(doc, "@type");
    let type_ = {
        let mapped = C::so_to_cm(type_raw);
        if mapped.is_empty() {
            "Software"
        } else {
            mapped
        }
    }
    .to_string();

    // URL — from codeRepository
    let url = normalize_id(str_field(doc, "codeRepository"));

    // Title — prefer "title", fall back to "name"
    let title = {
        let t = str_field(doc, "title");
        if !t.is_empty() {
            t
        } else {
            str_field(doc, "name")
        }
    }
    .to_string();

    // Contributors — "agents" takes priority over "authors"
    let author_val = doc
        .get("agents")
        .filter(|v| !v.is_null())
        .or_else(|| doc.get("authors"))
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(Value::Null);
    let mut contributors: Vec<_> = value_to_contributors(&author_val)
        .into_iter()
        .map(|c| get_contributor(c, "Author"))
        .collect();

    // Editors
    if let Some(ed_val) = doc.get("editor").filter(|v| !v.is_null()) {
        let editors: Vec<_> = value_to_contributors(ed_val)
            .into_iter()
            .map(|c| get_contributor(c, "Editor"))
            .collect();
        contributors.extend(editors);
    }

    // Dates
    let date_created = str_field(doc, "dateCreated").to_string();
    let date_published = str_field(doc, "datePublished").to_string();
    let date_updated = str_field(doc, "dateModified").to_string();

    // Publisher
    let publisher = parse_publisher(doc);

    // Description
    let desc_raw = str_field(doc, "description");
    let description = if !desc_raw.is_empty() {
        sanitize(desc_raw)
    } else {
        String::new()
    };

    // License — codemeta uses "licenseId" as an SPDX identifier
    let license_id = str_field(doc, "licenseId").to_string();
    let license = if !license_id.is_empty() {
        crate::spdx::from_id(&license_id)
    } else {
        License::default()
    };

    // Version
    let version = str_field_owned(doc, "version");

    // Keywords → subjects
    let subjects = parse_keywords(doc);

    Data {
        id,
        type_,
        url,
        title,
        contributors,
        date_published,
        dates: crate::data::Dates {
            created: date_created,
            ..Default::default()
        },
        date_updated,
        publisher,
        description,
        license,
        version,
        subjects,
        ..Data::default()
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let doc: Value = serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_value(&doc))
}

/// Fetch a codemeta.json from a GitHub repository URL and parse it.
pub fn fetch(url: &str) -> Result<Data> {
    let codemeta_url = github_as_codemeta_url(url)
        .ok_or_else(|| Error::Parse(format!("cannot derive codemeta.json URL from: {}", url)))?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let mut doc: Value = client
        .get(&codemeta_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .json()
        .map_err(|e| Error::Parse(e.to_string()))?;

    // If codeRepository is absent, fill from the canonical repo URL
    if doc.get("codeRepository").is_none_or(|v| v.is_null())
        && let Some(repo_url) = github_as_repo_url(&codemeta_url)
    {
        doc["codeRepository"] = Value::String(repo_url);
    }

    Ok(from_value(&doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEMETA_SOFTWARE: &str = r#"{
  "@context": "https://doi.org/10.5063/schema/codemeta-2.0",
  "@type": "SoftwareSourceCode",
  "@id": "https://doi.org/10.5281/zenodo.6154694",
  "name": "Commonmeta Ruby",
  "authors": [
    {
      "@type": "Person",
      "@id": "https://orcid.org/0000-0003-1419-2405",
      "givenName": "Martin",
      "familyName": "Fenner",
      "affiliation": {
        "@type": "Organization",
        "name": "Front Matter"
      }
    }
  ],
  "editor": [
    {
      "@type": "Person",
      "givenName": "Jane",
      "familyName": "Doe"
    }
  ],
  "dateCreated": "2022-01-01",
  "datePublished": "2022-02-15",
  "dateModified": "2023-05-10",
  "description": "Ruby library for conversion of scholarly metadata.",
  "licenseId": "MIT",
  "version": "2.1.0",
  "keywords": ["metadata", "scholarly"],
  "codeRepository": "https://github.com/front-matter/commonmeta-ruby",
  "publisher": "GitHub"
}"#;

    #[test]
    fn test_read_codemeta_basic() {
        let data = read_json(CODEMETA_SOFTWARE).unwrap();

        assert_eq!(data.type_, "Software");
        assert_eq!(data.id, "https://doi.org/10.5281/zenodo.6154694");
        assert_eq!(data.url, "https://github.com/front-matter/commonmeta-ruby");
        assert_eq!(data.title, "Commonmeta Ruby");
        assert_eq!(data.version, "2.1.0");
        assert_eq!(data.license.id, "MIT");
        assert_eq!(data.publisher.name, "GitHub");
    }

    #[test]
    fn test_codemeta_dates() {
        let data = read_json(CODEMETA_SOFTWARE).unwrap();
        assert_eq!(data.dates.created, "2022-01-01");
        assert_eq!(data.date_published, "2022-02-15");
        assert_eq!(data.date_updated, "2023-05-10");
    }

    #[test]
    fn test_codemeta_contributors() {
        let data = read_json(CODEMETA_SOFTWARE).unwrap();

        // Author
        assert_eq!(data.contributors.len(), 2);
        let author = &data.contributors[0];
        assert_eq!(author.type_, "Person");
        assert_eq!(author.given_name(), "Martin");
        assert_eq!(author.family_name(), "Fenner");
        assert_eq!(author.id(), "https://orcid.org/0000-0003-1419-2405");
        assert_eq!(author.affiliations()[0].name, "Front Matter");
        assert!(author.roles.contains(&"Author".to_string()));

        // Editor
        let editor = &data.contributors[1];
        assert_eq!(editor.family_name(), "Doe");
        assert!(editor.roles.contains(&"Editor".to_string()));
    }

    #[test]
    fn test_codemeta_subjects() {
        let data = read_json(CODEMETA_SOFTWARE).unwrap();
        assert_eq!(data.subjects.len(), 2);
        assert_eq!(data.subjects[0].subject, "metadata");
        assert_eq!(data.subjects[1].subject, "scholarly");
    }

    #[test]
    fn test_codemeta_description() {
        let data = read_json(CODEMETA_SOFTWARE).unwrap();
        assert_eq!(
            data.description,
            "Ruby library for conversion of scholarly metadata."
        );
    }

    #[test]
    fn test_codemeta_agents_priority() {
        let json = r#"{
  "@type": "SoftwareSourceCode",
  "name": "Test",
  "agents": [{"@type": "Person", "givenName": "A", "familyName": "Agent"}],
  "authors": [{"@type": "Person", "givenName": "B", "familyName": "Author"}]
}"#;
        let data = read_json(json).unwrap();
        // agents takes priority over authors
        assert_eq!(data.contributors[0].family_name(), "Agent");
    }

    #[test]
    fn test_codemeta_title_fallback() {
        // "title" takes priority; falls back to "name"
        let with_title =
            r#"{"@type":"SoftwareSourceCode","title":"Title Field","name":"Name Field"}"#;
        let data = read_json(with_title).unwrap();
        assert_eq!(data.title, "Title Field");

        let with_name_only = r#"{"@type":"SoftwareSourceCode","name":"Name Field"}"#;
        let data = read_json(with_name_only).unwrap();
        assert_eq!(data.title, "Name Field");
    }

    #[test]
    fn test_codemeta_keyword_string() {
        let json = r#"{"@type":"SoftwareSourceCode","name":"T","keywords":"foo, bar, baz"}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.subjects.len(), 3);
        assert_eq!(data.subjects[0].subject, "foo");
    }

    #[test]
    fn test_github_as_codemeta_url() {
        assert_eq!(
            github_as_codemeta_url("https://github.com/owner/repo"),
            Some("https://raw.githubusercontent.com/owner/repo/main/codemeta.json".to_string())
        );
        assert_eq!(
            github_as_codemeta_url("https://github.com/owner/repo/tree/v1.0/codemeta.json"),
            Some("https://raw.githubusercontent.com/owner/repo/v1.0/codemeta.json".to_string())
        );
    }
}
