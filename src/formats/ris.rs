use std::collections::HashMap;

use crate::author_utils::{cleanup_author, infer_contributor_type, split_person_name};
use crate::constants as C;
use crate::data::{Container, Contributor, Data, Identifier, Organization, Person, Publisher, Subject};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::Result;

fn ris_to_cm_type(ris: &str) -> &'static str {
    C::ris_to_cm(ris)
}

// Parse RIS text into a field map. Each tag maps to one or more values.
// Uses "  - " (two spaces, hyphen, space) as the tag/value separator per the RIS spec,
// which avoids incorrectly splitting on hyphens that appear inside field values.
fn parse_ris(data: &str) -> HashMap<String, Vec<String>> {
    let mut meta: HashMap<String, Vec<String>> = HashMap::new();
    for line in data.lines() {
        if let Some(idx) = line.find("  - ") {
            let key = line[..idx].trim();
            let value = line[idx + 4..].trim();
            if !key.is_empty() && !value.is_empty() {
                meta.entry(key.to_string())
                    .or_default()
                    .push(value.to_string());
            }
        }
    }
    meta
}

fn first_val<'a>(meta: &'a HashMap<String, Vec<String>>, key: &str) -> &'a str {
    meta.get(key)
        .and_then(|v| v.first())
        .map(|s| s.as_str())
        .unwrap_or("")
}

// Parse author name: "Family, Given" → Person, "Given Family" → Person, single token → Organization
fn parse_author(name: &str) -> Contributor {
    let cleaned = cleanup_author(Some(name)).unwrap_or_else(|| name.trim().to_string());
    let (given_name, family_name, fallback_name) = split_person_name(&cleaned);
    let type_ = infer_contributor_type(
        "",
        "",
        &given_name,
        &family_name,
        &cleaned,
        None,
    );

    if type_ == "Person" {
        Contributor::person(
            Person {
                given_name,
                family_name,
                ..Default::default()
            },
            vec!["Author".to_string()],
        )
    } else {
        Contributor::organization(
            Organization {
                name: fallback_name,
                ..Default::default()
            },
            vec!["Author".to_string()],
        )
    }
}

// Parse date from "YYYY", "YYYY/MM", or "YYYY/MM/DD" into ISO 8601
fn parse_ris_date(s: &str) -> String {
    let parts: Vec<&str> = s.split('/').collect();
    match parts.len() {
        3 => {
            let y = parts[0].trim();
            let m = parts[1].trim();
            let d = parts[2].trim();
            if m.is_empty() {
                y.to_string()
            } else if d.is_empty() {
                format!("{}-{:0>2}", y, m)
            } else {
                format!("{}-{:0>2}-{:0>2}", y, m, d)
            }
        }
        2 => {
            let y = parts[0].trim();
            let m = parts[1].trim();
            if m.is_empty() {
                y.to_string()
            } else {
                format!("{}-{:0>2}", y, m)
            }
        }
        _ => parts[0].trim().to_string(),
    }
}

pub fn read(input: &str) -> Result<Data> {
    let meta = parse_ris(input);

    let ty = first_val(&meta, "TY");
    let type_ = ris_to_cm_type(ty).to_string();

    // DOI
    let id = {
        let raw = first_val(&meta, "DO");
        normalize_doi(raw)
    };

    let mut identifiers = Vec::new();
    if !id.is_empty() {
        identifiers.push(Identifier {
            identifier: id.clone(),
            identifier_type: "DOI".to_string(),
            ..Default::default()
        });
    }

    // URL
    let url = first_val(&meta, "UR").to_string();

    // Title
    let title = first_val(&meta, "T1").to_string();

    // Contributors from AU field
    let contributors: Vec<Contributor> = meta
        .get("AU")
        .map(|authors| authors.iter().map(|a| parse_author(a)).collect())
        .unwrap_or_default();

    // Dates
    let mut date_published = String::new();
    let py = first_val(&meta, "PY");
    if !py.is_empty() {
        date_published = parse_ris_date(py);
    }
    let mut date_created = String::new();
    let y1 = first_val(&meta, "Y1");
    if !y1.is_empty() {
        date_created = parse_ris_date(y1);
    }

    // Description (abstract)
    let description = first_val(&meta, "AB").to_string();

    // Container (from T2 secondary title)
    let t2 = first_val(&meta, "T2");
    let container = if !t2.is_empty() {
        let container_type = if type_ == "JournalArticle" {
            "Journal"
        } else {
            ""
        };
        Container {
            type_: container_type.to_string(),
            title: t2.to_string(),
            volume: first_val(&meta, "VL").to_string(),
            issue: first_val(&meta, "IS").to_string(),
            first_page: first_val(&meta, "SP").to_string(),
            last_page: first_val(&meta, "EP").to_string(),
            ..Default::default()
        }
    } else {
        Container::default()
    };

    // Publisher
    let pb = first_val(&meta, "PB");
    let publisher = Publisher {
        name: pb.to_string(),
        ..Default::default()
    };

    // Subjects from KW (keyword) field
    let subjects: Vec<Subject> = meta
        .get("KW")
        .map(|kws| {
            kws.iter()
                .map(|k| Subject {
                    subject: k.clone(),
                    ..Default::default()
                })
                .collect()
        })
        .unwrap_or_default();

    // Language
    let language = first_val(&meta, "LA").to_string();

    Ok(Data {
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
        description,
        container,
        publisher,
        identifiers,
        subjects,
        language,
        ..Data::default()
    })
}

