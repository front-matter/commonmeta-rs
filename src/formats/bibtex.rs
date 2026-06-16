//! BibTeX reader and writer for Commonmeta, backed by the `biblatex` crate.

use biblatex::{
    Bibliography, Chunk, ChunksExt, DateValue, Entry, EntryType, PermissiveType, Person, Spanned,
};

use crate::data::{Container, Contributor, Data, Description, Title};
use crate::doi_utils::normalize_doi;
use crate::error::{Error, Result};
use crate::utils::{get_language, url_to_spdx};

fn cm_to_bib_type(cm: &str) -> EntryType {
    match cm {
        "Article" | "BlogPost" | "JournalArticle" => EntryType::Article,
        "Book" => EntryType::Book,
        "BookChapter" => EntryType::InBook,
        "Dissertation" => EntryType::PhdThesis,
        "Manuscript" => EntryType::Unpublished,
        "Proceedings" => EntryType::Proceedings,
        "ProceedingsArticle" => EntryType::InProceedings,
        "Report" => EntryType::TechReport,
        "Software" => EntryType::Software,
        _ => EntryType::Misc,
    }
}

fn bare_doi(id: &str) -> String {
    id.strip_prefix("https://doi.org/")
        .unwrap_or("")
        .to_string()
}

fn chunks(s: &str) -> Vec<Spanned<Chunk>> {
    vec![Spanned::detached(Chunk::Normal(s.to_string()))]
}

// ─── BibTeX → Commonmeta type mapping ────────────────────────────────────────

fn bib_to_cm_type(entry_type: &EntryType) -> &'static str {
    match entry_type {
        EntryType::Article | EntryType::SuppPeriodical => "JournalArticle",
        EntryType::Book | EntryType::MvBook | EntryType::BookInBook | EntryType::SuppBook => {
            "Book"
        }
        EntryType::InBook | EntryType::InCollection => "BookChapter",
        EntryType::PhdThesis | EntryType::MastersThesis => "Dissertation",
        EntryType::Unpublished => "Manuscript",
        EntryType::Proceedings | EntryType::MvProceedings => "Proceedings",
        EntryType::InProceedings => "ProceedingsArticle",
        EntryType::TechReport | EntryType::Report => "Report",
        EntryType::Software => "Software",
        EntryType::Dataset => "Dataset",
        EntryType::Online | EntryType::Misc => "WebPage",
        EntryType::Periodical => "Journal",
        _ => "Other",
    }
}

/// Infer the container type for a given entry type.
fn container_type_for(entry_type: &EntryType) -> &'static str {
    match entry_type {
        EntryType::Article
        | EntryType::SuppPeriodical
        | EntryType::Periodical => "Journal",
        EntryType::InBook
        | EntryType::InCollection
        | EntryType::InProceedings => "Book",
        _ => "Periodical",
    }
}

// ─── Reader helpers ───────────────────────────────────────────────────────────

fn person_to_contributor(person: Person, role: &str) -> Contributor {
    // Organizations are wrapped in extra braces by the writer, e.g. `{ACME Corp}`.
    let (type_, name, given_name, family_name) = if person.given_name.is_empty()
        && person.name.starts_with('{')
        && person.name.ends_with('}')
    {
        let org = person.name[1..person.name.len() - 1].to_string();
        ("Organization".to_string(), org, String::new(), String::new())
    } else if person.given_name.is_empty() && person.name.contains(' ') {
        // Single string with spaces but no given_name — treat as organization.
        ("Organization".to_string(), person.name, String::new(), String::new())
    } else {
        (
            "Person".to_string(),
            String::new(),
            person.given_name,
            person.name,
        )
    };
    Contributor {
        type_,
        name,
        given_name,
        family_name,
        contributor_roles: vec![role.to_string()],
        ..Default::default()
    }
}

/// Format a `biblatex::Date` as an ISO 8601 partial date string (YYYY, YYYY-MM,
/// or YYYY-MM-DD). Months and days in `biblatex` are 0-indexed.
fn date_to_iso(date: biblatex::Date) -> String {
    let dt = match date.value {
        DateValue::At(dt)
        | DateValue::After(dt)
        | DateValue::Before(dt) => dt,
        DateValue::Between(start, _) => start,
    };
    match (dt.month, dt.day) {
        (None, _) => format!("{:04}", dt.year),
        (Some(m), None) => format!("{:04}-{:02}", dt.year, m + 1),
        (Some(m), Some(d)) => format!("{:04}-{:02}-{:02}", dt.year, m + 1, d + 1),
    }
}

