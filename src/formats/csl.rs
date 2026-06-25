use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::author_utils::normalize_contributor_roles;
use crate::constants as C;
use crate::data::{
    Container, Contributor, Data, Identifier, Organization, Person, Publisher, Relation, Subject,
    Title,
};
use crate::doi_utils::normalize_doi;
use crate::error::{Error, Result};
use crate::utils::{get_language, issn_as_url, normalize_url, sanitize, validate_id};

// ─── Reader input structs ─────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct CslAuthor {
    #[serde(default)]
    family: String,
    #[serde(default)]
    given: String,
    #[serde(default)]
    literal: String,
}

/// Date parts are `[[year, month, day]]` where each element may be int or string.
#[derive(Deserialize, Default)]
struct CslDateField {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Value>>,
}

impl CslDateField {
    fn to_iso(&self) -> String {
        let parts = match self.date_parts.first() {
            Some(p) => p,
            None => return String::new(),
        };
        let nums: Vec<i32> = parts
            .iter()
            .filter_map(|v| match v {
                Value::Number(n) => n.as_i64().map(|i| i as i32),
                Value::String(s) => s.parse().ok(),
                _ => None,
            })
            .collect();
        match nums.as_slice() {
            [y] => format!("{:04}", y),
            [y, m] => format!("{:04}-{:02}", y, m),
            [y, m, d, ..] => format!("{:04}-{:02}-{:02}", y, m, d),
            _ => String::new(),
        }
    }
}

/// The flexible CSL input struct. `publisher` can be a plain string or
/// `{"name": "..."}`, so we keep it as raw `Value`.
#[derive(Deserialize, Default)]
struct CslContent {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(rename = "abstract", default)]
    abstract_: String,
    #[serde(default)]
    accessed: CslDateField,
    #[serde(default)]
    author: Vec<CslAuthor>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(rename = "container-title", default)]
    container_title: String,
    #[serde(rename = "DOI", default)]
    doi: String,
    #[serde(default)]
    editor: Vec<CslAuthor>,
    #[serde(rename = "ISSN", default)]
    issn: String,
    #[serde(default)]
    issue: String,
    #[serde(default)]
    issued: CslDateField,
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    language: String,
    #[serde(default)]
    license: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    page: String,
    #[serde(rename = "PMID", default)]
    pmid: String,
    // publisher is string or {name: string}
    #[serde(default)]
    publisher: Option<Value>,
    #[serde(default)]
    submitted: CslDateField,
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(rename = "URL", default)]
    url: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    volume: String,
}

// ─── CSL → CM type mapping ────────────────────────────────────────────────────

fn csl_to_cm_type(csl: &str) -> &'static str {
    C::csl_to_cm(csl)
}

// ─── Core reader ──────────────────────────────────────────────────────────────

