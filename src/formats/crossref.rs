use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use url::Url;

use crate::data::{
    Affiliation, Container, Contributor, Data, Date, Description, FundingReference, License,
    Publisher, Reference, Subject, Title,
};
use crate::error::{Error, Result};

// ─── Crossref API structs ────────────────────────────────────────────────────

#[derive(Deserialize)]
struct CrossrefResponse {
    message: CrossrefWork,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct CrossrefListResponse {
    message: CrossrefListMessage,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct CrossrefListMessage {
    #[serde(default)]
    items: Vec<CrossrefWork>,
}

#[derive(Deserialize)]
pub(crate) struct CrossrefWork {
    #[serde(rename = "DOI")]
    doi: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    title: Vec<String>,
    #[serde(default)]
    author: Vec<CrossrefAuthor>,
    #[serde(default)]
    publisher: String,
    #[serde(rename = "container-title", default)]
    container_title: Vec<String>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(default)]
    issue: Option<String>,
    #[serde(default)]
    page: Option<String>,
    #[serde(rename = "ISSN", default)]
    issn: Vec<String>,
    #[serde(rename = "URL", default)]
    url: String,
    #[serde(rename = "abstract", default)]
    abstract_text: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    subject: Vec<String>,
    #[serde(default)]
    license: Vec<CrossrefLicense>,
    #[serde(default)]
    funder: Vec<CrossrefFunder>,
    #[serde(default)]
    reference: Vec<CrossrefReference>,
    #[serde(default)]
    published: Option<CrossrefDate>,
    #[serde(rename = "published-print", default)]
    published_print: Option<CrossrefDate>,
    #[serde(rename = "published-online", default)]
    published_online: Option<CrossrefDate>,
    #[serde(default)]
    created: Option<CrossrefDate>,
}

#[derive(Deserialize)]
struct CrossrefAuthor {
    #[serde(rename = "ORCID")]
    orcid: Option<String>,
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
    #[serde(default)]
    affiliation: Vec<CrossrefAffiliation>,
}

#[derive(Deserialize)]
struct CrossrefAffiliation {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: Vec<CrossrefAffiliationId>,
}

#[derive(Deserialize)]
struct CrossrefAffiliationId {
    id: String,
    #[serde(rename = "id-type")]
    id_type: String,
    #[serde(rename = "asserted-by", default)]
    asserted_by: String,
}

#[derive(Deserialize)]
struct CrossrefDate {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<i32>>>,
}

#[derive(Deserialize)]
struct CrossrefLicense {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "content-version")]
    content_version: Option<String>,
}

#[derive(Deserialize)]
struct CrossrefFunder {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    award: Vec<String>,
}

#[derive(Deserialize)]
struct CrossrefReference {
    key: Option<String>,
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "article-title")]
    article_title: Option<String>,
    year: Option<String>,
    #[serde(rename = "first-page")]
    first_page: Option<String>,
    #[serde(rename = "last-page")]
    last_page: Option<String>,
    unstructured: Option<String>,
    #[serde(rename = "doi-asserted-by")]
    doi_asserted_by: Option<String>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn format_date(d: &CrossrefDate) -> String {
    let parts = match d.date_parts.first() {
        Some(p) => p,
        None => return String::new(),
    };
    let year = match parts.first().and_then(|v| *v) {
        Some(y) => y,
        None => return String::new(),
    };
    match (parts.get(1).and_then(|v| *v), parts.get(2).and_then(|v| *v)) {
        (Some(m), Some(d)) => format!("{:04}-{:02}-{:02}", year, m, d),
        (Some(m), None) => format!("{:04}-{:02}", year, m),
        _ => format!("{:04}", year),
    }
}

fn crossref_type(t: &str) -> &str {
    match t {
        "journal-article" => "JournalArticle",
        "book-chapter" => "BookChapter",
        "book" | "monograph" | "edited-book" | "reference-book" => "Book",
        "proceedings-article" => "ProceedingsArticle",
        "proceedings" | "conference" => "Proceedings",
        "dataset" => "Dataset",
        "dissertation" => "Dissertation",
        "posted-content" => "Preprint",
        "report" | "report-series" => "Report",
        "peer-review" => "PeerReview",
        "reference-entry" => "Entry",
        "journal" => "Journal",
        "journal-volume" => "JournalVolume",
        "journal-issue" => "JournalIssue",
        "component" => "Component",
        "grant" => "Grant",
        "standard" => "Standard",
        _ => "Other",
    }
}