// ─── Reader ───────────────────────────────────────────────────────────────────

/// Parse BibTeX text and return the first entry as a [`Data`] record.
pub fn read(input: &str) -> Result<Data> {
    let bib = Bibliography::parse(input)
        .map_err(|e| Error::Parse(e.to_string()))?;
    let entry = bib
        .iter()
        .next()
        .ok_or_else(|| Error::Parse("no entries found in BibTeX input".to_string()))?;
    from_entry(entry)
}

fn from_entry(entry: &Entry) -> Result<Data> {
    let mut data = Data::default();

    // Type
    data.type_ = bib_to_cm_type(&entry.entry_type).to_string();

    // ID: prefer DOI, then URL, then cite key
    let doi_str = entry.doi().unwrap_or_default();
    if !doi_str.is_empty() {
        data.id = normalize_doi(&doi_str);
    } else {
        let url_str = entry.url().unwrap_or_default();
        data.id = if url_str.is_empty() {
            entry.key.clone()
        } else {
            url_str
        };
    }

    // URL (always populate separately)
    let url_str = entry.url().unwrap_or_default();
    if !url_str.is_empty() {
        data.url = url_str;
    }

    // Titles
    if let Ok(title_chunks) = entry.title() {
        let text = title_chunks.format_verbatim();
        if !text.is_empty() {
            data.titles.push(Title {
                title: text,
                ..Default::default()
            });
        }
    }
    if let Ok(sub_chunks) = entry.subtitle() {
        let text = sub_chunks.format_verbatim();
        if !text.is_empty() {
            data.titles.push(Title {
                title: text,
                ..Default::default()
            });
        }
    }

    // Contributors: authors
    if let Ok(authors) = entry.author() {
        for person in authors {
            data.contributors.push(person_to_contributor(person, "Author"));
        }
    }
    // Contributors: editors
    if let Ok(editor_groups) = entry.editors() {
        for (persons, _) in editor_groups {
            for person in persons {
                data.contributors.push(person_to_contributor(person, "Editor"));
            }
        }
    }

    // Date published (via `date` field or year/month/day fallback)
    if let Ok(PermissiveType::Typed(date)) = entry.date() {
        let iso = date_to_iso(date);
        if !iso.is_empty() {
            data.date.published = iso;
        }
    }

    // Abstract
    if let Ok(abs_chunks) = entry.abstract_() {
        let text = abs_chunks.format_verbatim();
        if !text.is_empty() {
            data.descriptions.push(Description {
                description: text,
                type_: "Abstract".to_string(),
                ..Default::default()
            });
        }
    }

    // Note → Other description
    if let Ok(note_chunks) = entry.note() {
        let text = note_chunks.format_verbatim();
        if !text.is_empty() {
            data.descriptions.push(Description {
                description: text,
                type_: "Other".to_string(),
                ..Default::default()
            });
        }
    }

    // License (copyright field → URL and SPDX id)
    if let Some(chunks) = entry.get("copyright") {
        let url = chunks.format_verbatim();
        if !url.is_empty() {
            let id = url_to_spdx(&url);
            if !id.is_empty() {
                data.license.id = id;
            }
            data.license.url = url;
        }
    }

    // Publisher / institution
    if let Ok(pubs) = entry.publisher() {
        if let Some(pub_chunks) = pubs.into_iter().next() {
            let name = pub_chunks.format_verbatim();
            if !name.is_empty() {
                data.publisher.name = name;
            }
        }
    } else if let Ok(inst_chunks) = entry.institution() {
        let name = inst_chunks.format_verbatim();
        if !name.is_empty() {
            data.publisher.name = name;
        }
    }

    // Language: raw chunks field → convert name/code to ISO 639-1
    if let Some(lang_chunks) = entry.get("language") {
        let raw = lang_chunks.format_verbatim();
        if !raw.is_empty() {
            let iso = get_language(&raw, "");
            data.language = if iso.is_empty() { raw } else { iso };
        }
    }

    // Version
    if let Ok(ver_chunks) = entry.version() {
        let text = ver_chunks.format_verbatim();
        if !text.is_empty() {
            data.version = text;
        }
    }

    // Container
    let container_title = entry
        .journal()
        .ok()
        .map(|c| c.format_verbatim())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            entry
                .book_title()
                .ok()
                .map(|c| c.format_verbatim())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default();

    let (identifier, identifier_type) = if let Some(issn_c) = entry.get("issn") {
        let issn = issn_c.format_verbatim();
        if issn.is_empty() {
            (String::new(), String::new())
        } else {
            (issn, "ISSN".to_string())
        }
    } else if let Some(isbn_c) = entry.get("isbn") {
        let isbn = isbn_c.format_verbatim();
        if isbn.is_empty() {
            (String::new(), String::new())
        } else {
            (isbn, "ISBN".to_string())
        }
    } else {
        (String::new(), String::new())
    };

    let volume = match entry.volume().ok() {
        Some(PermissiveType::Typed(v)) => v.to_string(),
        Some(PermissiveType::Chunks(ref c)) => c.format_verbatim(),
        None => String::new(),
    };

    let issue = entry
        .get("issue")
        .map(|c| c.format_verbatim())
        .unwrap_or_default();

    let (first_page, last_page) = entry
        .get("pages")
        .map(|c| c.format_verbatim())
        .map(|raw| {
            // biblatex converts BibTeX `--` to U+2013 en-dash during parsing.
            if let Some((f, l)) = raw.split_once('\u{2013}') {
                (f.trim().to_string(), l.trim().to_string())
            } else if let Some((f, l)) = raw.split_once("--") {
                (f.trim().to_string(), l.trim().to_string())
            } else {
                (raw, String::new())
            }
        })
        .unwrap_or_default();

    if !container_title.is_empty()
        || !identifier.is_empty()
        || !volume.is_empty()
        || !issue.is_empty()
        || !first_page.is_empty()
    {
        data.container = Container {
            type_: container_type_for(&entry.entry_type).to_string(),
            title: container_title,
            identifier,
            identifier_type,
            volume,
            issue,
            first_page,
            last_page,
            ..Default::default()
        };
    }

    Ok(data)
}

