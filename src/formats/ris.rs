use std::collections::HashMap;

use crate::data::{Container, Contributor, Data, Date, Description, Publisher, Subject, Title};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::Result;

fn ris_to_cm_type(ris: &str) -> &'static str {
    match ris {
        "ABST" | "ADVS" | "AGGR" | "ANCIENT" | "ART" | "BILL" | "CASE" | "CHART"
        | "CLSWK" | "ELEC" | "ICOMM" | "INPR" | "MANSCPT" => "Text",
        "BLOG" => "BlogPost",
        "BOOK" | "EBOOK" | "EDBOOK" => "Book",
        "CHAP" | "ECHAP" => "BookChapter",
        "CTLG" => "Collection",
        "COMP" => "Software",
        "DATA" => "Dataset",
        "DBASE" => "Dataset",
        "DICT" => "Dictionary",
        "EJOUR" | "JFULL" | "JOUR" => "JournalArticle",
        "ENCYC" => "Encyclopedia",
        "EQUA" => "Equation",
        "FIGURE" => "Image",
        "GEN" => "CreativeWork",
        "GOVDOC" => "GovernmentDocument",
        "GRANT" => "Grant",
        "HEAR" => "Hearing",
        "LEGAL" => "LegalRuleOrRegulation",
        "MAP" => "Map",
        "MGZN" => "MagazineArticle",
        "MPCT" | "MULTI" | "VIDEO" => "Audiovisual",
        "MUSIC" => "MusicScore",
        "NEWS" => "NewspaperArticle",
        "PAMP" => "Pamphlet",
        "PAT" => "Patent",
        "PCOMM" => "PersonalCommunication",
        "RPRT" => "Report",
        "SER" => "SerialPublication",
        "SLIDE" => "Slide",
        "SOUND" => "SoundRecording",
        "STAND" => "Standard",
        "THES" => "Dissertation",
        "UNBILL" => "UnenactedBill",
        "UNPB" => "UnpublishedWork",
        "WEB" => "WebPage",
        _ => "Other",
    }
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
                meta.entry(key.to_string()).or_default().push(value.to_string());
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
    let name = name.trim();
    if let Some(comma) = name.find(',') {
        let family = name[..comma].trim().to_string();
        let given = name[comma + 1..].trim().to_string();
        Contributor {
            type_: "Person".to_string(),
            given_name: given,
            family_name: family,
            contributor_roles: vec!["Author".to_string()],
            ..Default::default()
        }
    } else if name.contains(' ') {
        // "First Last" — split at last space
        let idx = name.rfind(' ').unwrap();
        let given = name[..idx].trim().to_string();
        let family = name[idx + 1..].trim().to_string();
        Contributor {
            type_: "Person".to_string(),
            given_name: given,
            family_name: family,
            contributor_roles: vec!["Author".to_string()],
            ..Default::default()
        }
    } else {
        Contributor {
            type_: "Organization".to_string(),
            name: name.to_string(),
            contributor_roles: vec!["Author".to_string()],
            ..Default::default()
        }
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

    // URL
    let url = first_val(&meta, "UR").to_string();

    // Titles
    let t1 = first_val(&meta, "T1");
    let titles = if !t1.is_empty() {
        vec![Title {
            title: t1.to_string(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

    // Contributors from AU field
    let contributors: Vec<Contributor> = meta
        .get("AU")
        .map(|authors| authors.iter().map(|a| parse_author(a)).collect())
        .unwrap_or_default();

    // Dates
    let mut date = Date::default();
    let py = first_val(&meta, "PY");
    if !py.is_empty() {
        date.published = parse_ris_date(py);
    }
    let y1 = first_val(&meta, "Y1");
    if !y1.is_empty() {
        date.created = parse_ris_date(y1);
    }

    // Description (abstract)
    let ab = first_val(&meta, "AB");
    let descriptions = if !ab.is_empty() {
        vec![Description {
            description: ab.to_string(),
            type_: "Abstract".to_string(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

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
        titles,
        contributors,
        date,
        descriptions,
        container,
        publisher,
        subjects,
        language,
        ..Data::default()
    })
}

// ── Writer ────────────────────────────────────────────────────────────────────

fn cm_to_ris_type(cm: &str) -> &'static str {
    match cm {
        "Article" => "JOUR",
        "Audiovisual" => "VIDEO",
        "BlogPost" => "BLOG",
        "Book" => "BOOK",
        "BookChapter" => "CHAP",
        "Collection" => "CTLG",
        "Dataset" => "DATA",
        "Dissertation" => "THES",
        "Document" => "GEN",
        "Entry" => "DICT",
        "Event" => "GEN",
        "Figure" => "FIGURE",
        "Image" => "FIGURE",
        "JournalArticle" => "JOUR",
        "LegalDocument" => "GEN",
        "Manuscript" => "GEN",
        "Map" => "MAP",
        "Patent" => "PAT",
        "Performance" => "GEN",
        "PersonalCommunication" => "PCOMM",
        "Post" => "GEN",
        "ProceedingsArticle" => "CPAPER",
        "Proceedings" => "CONF",
        "Report" => "RPRT",
        "Review" => "GEN",
        "Software" => "COMP",
        "Sound" => "SOUND",
        "Standard" => "STAND",
        "WebPage" => "WEB",
        _ => "GEN",
    }
}

// Format a contributor as "Family, Given" or fall back to name (for organizations)
fn contributor_to_ris(c: &Contributor) -> Option<String> {
    if !c.family_name.is_empty() {
        let mut s = c.family_name.clone();
        if !c.given_name.is_empty() {
            s.push_str(", ");
            s.push_str(&c.given_name);
        }
        Some(s)
    } else if !c.name.is_empty() {
        Some(c.name.clone())
    } else {
        None
    }
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
    if let Some(t) = data.titles.first() {
        if !t.title.is_empty() {
            lines.push(format!("T1  - {}", t.title));
        }
    }

    // T2 – container title
    if !data.container.title.is_empty() {
        lines.push(format!("T2  - {}", data.container.title));
    }

    // AU – authors (one line each, Authors only)
    for c in &data.contributors {
        if c.contributor_roles.contains(&"Author".to_string()) {
            if let Some(name) = contributor_to_ris(c) {
                lines.push(format!("AU  - {}", name));
            }
        }
    }

    // DO – DOI (bare, without https://doi.org/ prefix)
    if let Some(doi) = validate_doi(&data.id) {
        lines.push(format!("DO  - {}", doi));
    }

    // UR – URL
    field!("UR", &data.url);

    // AB – abstract (first description)
    if let Some(d) = data.descriptions.first() {
        if !d.description.is_empty() {
            lines.push(format!("AB  - {}", d.description));
        }
    }

    // KW – keywords (one line each)
    for s in &data.subjects {
        if !s.subject.is_empty() {
            lines.push(format!("KW  - {}", s.subject));
        }
    }

    // PY – publication year (first 4 chars)
    if !data.date.published.is_empty() {
        let year: &str = if data.date.published.len() >= 4 {
            &data.date.published[..4]
        } else {
            &data.date.published
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
        assert_eq!(data.titles[0].title, "A Test Article");
        assert_eq!(data.contributors.len(), 2);
        assert_eq!(data.contributors[0].family_name, "Smith");
        assert_eq!(data.contributors[0].given_name, "John");
        assert_eq!(data.contributors[1].family_name, "Doe");
        assert_eq!(data.date.published, "2023");
        assert_eq!(data.descriptions[0].description, "This is the abstract.");
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
        assert_eq!(a.family_name, "Smith");
        assert_eq!(a.given_name, "John");
        assert_eq!(a.type_, "Person");

        let b = parse_author("John Smith");
        assert_eq!(b.family_name, "Smith");
        assert_eq!(b.given_name, "John");

        // Single token without spaces → Organization
        let c = parse_author("NIH");
        assert_eq!(c.type_, "Organization");
        assert_eq!(c.name, "NIH");
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
}
