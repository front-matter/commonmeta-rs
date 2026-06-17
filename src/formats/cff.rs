use serde_yaml::Value;

use crate::data::{Affiliation, Contributor, Data, Date, Description, License, Publisher, Reference, Subject, Title};
use crate::doi_utils::normalize_doi;
use crate::error::{Error, Result};
use crate::utils::{normalize_id, normalize_orcid, sanitize};

// ── YAML value helpers ────────────────────────────────────────────────────────

fn val_str(v: &Value) -> &str {
    match v {
        Value::String(s) => s.as_str(),
        _ => "",
    }
}

fn val_str_owned(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn val_seq(v: &Value) -> &[Value] {
    match v {
        Value::Sequence(s) => s.as_slice(),
        _ => &[],
    }
}

static NULL_VAL: std::sync::OnceLock<Value> = std::sync::OnceLock::new();

fn null_val() -> &'static Value {
    NULL_VAL.get_or_init(|| Value::Null)
}

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    match v {
        Value::Mapping(m) => m
            .get(&Value::String(key.to_string()))
            .unwrap_or(null_val()),
        _ => null_val(),
    }
}

// ── GitHub URL utilities ──────────────────────────────────────────────────────

struct GithubParts {
    owner: String,
    repo: String,
    release: String, // branch or tag, defaults to "main"
    path: String,    // sub-path within the repo
}

fn github_from_url(url: &str) -> Option<GithubParts> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    if !host.ends_with("github.com") && !host.ends_with("githubusercontent.com") {
        return None;
    }
    let words: Vec<&str> = parsed
        .path()
        .trim_start_matches('/')
        .split('/')
        .collect();
    let owner = words.first().copied().filter(|s| !s.is_empty())?.to_string();
    let repo = words.get(1).copied().filter(|s| !s.is_empty())?.to_string();
    // GitHub web URLs: owner/repo/tree/<branch>/<path...>
    //                  words: [0]=owner [1]=repo [2]="tree"|"blob" [3]=branch [4+]=path
    let release = words
        .get(3)
        .copied()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "main".to_string());
    let path = if words.len() > 4 {
        words[4..].join("/")
    } else {
        String::new()
    };
    Some(GithubParts { owner, repo, release, path })
}

/// Convert any GitHub URL to the raw CITATION.cff download URL.
fn github_as_cff_url(url: &str) -> Option<String> {
    let p = github_from_url(url)?;
    if !p.path.is_empty() && p.path.ends_with("CITATION.cff") {
        Some(format!(
            "https://raw.githubusercontent.com/{}/{}/{}/{}",
            p.owner, p.repo, p.release, p.path
        ))
    } else {
        Some(format!(
            "https://raw.githubusercontent.com/{}/{}/main/CITATION.cff",
            p.owner, p.repo
        ))
    }
}

/// Convert any GitHub-related URL to the canonical repo URL.
fn github_as_repo_url(url: &str) -> Option<String> {
    let p = github_from_url(url)?;
    Some(format!("https://github.com/{}/{}", p.owner, p.repo))
}

// ── Contributor parsing ───────────────────────────────────────────────────────

fn parse_cff_contributors(authors: &[Value]) -> Vec<Contributor> {
    authors
        .iter()
        .map(|author| {
            let family_name = val_str(get(author, "family-names")).to_string();
            let given_name = val_str(get(author, "given-names")).to_string();
            let orcid_raw = val_str(get(author, "orcid")).to_string();
            let orcid = if !orcid_raw.is_empty() {
                let n = normalize_orcid(&orcid_raw);
                if n.is_empty() { None } else { Some(n) }
            } else {
                None
            };

            if !family_name.is_empty() || !given_name.is_empty() || orcid.is_some() {
                // Person
                let affiliations: Vec<Affiliation> = match get(author, "affiliation") {
                    Value::String(s) if !s.is_empty() => vec![Affiliation {
                        name: s.clone(),
                        ..Default::default()
                    }],
                    Value::Sequence(seq) => seq
                        .iter()
                        .filter_map(|a| {
                            let name = val_str(a).to_string();
                            if name.is_empty() { None } else {
                                Some(Affiliation { name, ..Default::default() })
                            }
                        })
                        .collect(),
                    _ => vec![],
                };
                Contributor {
                    id: orcid.unwrap_or_default(),
                    type_: "Person".to_string(),
                    given_name,
                    family_name,
                    affiliations,
                    contributor_roles: vec!["Author".to_string()],
                    ..Default::default()
                }
            } else {
                // Organization
                let name = val_str(get(author, "name")).to_string();
                Contributor {
                    type_: "Organization".to_string(),
                    name,
                    contributor_roles: vec!["Author".to_string()],
                    ..Default::default()
                }
            }
        })
        .collect()
}