fn container_type(work_type: &str) -> &str {
    match work_type {
        "journal-article" => "Journal",
        "book-chapter" => "Book",
        "proceedings-article" => "Proceedings",
        _ => "",
    }
}

fn normalize_doi_url(doi: &str) -> String {
    if doi.starts_with("https://doi.org/") {
        doi.to_string()
    } else {
        let bare = doi
            .trim_start_matches("https://dx.doi.org/")
            .trim_start_matches("http://dx.doi.org/")
            .trim_start_matches("http://doi.org/")
            .trim_start_matches("https://doi.org/");
        format!("https://doi.org/{}", bare)
    }
}

fn normalize_orcid(orcid: &str) -> String {
    let bare = orcid
        .trim_start_matches("https://orcid.org/")
        .trim_start_matches("http://orcid.org/");
    format!("https://orcid.org/{}", bare)
}

lazy_static! {
    static ref JATS_TAG: Regex = Regex::new(r"<[^>]+>").unwrap();
}

fn strip_jats(s: &str) -> String {
    JATS_TAG.replace_all(s, "").to_string()
}

fn license_spdx(url: &str) -> &str {
    if url.contains("creativecommons.org/licenses/by/4.0") {
        "CC-BY-4.0"
    } else if url.contains("creativecommons.org/licenses/by-sa/4.0") {
        "CC-BY-SA-4.0"
    } else if url.contains("creativecommons.org/licenses/by-nd/4.0") {
        "CC-BY-ND-4.0"
    } else if url.contains("creativecommons.org/licenses/by-nc/4.0") {
        "CC-BY-NC-4.0"
    } else if url.contains("creativecommons.org/licenses/by-nc-sa/4.0") {
        "CC-BY-NC-SA-4.0"
    } else if url.contains("creativecommons.org/licenses/by-nc-nd/4.0") {
        "CC-BY-NC-ND-4.0"
    } else if url.contains("creativecommons.org/publicdomain/zero/1.0") {
        "CC0-1.0"
    } else {
        ""
    }
}

fn split_page(page: &str) -> (String, String) {
    match page.find('-') {
        Some(i) => (page[..i].to_string(), page[i + 1..].to_string()),
        None => (page.to_string(), String::new()),
    }
}

// ─── Conversion ──────────────────────────────────────────────────────────────