fn from_csl(content: CslContent) -> Data {
    let mut data = Data::default();

    // ID: DOI > "DOI: " note prefix > URL
    if !content.doi.is_empty() {
        data.id = normalize_doi(&content.doi);
    } else if content.note.starts_with("DOI: ") {
        let doi = content.note.trim_start_matches("DOI: ");
        data.id = normalize_doi(doi);
    } else if !content.url.is_empty() {
        data.id = content.url.clone();
    }

    // Type
    let cm_type = csl_to_cm_type(&content.type_);
    data.type_ = if cm_type.is_empty() {
        "Other".to_string()
    } else {
        cm_type.to_string()
    };

    // ISSN → relation + container identifier
    let (identifier, identifier_type) = if content.issn.len() >= 9 {
        // strip extra info like "(Electronic)"
        let issn = content.issn[..9].to_string();
        data.relations.push(Relation {
            id: issn_as_url(&issn),
            type_: "IsPartOf".to_string(),
            ..Default::default()
        });
        (issn, "ISSN".to_string())
    } else {
        (String::new(), String::new())
    };

    // Page range
    let (first_page, last_page) = if !content.page.is_empty() {
        let parts: Vec<&str> = content.page.splitn(2, '-').collect();
        let first = parts[0].to_string();
        let last = if parts.len() > 1 && parts[1] > parts[0] {
            parts[1].to_string()
        } else {
            String::new()
        };
        (first, last)
    } else {
        (String::new(), String::new())
    };

    // Container
    let container_type = match data.type_.as_str() {
        "JournalArticle" | "JournalIssue" | "JournalVolume" | "Journal" => "Periodical",
        "ProceedingsArticle" | "Proceedings" => "Proceedings",
        "BookChapter" | "Book" => "Book",
        "Dataset" => "DataRepository",
        _ => "",
    };

    data.container = Container {
        type_: container_type.to_string(),
        title: content.container_title.clone(),
        identifier,
        identifier_type,
        volume: content.volume.clone(),
        issue: content.issue.clone(),
        first_page,
        last_page,
        ..Default::default()
    };

    // Contributors: authors
    for a in &content.author {
        let roles = normalize_contributor_roles(&["Author".to_string()], "Author");
        let c = if !a.literal.is_empty() {
            Contributor::organization(
                Organization { name: a.literal.clone(), ..Default::default() },
                roles,
            )
        } else {
            Contributor::person(
                Person {
                    given_name: a.given.clone(),
                    family_name: a.family.clone(),
                    ..Default::default()
                },
                roles,
            )
        };
        data.contributors.push(c);
    }

    // Contributors: editors
    for e in &content.editor {
        let roles = normalize_contributor_roles(&["Editor".to_string()], "Editor");
        let c = if !e.literal.is_empty() {
            Contributor::organization(
                Organization { name: e.literal.clone(), ..Default::default() },
                roles,
            )
        } else {
            Contributor::person(
                Person {
                    given_name: e.given.clone(),
                    family_name: e.family.clone(),
                    ..Default::default()
                },
                roles,
            )
        };
        data.contributors.push(c);
    }

    // Dates
    let published = content.issued.to_iso();
    if !published.is_empty() {
        data.date_published = published;
    }
    let submitted = content.submitted.to_iso();
    if !submitted.is_empty() {
        data.dates.submitted = submitted;
    }
    let accessed = content.accessed.to_iso();
    if !accessed.is_empty() {
        data.dates.accessed = accessed;
    }

    // Description
    if !content.abstract_.is_empty() {
        data.description = sanitize(&content.abstract_);
    }

    // Identifiers: CSL `id` field (if not DOI)
    if !content.id.is_empty() {
        let (id_val, id_type) = validate_id(&content.id);
        let id_val = if id_val.is_empty() {
            content.id.clone()
        } else {
            id_val
        };
        let id_type = if id_type.is_empty() { "Other" } else { id_type };
        if id_type != "DOI" {
            data.identifiers.push(Identifier {
                identifier: id_val,
                identifier_type: id_type.to_string(),
                ..Default::default()
            });
        }
    }

    // PMID identifier
    if !content.pmid.is_empty() {
        data.identifiers.push(Identifier {
            identifier: content.pmid.clone(),
            identifier_type: "PMID".to_string(),
            ..Default::default()
        });
    }

    // Language
    if !content.language.is_empty() {
        data.language = get_language(&content.language, "iso639-1");
    }

    // License
    if !content.license.is_empty()
        && let Some(url) = normalize_url(&content.license, true, true)
    {
        data.license = crate::spdx::from_url(&url);
    }

    // Publisher — string or {name: string}
    if let Some(pub_val) = &content.publisher {
        let name = if let Some(s) = pub_val.as_str() {
            s.to_string()
        } else if let Some(obj) = pub_val.as_object() {
            obj.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        if !name.is_empty() {
            data.publisher = Publisher {
                name,
                ..Default::default()
            };
        }
    }

    // Subjects: keyword (comma-separated) or categories array
    if !content.keyword.is_empty() {
        for kw in content.keyword.split(',') {
            let kw = kw.trim().to_string();
            if !kw.is_empty() {
                data.subjects.push(Subject { subject: kw, ..Default::default() });
            }
        }
    } else {
        for cat in &content.categories {
            if !cat.is_empty() {
                data.subjects.push(Subject { subject: cat.clone(), ..Default::default() });
            }
        }
    }

    // Title
    let title = sanitize(&content.title);
    if !title.is_empty() {
        data.title = title;
    }
    let subtitle = sanitize(&content.subtitle);
    if !subtitle.is_empty() {
        data.additional_titles.push(Title {
            title: subtitle,
            type_: "Subtitle".to_string(),
            ..Default::default()
        });
    }

    // URL
    if let Some(url) = normalize_url(&content.url, true, false) {
        data.url = url;
    }

    // Version
    data.version = content.version.clone();

    data
}

// ─── Public reader API ────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let content: CslContent =
        serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_csl(content))
}

// ─── CSL-JSON output structs ─────────────────────────────────────────────────