// ── Reference parsing ─────────────────────────────────────────────────────────

fn parse_cff_references(references: &[Value]) -> Vec<Reference> {
    references
        .iter()
        .filter_map(|r| {
            // CFF spec field is "identifiers"; look for a DOI entry
            let identifiers = val_seq(get(r, "identifiers"));
            let doi_entry = identifiers.iter().find(|id| {
                val_str(get(id, "type")) == "doi"
            })?;
            let value = val_str(get(doi_entry, "value"));
            if value.is_empty() {
                return None;
            }
            let id = normalize_doi(value);
            if id.is_empty() {
                return None;
            }
            Some(Reference {
                id,
                ..Default::default()
            })
        })
        .collect()
}

// ── Core reader ───────────────────────────────────────────────────────────────

fn from_value(doc: &Value) -> Data {
    // ID from doi field (raw DOI suffix or URL)
    let doi_raw = val_str_owned(get(doc, "doi"));
    let id = if !doi_raw.is_empty() {
        normalize_doi(&doi_raw)
    } else {
        String::new()
    };

    // Repository URL
    let repo_code = val_str(get(doc, "repository-code")).to_string();
    let url = if !repo_code.is_empty() {
        normalize_id(&repo_code)
    } else {
        String::new()
    };

    // Publisher: GitHub if the repository URL is on github.com
    let publisher = if url.contains("github.com") {
        Publisher {
            name: "GitHub".to_string(),
            ..Default::default()
        }
    } else {
        Publisher::default()
    };

    // Title
    let title = val_str(get(doc, "title")).to_string();
    let titles = if !title.is_empty() {
        vec![Title { title, ..Default::default() }]
    } else {
        vec![]
    };

    // Contributors from `authors`
    let contributors = parse_cff_contributors(val_seq(get(doc, "authors")));

    // Date from `date-released`
    let date_released = val_str_owned(get(doc, "date-released"));
    let date = if !date_released.is_empty() {
        Date {
            published: date_released,
            ..Default::default()
        }
    } else {
        Date::default()
    };

    // Abstract
    let abstract_text = val_str(get(doc, "abstract"));
    let descriptions = if !abstract_text.is_empty() {
        vec![Description {
            description: sanitize(abstract_text),
            type_: "Abstract".to_string(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

    // License: can be a single SPDX ID string or a list — take first
    let license_val = get(doc, "license");
    let license_id = match license_val {
        Value::String(s) => s.clone(),
        Value::Sequence(seq) => seq
            .first()
            .and_then(|v| if let Value::String(s) = v { Some(s.clone()) } else { None })
            .unwrap_or_default(),
        _ => String::new(),
    };
    let license = if !license_id.is_empty() {
        License { id: license_id, ..Default::default() }
    } else {
        License::default()
    };

    // Version
    let version = val_str_owned(get(doc, "version"));

    // Keywords → subjects
    let subjects: Vec<Subject> = val_seq(get(doc, "keywords"))
        .iter()
        .map(|k| Subject { subject: val_str(k).to_string() })
        .filter(|s| !s.subject.is_empty())
        .collect();

    // References
    let references = parse_cff_references(val_seq(get(doc, "references")));

    Data {
        id,
        type_: "Software".to_string(),
        url,
        titles,
        contributors,
        date,
        descriptions,
        license,
        version,
        subjects,
        references,
        publisher,
        ..Data::default()
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_yaml(input: &str) -> Result<Data> {
    let doc: Value = serde_yaml::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_value(&doc))
}

/// Fetch a CITATION.cff from a GitHub repository URL and parse it.
pub fn fetch(url: &str) -> Result<Data> {
    let cff_url = github_as_cff_url(url)
        .ok_or_else(|| Error::Parse(format!("cannot derive CITATION.cff URL from: {}", url)))?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let text = client
        .get(&cff_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;

    let mut doc: Value =
        serde_yaml::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;

    // If repository-code is absent, fill it from the canonical repo URL
    if get(&doc, "repository-code") == null_val() {
        if let Some(repo_url) = github_as_repo_url(&cff_url) {
            if let Value::Mapping(ref mut m) = doc {
                m.insert(
                    Value::String("repository-code".to_string()),
                    Value::String(repo_url),
                );
            }
        }
    }

    Ok(from_value(&doc))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFF_SOFTWARE: &str = r#"
cff-version: 1.2.0
title: My Research Software
authors:
  - family-names: Smith
    given-names: John
    orcid: https://orcid.org/0000-0002-1825-0097
    affiliation: University of Example
  - name: ACME Research Group
doi: 10.5281/zenodo.1234567
version: 2.1.0
date-released: 2024-03-15
abstract: A software tool for research.
license: MIT
keywords:
  - research
  - data science
repository-code: https://github.com/example/my-software
"#;

    #[test]
    fn test_read_cff_basic() {
        let data = read_yaml(CFF_SOFTWARE).unwrap();

        assert_eq!(data.type_, "Software");
        assert_eq!(data.id, "https://doi.org/10.5281/zenodo.1234567");
        assert_eq!(data.url, "https://github.com/example/my-software");
        assert_eq!(data.titles[0].title, "My Research Software");
        assert_eq!(data.version, "2.1.0");
        assert_eq!(data.date.published, "2024-03-15");
        assert_eq!(data.license.id, "MIT");
        assert_eq!(data.publisher.name, "GitHub");
    }

    #[test]
    fn test_cff_contributors() {
        let data = read_yaml(CFF_SOFTWARE).unwrap();

        assert_eq!(data.contributors.len(), 2);

        let person = &data.contributors[0];
        assert_eq!(person.type_, "Person");
        assert_eq!(person.family_name, "Smith");
        assert_eq!(person.given_name, "John");
        assert_eq!(person.id, "https://orcid.org/0000-0002-1825-0097");
        assert_eq!(person.affiliations[0].name, "University of Example");

        let org = &data.contributors[1];
        assert_eq!(org.type_, "Organization");
        assert_eq!(org.name, "ACME Research Group");
    }

    #[test]
    fn test_cff_subjects() {
        let data = read_yaml(CFF_SOFTWARE).unwrap();
        assert_eq!(data.subjects.len(), 2);
        assert_eq!(data.subjects[0].subject, "research");
        assert_eq!(data.subjects[1].subject, "data science");
    }

    #[test]
    fn test_cff_description() {
        let data = read_yaml(CFF_SOFTWARE).unwrap();
        assert_eq!(data.descriptions.len(), 1);
        assert_eq!(data.descriptions[0].description, "A software tool for research.");
        assert_eq!(data.descriptions[0].type_, "Abstract");
    }

    #[test]
    fn test_github_as_cff_url() {
        assert_eq!(
            github_as_cff_url("https://github.com/owner/repo"),
            Some("https://raw.githubusercontent.com/owner/repo/main/CITATION.cff".to_string())
        );
        assert_eq!(
            github_as_cff_url("https://github.com/owner/repo/tree/v1.0/CITATION.cff"),
            Some("https://raw.githubusercontent.com/owner/repo/v1.0/CITATION.cff".to_string())
        );
    }

    #[test]
    fn test_cff_references() {
        let cff = r#"
cff-version: 1.2.0
title: Test
authors:
  - name: Author
references:
  - type: article
    title: Related paper
    identifiers:
      - type: doi
        value: 10.1000/ref.2024
  - type: book
    title: No DOI book
"#;
        let data = read_yaml(cff).unwrap();
        assert_eq!(data.references.len(), 1);
        assert_eq!(data.references[0].id, "https://doi.org/10.1000/ref.2024");
    }

    #[test]
    fn test_cff_license_list() {
        let cff = r#"
cff-version: 1.2.0
title: Test
authors:
  - name: Author
license:
  - Apache-2.0
  - MIT
"#;
        let data = read_yaml(cff).unwrap();
        assert_eq!(data.license.id, "Apache-2.0");
    }
}