fn from_work(w: CrossrefWork) -> Data {
    let id = normalize_doi_url(&w.doi);
    let type_ = crossref_type(&w.type_).to_string();
    let url = if w.url.is_empty() { id.clone() } else { normalize_doi_url(&w.url) };

    let titles: Vec<Title> = w
        .title
        .into_iter()
        .map(|t| Title { title: t, ..Default::default() })
        .collect();

    let contributors: Vec<Contributor> = w
        .author
        .into_iter()
        .map(|a| {
            let is_person = a.given.is_some() || a.family.is_some();
            let (type_str, given, family, name) = if is_person {
                (
                    "Person".to_string(),
                    a.given.unwrap_or_default(),
                    a.family.unwrap_or_default(),
                    String::new(),
                )
            } else {
                (
                    "Organization".to_string(),
                    String::new(),
                    String::new(),
                    a.name.unwrap_or_default(),
                )
            };
            let orcid = a.orcid.as_deref().map(normalize_orcid).unwrap_or_default();
            let affiliations = a
                .affiliation
                .into_iter()
                .map(|af| {
                    let ror = af.id.into_iter().find(|i| i.id_type == "ROR");
                    Affiliation {
                        id: ror.as_ref().map(|r| r.id.clone()).unwrap_or_default(),
                        name: af.name,
                        asserted_by: ror.map(|r| r.asserted_by).unwrap_or_default(),
                    }
                })
                .collect();
            Contributor {
                id: orcid,
                type_: type_str,
                name,
                given_name: given,
                family_name: family,
                affiliations,
                contributor_roles: vec!["Author".to_string()],
            }
        })
        .collect();

    let pub_date = w
        .published
        .as_ref()
        .or(w.published_print.as_ref())
        .or(w.published_online.as_ref())
        .map(format_date)
        .unwrap_or_default();
    let created_date = w.created.as_ref().map(format_date).unwrap_or_default();
    let date = Date { created: created_date, published: pub_date, ..Default::default() };

    let container_name = w.container_title.into_iter().next().unwrap_or_default();
    let issn = w.issn.into_iter().next().unwrap_or_default();
    let has_issn = !issn.is_empty();
    let (first_page, last_page) =
        w.page.as_deref().map(split_page).unwrap_or((String::new(), String::new()));
    let container = Container {
        type_: container_type(&w.type_).to_string(),
        title: container_name,
        identifier: issn,
        identifier_type: if has_issn { "ISSN".to_string() } else { String::new() },
        volume: w.volume.unwrap_or_default(),
        issue: w.issue.unwrap_or_default(),
        first_page,
        last_page,
        ..Default::default()
    };

    let publisher = Publisher { name: w.publisher, ..Default::default() };

    let descriptions: Vec<Description> = w
        .abstract_text
        .into_iter()
        .filter_map(|a| {
            let text = strip_jats(&a);
            if text.is_empty() {
                None
            } else {
                Some(Description { description: text, type_: "Abstract".to_string(), ..Default::default() })
            }
        })
        .collect();

    let license = {
        let chosen = w
            .license
            .iter()
            .find(|l| l.content_version.as_deref() == Some("vor"))
            .or_else(|| w.license.first());
        chosen
            .map(|l| License { id: license_spdx(&l.url).to_string(), url: l.url.clone() })
            .unwrap_or_default()
    };

    let subjects: Vec<Subject> =
        w.subject.into_iter().map(|s| Subject { subject: s }).collect();

    let funding_references: Vec<FundingReference> = w
        .funder
        .into_iter()
        .flat_map(|f| {
            let funder_id = f.doi.as_deref().map(normalize_doi_url).unwrap_or_default();
            let has_doi = !funder_id.is_empty();
            let id_type =
                if has_doi { "Crossref Funder ID".to_string() } else { String::new() };
            if f.award.is_empty() {
                vec![FundingReference {
                    funder_identifier: funder_id,
                    funder_identifier_type: id_type,
                    funder_name: f.name,
                    ..Default::default()
                }]
            } else {
                f.award
                    .into_iter()
                    .map(|award| FundingReference {
                        funder_identifier: funder_id.clone(),
                        funder_identifier_type: id_type.clone(),
                        funder_name: f.name.clone(),
                        award_number: award,
                        ..Default::default()
                    })
                    .collect::<Vec<_>>()
            }
        })
        .collect();

    let references: Vec<Reference> = w
        .reference
        .into_iter()
        .map(|r| Reference {
            key: r.key.unwrap_or_default(),
            id: r.doi.as_deref().map(normalize_doi_url).unwrap_or_default(),
            title: r.article_title.unwrap_or_default(),
            publication_year: r.year.unwrap_or_default(),
            first_page: r.first_page.unwrap_or_default(),
            last_page: r.last_page.unwrap_or_default(),
            unstructured: r.unstructured.unwrap_or_default(),
            asserted_by: r.doi_asserted_by.unwrap_or_default(),
            ..Default::default()
        })
        .collect();

    Data {
        id,
        type_,
        url,
        language: w.language.unwrap_or_default(),
        provider: "Crossref".to_string(),
        titles,
        contributors,
        date,
        container,
        publisher,
        descriptions,
        license,
        subjects,
        funding_references,
        references,
        ..Default::default()
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Parse a Crossref API JSON response (the full `{"status":"ok","message":{...}}` envelope).
pub fn read_json(json: &str) -> Result<Data> {
    let r: CrossrefResponse =
        serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_work(r.message))
}

/// Fetch a work from the Crossref REST API by DOI and convert it to `Data`.
pub fn fetch(doi: &str) -> Result<Data> {
    let bare = doi
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/");
    let url = format!("https://api.crossref.org/works/{bare}");
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;
    let json = client
        .get(&url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;
    read_json(&json)
}

/// Fetch a list of works from the Crossref API and convert them to `Data`.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub fn fetch_all(
    number: usize,
    page: usize,
    member: &str,
    type_: &str,
    sample: bool,
    year: &str,
    ror: &str,
    orcid: &str,
    has_orcid: bool,
    has_ror: bool,
    has_references: bool,
    has_relation: bool,
    has_abstract: bool,
    has_award: bool,
    has_license: bool,
    has_archive: bool,
) -> Result<Vec<Data>> {
    let works = get_all(
        number,
        page,
        member,
        type_,
        sample,
        year,
        ror,
        orcid,
        has_orcid,
        has_ror,
        has_references,
        has_relation,
        has_abstract,
        has_award,
        has_license,
        has_archive,
    )?;
    read_all(works)
}

/// Get a list of raw Crossref work items from the Crossref API.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn get_all(
    number: usize,
    page: usize,
    member: &str,
    type_: &str,
    sample: bool,
    year: &str,
    ror: &str,
    orcid: &str,
    has_orcid: bool,
    has_ror: bool,
    has_references: bool,
    has_relation: bool,
    has_abstract: bool,
    has_award: bool,
    has_license: bool,
    has_archive: bool,
) -> Result<Vec<CrossrefWork>> {
    let url = query_url(
        number,
        page,
        member,
        type_,
        sample,
        year,
        orcid,
        ror,
        has_orcid,
        has_ror,
        has_references,
        has_relation,
        has_abstract,
        has_award,
        has_license,
        has_archive,
    );

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let json = client
        .get(&url)
        .header("Cache-Control", "private")
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;

    let response: CrossrefListResponse =
        serde_json::from_str(&json).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(response.message.items)
}

