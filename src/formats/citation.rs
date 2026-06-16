use std::collections::BTreeMap;
use std::str::FromStr;

use unic_langid::LanguageIdentifier;

use hayagriva::citationberg::Style;
use hayagriva::archive::{ArchivedStyle, locales};
use hayagriva::types::{
    Date, EntryType, FormatString, MaybeTyped, Numeric, Person, Publisher, QualifiedUrl,
    SerialNumber,
};
use hayagriva::citationberg::LocaleCode;
use hayagriva::{
    BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, Entry,
};

use crate::data::Data;
use crate::error::{Error, Result};

// ─── Type mapping ─────────────────────────────────────────────────────────────

fn to_entry_type(t: &str) -> EntryType {
    match t {
        "JournalArticle" => EntryType::Article,
        "BookChapter" => EntryType::Chapter,
        "Book" | "EditedBook" => EntryType::Book,
        "ProceedingsArticle" => EntryType::Article,
        "Proceedings" => EntryType::Proceedings,
        "Dissertation" => EntryType::Thesis,
        "Preprint" | "Article" => EntryType::Article,
        "Report" => EntryType::Report,
        "WebPage" => EntryType::Web,
        "Software" => EntryType::Repository,
        "Dataset" => EntryType::Misc,
        "Post" | "BlogPost" => EntryType::Post,
        _ => EntryType::Misc,
    }
}