// ── Writer ────────────────────────────────────────────────────────────────────

fn cm_to_ris_type(cm: &str) -> &'static str {
    C::cm_to_ris(cm)
}

// Format a contributor as "Family, Given" or fall back to name (for organizations)
fn contributor_to_ris(c: &Contributor) -> Option<String> {
    if !c.family_name().is_empty() {
        let mut s = c.family_name().to_string();
        if !c.given_name().is_empty() {
            s.push_str(", ");
            s.push_str(c.given_name());
        }
        Some(s)
    } else {
        let name = c.name();
        if name.is_empty() { None } else { Some(name) }
    }
}

fn doi_from_identifiers(data: &Data) -> Option<String> {
    data.identifiers
        .iter()
        .find(|id| id.identifier_type == "DOI" && !id.identifier.is_empty())
        .and_then(|id| validate_doi(&id.identifier))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let mut lines: Vec<String> = Vec::new();

    macro_rules! field {
        ($key:expr, $val:expr) => {
            let v: &str = $val;
            if !v.is_empty() {
                lines.push(format!("{}  - {}", $key, v));
            }
        };
    }

    // TY must be first
    lines.push(format!("TY  - {}", cm_to_ris_type(&data.type_)));

    // T1 – title
    if !data.title.is_empty() {
        lines.push(format!("T1  - {}", data.title));
    }

    // T2 – container title
    if !data.container.title.is_empty() {
        lines.push(format!("T2  - {}", data.container.title));
    }

    // AU – authors (one line each, Authors only)
    for c in &data.contributors {
        if c.roles.contains(&"Author".to_string())
            && let Some(name) = contributor_to_ris(c)
        {
            lines.push(format!("AU  - {}", name));
        }
    }

    // DO – DOI (bare, without https://doi.org/ prefix)
    if let Some(doi) = doi_from_identifiers(data).or_else(|| validate_doi(&data.id)) {
        lines.push(format!("DO  - {}", doi));
    }

    // UR – URL
    field!("UR", &data.url);

    // AB – abstract
    if !data.description.is_empty() {
        lines.push(format!("AB  - {}", data.description));
    }

    // KW – keywords (one line each)
    for s in &data.subjects {
        if !s.subject.is_empty() {
            lines.push(format!("KW  - {}", s.subject));
        }
    }

    // PY – publication year (first 4 chars)
    if !data.date_published.is_empty() {
        let year: &str = if data.date_published.len() >= 4 {
            &data.date_published[..4]
        } else {
            &data.date_published
        };
        lines.push(format!("PY  - {}", year));
    }

    // PB – publisher
    field!("PB", &data.publisher.name);

    // LA – language
    field!("LA", &data.language);

    // VL IS SP EP – container fields
    field!("VL", &data.container.volume);
    field!("IS", &data.container.issue);
    field!("SP", &data.container.first_page);
    field!("EP", &data.container.last_page);

    // ER – end of record (always last, empty value)
    lines.push("ER  - ".to_string());

    Ok(lines.join("\r\n").into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL_ARTICLE_RIS: &str = "\
TY  - JOUR
AU  - Smith, John
AU  - Doe, Jane
T1  - A Test Article
T2  - Journal of Testing
VL  - 5
IS  - 2
SP  - 100
EP  - 110
PY  - 2023
DO  - 10.1000/test-article
AB  - This is the abstract.
PB  - Test Publisher
KW  - keyword1
KW  - keyword2
LA  - en
UR  - https://example.org/article
ER  - \
";

    #[test]
    fn test_parse_ris_journal_article() {
        let data = read(JOURNAL_ARTICLE_RIS).unwrap();
        assert_eq!(data.type_, "JournalArticle");
        assert_eq!(data.id, "https://doi.org/10.1000/test-article");
        assert_eq!(data.identifiers.len(), 1);
        assert_eq!(data.identifiers[0].identifier_type, "DOI");
        assert_eq!(data.identifiers[0].identifier, "https://doi.org/10.1000/test-article");
        assert_eq!(data.title, "A Test Article");
        assert_eq!(data.contributors.len(), 2);
        assert_eq!(data.contributors[0].family_name(), "Smith");
        assert_eq!(data.contributors[0].given_name(), "John");
        assert_eq!(data.contributors[1].family_name(), "Doe");
        assert_eq!(data.date_published, "2023");
        assert_eq!(data.description, "This is the abstract.");
        assert_eq!(data.container.title, "Journal of Testing");
        assert_eq!(data.container.type_, "Journal");
        assert_eq!(data.container.volume, "5");
        assert_eq!(data.container.issue, "2");
        assert_eq!(data.container.first_page, "100");
        assert_eq!(data.container.last_page, "110");
        assert_eq!(data.publisher.name, "Test Publisher");
        assert_eq!(data.subjects.len(), 2);
        assert_eq!(data.subjects[0].subject, "keyword1");
        assert_eq!(data.language, "en");
        assert_eq!(data.url, "https://example.org/article");
    }

    #[test]
    fn test_parse_ris_date_formats() {
        assert_eq!(parse_ris_date("2023"), "2023");
        assert_eq!(parse_ris_date("2023/06"), "2023-06");
        assert_eq!(parse_ris_date("2023/06/15"), "2023-06-15");
    }

    #[test]
    fn test_parse_author_formats() {
        let a = parse_author("Smith, John");
        assert_eq!(a.family_name(), "Smith");
        assert_eq!(a.given_name(), "John");
        assert_eq!(a.type_, "Person");

        let b = parse_author("John Smith");
        assert_eq!(b.family_name(), "Smith");
        assert_eq!(b.given_name(), "John");

        // Single token without spaces → Organization
        let c = parse_author("NIH");
        assert_eq!(c.type_, "Organization");
        assert_eq!(c.name(), "NIH");
    }

    #[test]
    fn test_ris_type_mapping() {
        assert_eq!(ris_to_cm_type("JOUR"), "JournalArticle");
        assert_eq!(ris_to_cm_type("BOOK"), "Book");
        assert_eq!(ris_to_cm_type("THES"), "Dissertation");
        assert_eq!(ris_to_cm_type("DATA"), "Dataset");
        assert_eq!(ris_to_cm_type("BLOG"), "BlogPost");
        assert_eq!(ris_to_cm_type("UNKNOWN"), "Other");
    }

    #[test]
    fn test_cm_to_ris_type_mapping() {
        assert_eq!(cm_to_ris_type("JournalArticle"), "JOUR");
        assert_eq!(cm_to_ris_type("Book"), "BOOK");
        assert_eq!(cm_to_ris_type("Dissertation"), "THES");
        assert_eq!(cm_to_ris_type("Dataset"), "DATA");
        assert_eq!(cm_to_ris_type("BlogPost"), "BLOG");
        assert_eq!(cm_to_ris_type("Unknown"), "GEN");
    }

    #[test]
    fn test_write_ris_roundtrip() {
        let input = read(JOURNAL_ARTICLE_RIS).unwrap();
        let output = write(&input).unwrap();
        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("TY  - JOUR"));
        assert!(output_str.contains("T1  - A Test Article"));
        assert!(output_str.contains("AU  - Smith, John"));
        assert!(output_str.contains("AU  - Doe, Jane"));
        assert!(output_str.contains("DO  - 10.1000/test-article"));
        assert!(output_str.contains("T2  - Journal of Testing"));
        assert!(output_str.contains("PY  - 2023"));
        assert!(output_str.contains("AB  - This is the abstract."));
        assert!(output_str.contains("KW  - keyword1"));
        assert!(output_str.contains("KW  - keyword2"));
        assert!(output_str.contains("PB  - Test Publisher"));
        assert!(output_str.contains("LA  - en"));
        assert!(output_str.contains("VL  - 5"));
        assert!(output_str.contains("IS  - 2"));
        assert!(output_str.contains("SP  - 100"));
        assert!(output_str.contains("EP  - 110"));
        assert!(output_str.contains("ER  - "));
        // TY must be first line
        assert!(output_str.starts_with("TY  - JOUR"));
        // ER must be last line (trailing space is part of ER tag format)
        let last_line = output_str.lines().last().unwrap_or("");
        assert_eq!(last_line.trim(), "ER  -");
    }

    #[test]
    fn test_write_prefers_doi_identifier_over_id() {
        let data = Data {
            id: "https://example.org/not-a-doi".to_string(),
            type_: "JournalArticle".to_string(),
            identifiers: vec![Identifier {
                identifier: "https://doi.org/10.5555/from-identifiers".to_string(),
                identifier_type: "DOI".to_string(),
                ..Default::default()
            }],
            ..Data::default()
        };

        let output = String::from_utf8(write(&data).unwrap()).unwrap();
        assert!(output.contains("DO  - 10.5555/from-identifiers"));
    }

    #[test]
    fn test_write_uses_title_for_t1() {
        let data = Data {
            type_: "JournalArticle".to_string(),
            title: "Primary Title".to_string(),
            ..Data::default()
        };

        let output = String::from_utf8(write(&data).unwrap()).unwrap();
        assert!(output.contains("T1  - Primary Title"));
    }
}