/// Render a [`Data`] record as BibTeX text (UTF-8 bytes).
pub fn write(data: &Data) -> Result<Vec<u8>> {
    let entry_type = cm_to_bib_type(&data.type_);

    // Flags computed before `entry_type` is moved into `Entry::new`.
    let is_article = matches!(entry_type, EntryType::Article);
    let is_phdthesis = matches!(
        entry_type,
        EntryType::PhdThesis | EntryType::MastersThesis
    );
    let is_inbook_or_inproc = matches!(
        entry_type,
        EntryType::InBook | EntryType::InProceedings
    );

    // Citation key: bare DOI if available, else full `id`.
    let doi_bare = bare_doi(&data.id);
    let cite_key = if doi_bare.is_empty() {
        data.id.clone()
    } else {
        doi_bare.clone()
    };

    let mut entry = Entry::new(cite_key, entry_type);

    // Title (join first two title segments with ": ").
    let title: String = if data.titles.len() > 1 {
        format!("{}: {}", data.titles[0].title, data.titles[1].title)
    } else {
        data.titles
            .first()
            .map(|t| t.title.clone())
            .unwrap_or_default()
    };
    if !title.is_empty() {
        entry.set_title(chunks(&title));
    }

    // Authors (contributors with role "Author").
    let authors: Vec<Person> = data
        .contributors
        .iter()
        .filter(|c| c.contributor_roles.iter().any(|r| r == "Author"))
        .filter_map(|c| {
            if !c.family_name.is_empty() {
                Some(Person {
                    name: c.family_name.clone(),
                    given_name: c.given_name.clone(),
                    prefix: String::new(),
                    suffix: String::new(),
                    id: None,
                    prefix_initials: None,
                    given_initials: None,
                    use_prefix: None,
                })
            } else if !c.name.is_empty() {
                // Organization: wrap in extra braces to prevent BibTeX name parsing.
                Some(Person {
                    name: format!("{{{}}}", c.name),
                    given_name: String::new(),
                    prefix: String::new(),
                    suffix: String::new(),
                    id: None,
                    prefix_initials: None,
                    given_initials: None,
                    use_prefix: None,
                })
            } else {
                None
            }
        })
        .collect();
    if !authors.is_empty() {
        entry.set_author(authors);
    }

    // Abstract – first description.
    if let Some(desc) = data.descriptions.first() {
        if !desc.description.is_empty() {
            entry.set_abstract_(chunks(&desc.description));
        }
    }

    // Copyright / license URL.
    if !data.license.url.is_empty() {
        entry.set("copyright", chunks(&data.license.url));
    }

    // DOI.
    if !doi_bare.is_empty() {
        entry.set_doi(doi_bare.clone());
    }

    // Container: ISSN / ISBN.
    let container = &data.container;
    if !container.identifier.is_empty() {
        match container.identifier_type.as_str() {
            "ISSN" => entry.set_issn(chunks(&container.identifier)),
            "ISBN" => entry.set_isbn(chunks(&container.identifier)),
            _ => {}
        }
    }

    // Institution (for theses) or publisher (for other non-article types).
    if is_phdthesis {
        if !data.publisher.name.is_empty() {
            entry.set_institution(chunks(&data.publisher.name));
        }
    } else if !is_article {
        if !data.publisher.name.is_empty() {
            entry.set_publisher(vec![chunks(&data.publisher.name)]);
        }
    }

    // Issue number (BibTeX `issue` field to match hand-rolled output).
    if !container.issue.is_empty() {
        entry.set("issue", chunks(&container.issue));
    }

    // Journal title or booktitle.
    let is_journal_container =
        matches!(container.type_.as_str(), "Journal" | "Periodical");
    if is_inbook_or_inproc && !container.title.is_empty() {
        entry.set_book_title(chunks(&container.title));
    } else if is_journal_container && !container.title.is_empty() {
        entry.set_journal(chunks(&container.title));
    }

    // Language (ISO 639-1 → English name).
    if !data.language.is_empty() {
        let lang_name = crate::utils::get_language(&data.language, "name");
        if !lang_name.is_empty() {
            entry.set("language", chunks(&lang_name));
        }
    }

    // Month from `date.published`.
    const MONTH_ABBREVS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct",
        "nov", "dec",
    ];
    let date_pub = &data.date.published;
    if date_pub.len() >= 7 {
        if let Ok(m) = date_pub[5..7].parse::<usize>() {
            if (1..=12).contains(&m) {
                entry.set("month", chunks(MONTH_ABBREVS[m - 1]));
            }
        }
    }

    // Pages: `first--last` or just `first`.
    let pages = match (container.first_page.as_str(), container.last_page.as_str()) {
        ("", _) | (_, "") => container.first_page.clone(),
        (f, l) => format!("{}--{}", f, l),
    };
    if !pages.is_empty() {
        entry.set("pages", chunks(&pages));
    }

    // URL.
    if !data.url.is_empty() {
        entry.set_url(data.url.clone());
    }

    // Volume.
    if !container.volume.is_empty() {
        entry.set("volume", chunks(&container.volume));
    }

    // Year from `date.published`.
    if date_pub.len() >= 4 {
        entry.set("year", chunks(&date_pub[..4]));
    }

    let mut bibtex_str = entry
        .to_bibtex_string()
        .map_err(|e| Error::Serialize(e.to_string()))?;
    bibtex_str.push('\n');

    Ok(bibtex_str.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cm_to_bib_type() {
        assert!(matches!(cm_to_bib_type("JournalArticle"), EntryType::Article));
        assert!(matches!(cm_to_bib_type("BookChapter"), EntryType::InBook));
        assert!(matches!(cm_to_bib_type("Dissertation"), EntryType::PhdThesis));
        assert!(matches!(
            cm_to_bib_type("ProceedingsArticle"),
            EntryType::InProceedings
        ));
        assert!(matches!(cm_to_bib_type("Unknown"), EntryType::Misc));
    }

    #[test]
    fn test_bare_doi() {
        assert_eq!(bare_doi("https://doi.org/10.1234/foo"), "10.1234/foo");
        assert_eq!(bare_doi("https://example.org"), "");
    }
}