/// Parent type for a given work type.
fn parent_type(t: &str) -> Option<EntryType> {
    match t {
        "JournalArticle" => Some(EntryType::Periodical),
        "BookChapter" => Some(EntryType::Book),
        "ProceedingsArticle" => Some(EntryType::Proceedings),
        "BlogPost" => Some(EntryType::Blog),
        _ => None,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn to_person(c: &crate::data::Contributor) -> Option<Person> {
    if c.type_ == "Organization" || (!c.name.is_empty() && c.family_name.is_empty()) {
        // Use organisation name as a literal (no given name)
        Some(Person {
            name: c.name.clone(),
            given_name: None,
            prefix: None,
            suffix: None,
            comma_suffix: false,
            alias: None,
        })
    } else if !c.family_name.is_empty() {
        Some(Person {
            name: c.family_name.clone(),
            given_name: if c.given_name.is_empty() {
                None
            } else {
                Some(c.given_name.clone())
            },
            prefix: None,
            suffix: None,
            comma_suffix: false,
            alias: None,
        })
    } else {
        None
    }
}

fn parse_date(s: &str) -> Option<Date> {
    if s.is_empty() {
        return None;
    }
    // Date::from_str handles ISO 8601 and converts month/day to 0-indexed internally.
    Date::from_str(s).ok()
}

fn fmt_string(s: &str) -> Option<FormatString> {
    if s.is_empty() { None } else { FormatString::from_str(s).ok() }
}

// ─── Entry builder ────────────────────────────────────────────────────────────

fn build_entry(data: &Data) -> Entry {
    let entry_type = to_entry_type(&data.type_);
    let key = data
        .id
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .replace('/', "-");
    let mut entry = Entry::new(&key, entry_type);

    // Title
    if let Some(t) = data.titles.first() {
        if let Some(fs) = fmt_string(&t.title) {
            entry.set_title(fs);
        }
    }

    // Authors
    let authors: Vec<Person> = data
        .contributors
        .iter()
        .filter(|c| c.contributor_roles.iter().any(|r| r == "Author"))
        .filter_map(to_person)
        .collect();
    if !authors.is_empty() {
        entry.set_authors(authors);
    }

    // Editors
    let editors: Vec<Person> = data
        .contributors
        .iter()
        .filter(|c| c.contributor_roles.iter().any(|r| r == "Editor"))
        .filter_map(to_person)
        .collect();
    if !editors.is_empty() {
        entry.set_editors(editors);
    }

    // Date (prefer published, fall back to created)
    let date = parse_date(&data.date.published)
        .or_else(|| parse_date(&data.date.created));
    if let Some(d) = date {
        entry.set_date(d);
    }

    // URL
    if !data.url.is_empty() {
        if let Ok(qurl) = QualifiedUrl::from_str(&data.url) {
            entry.set_url(qurl);
        }
    }

    // DOI + ISSN/ISBN via serial-number
    let doi = data
        .id
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/");
    if !doi.is_empty() && doi != data.id {
        let mut sn = BTreeMap::new();
        sn.insert("doi".to_string(), doi.to_string());
        // ISSN from container
        if data.container.identifier_type == "ISSN" && !data.container.identifier.is_empty() {
            sn.insert("issn".to_string(), data.container.identifier.clone());
        }
        entry.set_serial_number(SerialNumber(sn));
    }

    // Publisher
    if !data.publisher.name.is_empty() {
        if let Ok(p) = Publisher::from_str(&data.publisher.name) {
            entry.set_publisher(p);
        }
    }

    // Volume, issue, page-range from container
    let container = &data.container;
    if let Ok(n) = Numeric::from_str(&container.volume) {
        entry.set_volume(MaybeTyped::Typed(n));
    }
    if let Ok(n) = Numeric::from_str(&container.issue) {
        entry.set_issue(MaybeTyped::Typed(n));
    }
    let page_str = match (container.first_page.as_str(), container.last_page.as_str()) {
        ("", _) => String::new(),
        (f, "") => f.to_string(),
        (f, l) => format!("{f}-{l}"),
    };
    if !page_str.is_empty() {
        if let Ok(pr) = hayagriva::types::PageRanges::from_str(&page_str) {
            entry.set_page_range(MaybeTyped::Typed(pr));
        }
    }

    // Language
    if !data.language.is_empty() {
        if let Ok(lang) = data.language.parse::<LanguageIdentifier>() {
            entry.set_language(lang);
        }
    }

    // Parent entry (journal / book / proceedings)
    if let Some(ptype) = parent_type(&data.type_) {
        if !container.title.is_empty() {
            let mut parent = Entry::new(&format!("{key}-parent"), ptype);
            if let Some(fs) = fmt_string(&container.title) {
                parent.set_title(fs);
            }
            entry.set_parents(vec![parent]);
        }
    }

    entry
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Format a bibliography entry as HTML. Defaults to APA 7th edition.
/// `style_name` is a CSL style name (e.g. `"chicago-author-date"`).
/// `locale` is a BCP 47 tag (e.g. `"de-DE"`) that overrides the style's default language.
pub fn write(data: &Data, style_name: Option<&str>, locale: Option<&str>) -> Result<Vec<u8>> {
    let archived = style_name
        .and_then(ArchivedStyle::by_name)
        .unwrap_or(ArchivedStyle::AmericanPsychologicalAssociation);

    let style = match archived.get() {
        Style::Independent(s) => s,
        Style::Dependent(_) => {
            return Err(Error::Serialize("dependent style not supported".into()));
        }
    };

    let locale_code = locale.map(|l| LocaleCode(l.into()));
    let locale_list = locales();

    let entry = build_entry(data);
    let mut driver = BibliographyDriver::new();
    let items = vec![CitationItem::with_entry(&entry)];
    driver.citation(CitationRequest::new(
        items,
        &style,
        locale_code.clone(),
        &locale_list,
        None,
    ));

    let result = driver.finish(BibliographyRequest {
        style: &style,
        locale: locale_code,
        locale_files: &locale_list,
    });

    let text = result
        .bibliography
        .and_then(|bib| bib.items.into_iter().next())
        .map(|item| {
            let mut buf = String::new();
            item.content.write_buf(&mut buf, BufWriteFormat::Html).unwrap_or(());
            buf
        })
        .unwrap_or_default();

    Ok(text.into_bytes())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Data {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/commonmeta")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn apa_journal_article() {
        let data = load("journal_article.json");
        let out = write(&data, None, None).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Lovelace"), "author: {text}");
        assert!(text.contains("2024"), "year: {text}");
        assert!(text.contains("Study of Things"), "title: {text}");
        assert!(text.contains("Journal of Examples"), "journal: {text}");
    }

    #[test]
    fn chicago_style() {
        let data = load("journal_article.json");
        let out = write(&data, Some("chicago-author-date"), None).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(!text.is_empty(), "expected non-empty chicago citation");
    }
}