#[derive(Default, Serialize)]
struct CslRecord {
    id: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    author: Vec<CslName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    editor: Vec<CslName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issued: Option<CslDate>,
    #[serde(rename = "DOI", skip_serializing_if = "String::is_empty")]
    doi: String,
    #[serde(rename = "URL", skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(rename = "container-title", skip_serializing_if = "String::is_empty")]
    container_title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    volume: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issue: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    page: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    publisher: String,
    #[serde(rename = "ISSN", skip_serializing_if = "String::is_empty")]
    issn: String,
    #[serde(rename = "abstract", skip_serializing_if = "String::is_empty")]
    abstract_text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    language: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    source: String,
}

#[derive(Default, Serialize)]
struct CslName {
    #[serde(skip_serializing_if = "String::is_empty")]
    family: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    given: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    literal: String,
}

#[derive(Serialize)]
struct CslDate {
    #[serde(rename = "date-parts")]
    date_parts: Vec<Vec<i32>>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn to_csl_type(t: &str) -> &str {
    match t {
        "JournalArticle" => "article-journal",
        "BookChapter" => "chapter",
        "Book" | "EditedBook" => "book",
        "ProceedingsArticle" => "paper-conference",
        "Proceedings" => "event",
        "Dataset" => "dataset",
        "Dissertation" => "thesis",
        "Preprint" => "article",
        "Report" => "report",
        "PeerReview" => "peer-review",
        "Entry" | "EntryDictionary" | "EntryEncyclopedia" => "entry-encyclopedia",
        "Journal" => "periodical",
        "WebPage" => "webpage",
        "Software" => "software",
        "Standard" => "standard",
        _ => "document",
    }
}

fn parse_iso_date(s: &str) -> Option<CslDate> {
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split('-');
    let year: i32 = parts.next()?.parse().ok()?;
    let date_parts = match (parts.next(), parts.next()) {
        (Some(m), Some(d)) => {
            vec![year, m.parse().ok()?, d.parse().ok()?]
        }
        (Some(m), None) => vec![year, m.parse().ok()?],
        _ => vec![year],
    };
    Some(CslDate {
        date_parts: vec![date_parts],
    })
}

fn to_csl_name(c: &crate::data::Contributor) -> CslName {
    if c.type_ == "Organization" || (c.family_name().is_empty() && !c.name().is_empty()) {
        CslName {
            literal: c.name(),
            ..Default::default()
        }
    } else {
        CslName {
            family: c.family_name().to_string(),
            given: c.given_name().to_string(),
            ..Default::default()
        }
    }
}

fn bare_doi(id: &str) -> String {
    id.trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/")
        .to_string()
}

fn doi_from_identifiers(data: &Data) -> Option<String> {
    data.identifiers
        .iter()
        .find(|i| i.identifier_type == "DOI" && !i.identifier.is_empty())
        .map(|i| i.identifier.clone())
}

fn convert(data: &Data) -> CslRecord {
    let doi_source = doi_from_identifiers(data).unwrap_or_else(|| data.id.clone());
    let doi = if doi_source.contains("doi.org") {
        bare_doi(&doi_source)
    } else {
        String::new()
    };
    let csl_id = if doi.is_empty() {
        data.id.clone()
    } else {
        doi.clone()
    };

    let title = data.title.clone();
    let subtitle = data
        .additional_titles
        .iter()
        .find(|t| !t.title.is_empty() && t.type_ == "Subtitle")
        .map(|t| t.title.clone())
        .unwrap_or_default();

    let author: Vec<CslName> = data
        .contributors
        .iter()
        .filter(|c| c.roles.iter().any(|r| r == "Author"))
        .map(to_csl_name)
        .collect();

    let editor: Vec<CslName> = data
        .contributors
        .iter()
        .filter(|c| c.roles.iter().any(|r| r == "Editor"))
        .map(to_csl_name)
        .collect();

    let issued =
        parse_iso_date(&data.date_published).or_else(|| parse_iso_date(&data.dates.created));

    let container = &data.container;
    let page = match (container.first_page.as_str(), container.last_page.as_str()) {
        ("", _) => String::new(),
        (f, "") => f.to_string(),
        (f, l) => format!("{f}-{l}"),
    };
    let issn = if container.identifier_type == "ISSN" {
        container.identifier.clone()
    } else {
        String::new()
    };

    let abstract_text = data.description.clone();

    CslRecord {
        id: csl_id,
        type_: to_csl_type(&data.type_).to_string(),
        title: if subtitle.is_empty() {
            title
        } else {
            format!("{}: {}", title, subtitle)
        },
        author,
        editor,
        issued,
        doi,
        url: data.url.clone(),
        container_title: container.title.clone(),
        volume: container.volume.clone(),
        issue: container.issue.clone(),
        page,
        publisher: data.publisher.name.clone(),
        issn,
        abstract_text,
        language: data.language.clone(),
        source: data.provider.clone(),
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let record = convert(data);
    serde_json::to_vec_pretty(&record).map_err(|e| Error::Serialize(e.to_string()))
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let records: Vec<CslRecord> = list.iter().map(convert).collect();
    serde_json::to_vec_pretty(&records).map_err(|e| Error::Serialize(e.to_string()))
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> Data {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/commonmeta")
            .join(name);
        let json = std::fs::read_to_string(path).expect("fixture");
        crate::formats::commonmeta::read(&json).expect("parse")
    }

    #[test]
    fn journal_article_fields() {
        let data = load_fixture("journal_article.json");
        let out = write(&data).unwrap();
        let csl: serde_json::Value = serde_json::from_slice(&out).unwrap();

        assert_eq!(csl["type"], "article-journal");
        assert_eq!(csl["id"], "10.5555/12345678");
        assert_eq!(csl["title"], "A Study of Things");
        assert_eq!(csl["DOI"], "10.5555/12345678");
        assert_eq!(csl["container-title"], "Journal of Examples");
        assert_eq!(csl["ISSN"], "1234-5678");
        assert_eq!(csl["volume"], "12");
        assert_eq!(csl["issue"], "3");
        assert_eq!(csl["page"], "100-110");
        assert_eq!(csl["publisher"], "Example Publisher");
        assert_eq!(csl["language"], "en");
        assert_eq!(csl["author"][0]["family"], "Lovelace");
        assert_eq!(csl["author"][0]["given"], "Ada");
        assert_eq!(csl["issued"]["date-parts"][0][0], 2024);
        assert_eq!(csl["issued"]["date-parts"][0][1], 3);
        assert_eq!(csl["issued"]["date-parts"][0][2], 15);
    }

    #[test]
    fn type_mapping() {
        assert_eq!(to_csl_type("JournalArticle"), "article-journal");
        assert_eq!(to_csl_type("BookChapter"), "chapter");
        assert_eq!(to_csl_type("Dissertation"), "thesis");
        assert_eq!(to_csl_type("Preprint"), "article");
        assert_eq!(to_csl_type("Dataset"), "dataset");
        assert_eq!(to_csl_type("Unknown"), "document");
    }

    #[test]
    fn date_parsing() {
        let d = parse_iso_date("2024-03-15").unwrap();
        assert_eq!(d.date_parts[0], vec![2024, 3, 15]);

        let d = parse_iso_date("2024-03").unwrap();
        assert_eq!(d.date_parts[0], vec![2024, 3]);

        let d = parse_iso_date("2024").unwrap();
        assert_eq!(d.date_parts[0], vec![2024]);

        assert!(parse_iso_date("").is_none());
    }

    #[test]
    fn read_json_maps_subtitle_to_title_variant() {
        let json = r#"{
            "id": "10.1/test",
            "type": "article-journal",
            "title": "Main Title",
            "subtitle": "Sub Title"
        }"#;

        let data = read_json(json).unwrap();
        assert_eq!(data.title, "Main Title");
        assert_eq!(data.additional_titles.len(), 1);
        assert_eq!(data.additional_titles[0].title, "Sub Title");
        assert_eq!(data.additional_titles[0].type_, "Subtitle");
    }

    #[test]
    fn write_prefers_doi_identifier_over_id_for_doi_field() {
        let mut data = Data {
            id: "https://example.org/not-a-doi".to_string(),
            type_: "JournalArticle".to_string(),
            ..Default::default()
        };
        data.identifiers.push(Identifier {
            identifier: "https://doi.org/10.1234/identifier".to_string(),
            identifier_type: "DOI".to_string(),
            ..Default::default()
        });

        let out = write(&data).unwrap();
        let csl: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(csl["DOI"], "10.1234/identifier");
        assert_eq!(csl["id"], "10.1234/identifier");
    }

    #[test]
    fn read_json_sets_container_type_by_resource_kind() {
        let json = r#"{
            "id": "10.1/test",
            "type": "paper-conference",
            "title": "Conference Paper",
            "container-title": "Proceedings Title"
        }"#;

        let data = read_json(json).unwrap();
        assert_eq!(data.type_, "ProceedingsArticle");
        assert_eq!(data.container.type_, "Proceedings");
    }
}