/// Convert a list of Crossref works into commonmeta `Data`.
#[allow(dead_code)]
pub(crate) fn read_all(works: Vec<CrossrefWork>) -> Result<Vec<Data>> {
    Ok(works.into_iter().map(from_work).collect())
}

/// Build the Crossref `/works` query URL used by `get_all`.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn query_url(
    number: usize,
    page: usize,
    member: &str,
    type_: &str,
    sample: bool,
    year: &str,
    orcid: &str,
    ror: &str,
    has_orcid: bool,
    has_ror: bool,
    has_references: bool,
    has_relation: bool,
    has_abstract: bool,
    has_award: bool,
    has_license: bool,
    has_archive: bool,
) -> String {
    let supported_types = [
        "book",
        "book-chapter",
        "book-part",
        "book-section",
        "book-series",
        "book-set",
        "book-track",
        "component",
        "database",
        "dataset",
        "dissertation",
        "edited-book",
        "grant",
        "journal",
        "journal-article",
        "journal-issue",
        "journal-volume",
        "monograph",
        "other",
        "peer-review",
        "posted-content",
        "proceedings",
        "proceedings-article",
        "proceedings-series",
        "reference-book",
        "reference-entry",
        "report",
        "report-component",
        "report-series",
        "standard",
    ];

    let mut url = Url::parse("https://api.crossref.org/works")
        .expect("hardcoded Crossref URL should parse");
    let rows = number.clamp(1, 1000);
    let page = page.max(1);

    {
        let mut query = url.query_pairs_mut();

        if sample {
            query.append_pair("sample", &rows.to_string());
        } else {
            query.append_pair("rows", &rows.to_string());
            query.append_pair("offset", &((page - 1) * rows).to_string());
        }

        query.append_pair("sort", "published");
        query.append_pair("order", "desc");

        let mut filters: Vec<String> = Vec::new();
        if !member.is_empty() {
            filters.push(format!("member:{member}"));
        }
        if !type_.is_empty() && supported_types.contains(&type_) {
            filters.push(format!("type:{type_}"));
        }
        if !ror.is_empty() {
            let normalized = ror
                .trim_start_matches("https://ror.org/")
                .trim_start_matches("http://ror.org/");
            if !normalized.is_empty() {
                filters.push(format!("ror-id:{normalized}"));
            }
        }
        if !orcid.is_empty() {
            let normalized = orcid
                .trim_start_matches("https://orcid.org/")
                .trim_start_matches("http://orcid.org/");
            if !normalized.is_empty() {
                filters.push(format!("orcid:{normalized}"));
            }
        }
        if !year.is_empty() {
            filters.push(format!("from-pub-date:{year}-01-01"));
            filters.push(format!("until-pub-date:{year}-12-31"));
        }
        if has_orcid {
            filters.push("has-orcid:true".to_string());
        }
        if has_ror {
            filters.push("has-ror-id:true".to_string());
        }
        if has_references {
            filters.push("has-references:true".to_string());
        }
        if has_relation {
            filters.push("has-relation:true".to_string());
        }
        if has_abstract {
            filters.push("has-abstract:true".to_string());
        }
        if has_award {
            filters.push("has-award:true".to_string());
        }
        if has_license {
            filters.push("has-license:true".to_string());
        }
        if has_archive {
            filters.push("has-archive:true".to_string());
        }
        if !filters.is_empty() {
            query.append_pair("filter", &filters.join(","));
        }
    }

    url.to_string()
}
