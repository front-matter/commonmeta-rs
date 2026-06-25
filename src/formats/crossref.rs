use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

use crate::author_utils::{
    cleanup_author, infer_contributor_type, normalize_contributor_roles, split_person_name,
};
use crate::data::{
    Affiliation, Container, Contributor, Data, File, FundingReference, Identifier, Organization,
    Person, Publisher, Reference, Subject, Title,
};
use crate::constants as C;
use crate::error::{Error, Result};
use crate::utils::normalize_id;

// Crossref sometimes sends an explicit JSON `null` for an optional string
// (e.g. `"publisher":null`) rather than omitting the key. `#[serde(default)]`
// alone doesn't catch that: default only fires when the key is *absent*, not
// when it's present with value `null`, so deserializing straight into a
// `String` fails with "invalid type: null, expected a string".
fn null_to_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Same as `null_to_string`, but for array fields (e.g. `"title":[null]`)
/// where an individual element may be an explicit `null`.
fn null_to_string_vec<'de, D: Deserializer<'de>>(
    d: D,
) -> std::result::Result<Vec<String>, D::Error> {
    let values: Vec<Option<String>> = Deserialize::deserialize(d)?;
    Ok(values.into_iter().map(Option::unwrap_or_default).collect())
}

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
    #[serde(rename = "DOI", default, deserialize_with = "null_to_string")]
    doi: String,
    #[serde(rename = "type", default, deserialize_with = "null_to_string")]
    type_: String,
    #[serde(default, deserialize_with = "null_to_string_vec")]
    title: Vec<String>,
    #[serde(default, deserialize_with = "null_to_string_vec")]
    subtitle: Vec<String>,
    #[serde(default)]
    author: Vec<CrossrefAuthor>,
    #[serde(default, deserialize_with = "null_to_string")]
    publisher: String,
    #[serde(
        rename = "container-title",
        default,
        deserialize_with = "null_to_string_vec"
    )]
    container_title: Vec<String>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(default)]
    issue: Option<String>,
    #[serde(default)]
    page: Option<String>,
    #[serde(rename = "ISSN", default, deserialize_with = "null_to_string_vec")]
    issn: Vec<String>,
    #[serde(rename = "abstract", default)]
    abstract_text: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default, deserialize_with = "null_to_string_vec")]
    subject: Vec<String>,
    #[serde(default)]
    license: Vec<CrossrefLicense>,
    #[serde(default)]
    funder: Vec<CrossrefFunder>,
    #[serde(default)]
    reference: Vec<CrossrefReference>,
    #[serde(default)]
    issued: Option<CrossrefDate>,
    #[serde(default)]
    created: Option<CrossrefDate>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(rename = "group-title", default)]
    group_title: Option<String>,
    #[serde(default)]
    institution: Vec<CrossrefInstitution>,
    #[serde(default)]
    resource: Option<CrossrefResource>,
    #[serde(default)]
    version: Option<CrossrefVersion>,
    #[serde(default)]
    link: Vec<CrossrefLink>,
}

#[derive(Deserialize)]
struct CrossrefInstitution {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
}

#[derive(Deserialize)]
struct CrossrefResource {
    #[serde(default)]
    primary: Option<CrossrefPrimaryResource>,
}

#[derive(Deserialize)]
struct CrossrefPrimaryResource {
    #[serde(rename = "URL", default, deserialize_with = "null_to_string")]
    url: String,
}

#[derive(Deserialize)]
struct CrossrefVersion {
    #[serde(default, deserialize_with = "null_to_string")]
    version: String,
}

#[derive(Deserialize)]
struct CrossrefLink {
    #[serde(rename = "URL", default, deserialize_with = "null_to_string")]
    url: String,
    #[serde(rename = "content-type", default, deserialize_with = "null_to_string")]
    content_type: String,
}

#[derive(Deserialize)]
struct CrossrefAuthor {
    #[serde(rename = "ORCID")]
    orcid: Option<String>,
    #[serde(rename = "authenticated-orcid", default)]
    authenticated_orcid: Option<bool>,
    given: Option<String>,
    family: Option<String>,
    name: Option<String>,
    #[serde(default)]
    affiliation: Vec<CrossrefAffiliation>,
}

#[derive(Deserialize)]
struct CrossrefAffiliation {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default)]
    id: Vec<CrossrefAffiliationId>,
}

#[derive(Deserialize)]
struct CrossrefAffiliationId {
    #[serde(default, deserialize_with = "null_to_string")]
    id: String,
    #[serde(rename = "id-type", default, deserialize_with = "null_to_string")]
    id_type: String,
    #[serde(rename = "asserted-by", default, deserialize_with = "null_to_string")]
    asserted_by: String,
}

#[derive(Deserialize)]
struct CrossrefDate {
    #[serde(rename = "date-parts", default)]
    date_parts: Vec<Vec<Option<i32>>>,
    #[serde(rename = "date-time", default)]
    date_time: Option<String>,
}

#[derive(Deserialize)]
struct CrossrefLicense {
    #[serde(rename = "URL", default, deserialize_with = "null_to_string")]
    url: String,
    #[serde(rename = "content-version")]
    content_version: Option<String>,
}

#[derive(Deserialize)]
struct CrossrefFunder {
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_string_vec")]
    award: Vec<String>,
}

#[derive(Deserialize)]
struct CrossrefReference {
    key: Option<String>,
    #[serde(rename = "DOI")]
    doi: Option<String>,
    #[serde(rename = "article-title")]
    article_title: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    year: Option<String>,
    #[serde(default)]
    volume: Option<String>,
    #[serde(default)]
    issue: Option<String>,
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

/// `published` derivation: `issued.date-time` (raw timestamp string), else `issued`
/// reconstructed from `date-parts`, else `created.date-time`. Note `date.created`
/// is never set for Crossref records.
fn published_date(issued: &Option<CrossrefDate>, created: &Option<CrossrefDate>) -> String {
    if let Some(issued) = issued {
        if let Some(dt) = &issued.date_time
            && !dt.is_empty()
        {
            return dt.clone();
        }
        let formatted = format_date(issued);
        if !formatted.is_empty() {
            return formatted;
        }
    }
    created
        .as_ref()
        .and_then(|c| c.date_time.clone())
        .unwrap_or_default()
}

fn capitalize_provider(s: &str) -> String {
    match s {
        "crossref" => "Crossref".to_string(),
        "publisher" => "Publisher".to_string(),
        "author" => "Author".to_string(),
        other => other.to_string(),
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


lazy_static! {
    static ref JATS_TAG: Regex = Regex::new(r"<[^>]+>").unwrap();
}

fn strip_jats(s: &str) -> String {
    JATS_TAG.replace_all(s, "").to_string()
}

fn split_page(page: &str) -> (String, String) {
    match page.find('-') {
        Some(i) => (page[..i].to_string(), page[i + 1..].to_string()),
        None => (page.to_string(), String::new()),
    }
}

// ─── Conversion ──────────────────────────────────────────────────────────────

fn from_work(w: CrossrefWork) -> Data {
    let has_doi = !w.doi.is_empty();
    let id = if has_doi {
        normalize_doi_url(&w.doi)
    } else {
        w.resource
            .as_ref()
            .and_then(|r| r.primary.as_ref())
            .map(|p| p.url.clone())
            .unwrap_or_default()
    };

    // posted-content maps to "Article" by default,
    // but is re-classified as "BlogPost" when published by Front Matter
    // (i.e. Rogue Scholar-deposited blog posts).
    let mut type_ = C::cr_to_cm(&w.type_).to_string();
    if type_ == "Article" && w.publisher == "Front Matter" {
        type_ = "BlogPost".to_string();
    }

    // url comes from resource.primary.URL, not from the top-level URL
    // (which is just the DOI resolver link) or the DOI itself.
    let url = w
        .resource
        .as_ref()
        .and_then(|r| r.primary.as_ref())
        .map(|p| p.url.as_str())
        .and_then(|u| crate::utils::normalize_url(u, false, false))
        .unwrap_or_default();

    let mut title_strings = w.title;
    let title = if title_strings.is_empty() {
        String::new()
    } else {
        title_strings.remove(0)
    };
    let additional_titles: Vec<Title> = title_strings
        .into_iter()
        .map(|t| Title { title: t, ..Default::default() })
        .chain(w.subtitle.into_iter().map(|t| Title {
            title: t,
            type_: "Subtitle".to_string(),
            ..Default::default()
        }))
        .collect();

    let contributors: Vec<Contributor> = w
        .author
        .into_iter()
        .map(|a| {
            let mut given = a.given.unwrap_or_default();
            let mut family = a.family.unwrap_or_default();
            let cleaned_name = cleanup_author(a.name.as_deref()).unwrap_or_default();

            if given.is_empty() && family.is_empty() && !cleaned_name.is_empty() {
                let (g, f, _) = split_person_name(&cleaned_name);
                given = g;
                family = f;
            }

            let orcid = a.orcid.as_deref().map(normalize_id).unwrap_or_default();
            let mut type_str = infer_contributor_type(
                "",
                &orcid,
                &given,
                &family,
                &cleaned_name,
                Some("crossref"),
            );
            if type_str.is_empty() {
                type_str = "Organization".to_string();
            }

            let affiliations: Vec<Affiliation> = a
                .affiliation
                .into_iter()
                .map(|af| {
                    let ror = af.id.into_iter().find(|i| i.id_type == "ROR");
                    Affiliation {
                        id: ror.as_ref().map(|r| r.id.clone()).unwrap_or_default(),
                        name: af.name,
                        asserted_by: ror
                            .map(|r| capitalize_provider(&r.asserted_by))
                            .unwrap_or_default(),
                    }
                })
                .collect();
            let roles = normalize_contributor_roles(&["Author".to_string()], "Author");

            // authenticated-orcid=true → the author authenticated via OAuth;
            // false/absent → the publisher supplied the ORCID without verification.
            let orcid_asserted_by = if !orcid.is_empty() {
                match a.authenticated_orcid {
                    Some(true) => "Author",
                    _ => "Publisher",
                }
            } else {
                ""
            };

            if type_str == "Person" {
                Contributor::person(
                    Person {
                        id: orcid,
                        given_name: given,
                        family_name: family,
                        affiliations,
                        asserted_by: orcid_asserted_by.to_string(),
                    },
                    roles,
                )
            } else {
                Contributor::organization(
                    Organization { id: orcid, name: cleaned_name, asserted_by: String::new() },
                    roles,
                )
            }
        })
        .collect();

    let date_published = published_date(&w.issued, &w.created);

    // Container title fallback chain:
    // container-title[0] || group-title || institution[0].name
    let container_name = w
        .container_title
        .into_iter()
        .next()
        .filter(|t| !t.is_empty())
        .or(w.group_title.filter(|t| !t.is_empty()))
        .or_else(|| {
            w.institution
                .into_iter()
                .next()
                .map(|i| i.name)
                .filter(|n| !n.is_empty())
        })
        .unwrap_or_default();
    let issn = w.issn.into_iter().next().unwrap_or_default();
    let has_issn = !issn.is_empty();
    let (first_page, last_page) = w
        .page
        .as_deref()
        .map(split_page)
        .unwrap_or((String::new(), String::new()));
    let container = Container {
        type_: C::cr_work_to_container(&w.type_).to_string(),
        title: container_name,
        identifier: issn,
        identifier_type: if has_issn {
            "ISSN".to_string()
        } else {
            String::new()
        },
        volume: w.volume.unwrap_or_default(),
        issue: w.issue.unwrap_or_default(),
        first_page,
        last_page,
        ..Default::default()
    };

    let publisher = Publisher {
        name: w.publisher,
        ..Default::default()
    };

    let description = w
        .abstract_text
        .map(|a| strip_jats(&a))
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    let license = {
        let chosen = w
            .license
            .iter()
            .find(|l| l.content_version.as_deref() == Some("vor"))
            .or_else(|| w.license.first());
        chosen
            .map(|l| crate::spdx::from_url(&l.url))
            .unwrap_or_default()
    };

    let subjects: Vec<Subject> = w
        .subject
        .into_iter()
        .map(|s| Subject { subject: s, ..Default::default() })
        .collect();

    let funding_references: Vec<FundingReference> = w
        .funder
        .into_iter()
        .flat_map(|f| {
            let funder_id = f.doi.as_deref().map(normalize_doi_url).unwrap_or_default();
            if f.award.is_empty() {
                vec![FundingReference {
                    funder_id,
                    funder_name: f.name,
                    ..Default::default()
                }]
            } else {
                f.award
                    .into_iter()
                    .map(|award| FundingReference {
                        funder_id: funder_id.clone(),
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
        .map(|r| {
            // The formatted reference string prefers the free-text
            // "unstructured" citation over the structured article title,
            // since it's typically the fuller (and sometimes only) citation
            // text Crossref provides.
            let reference_text = r
                .unstructured
                .clone()
                .or_else(|| r.article_title.clone())
                .unwrap_or_default();
            Reference {
                key: r.key.unwrap_or_default(),
                id: r.doi.as_deref().map(normalize_doi_url).unwrap_or_default(),
                reference: reference_text,
                title: r.article_title.unwrap_or_default(),
                publisher: r.publisher.unwrap_or_default(),
                publication_year: r.year.unwrap_or_default(),
                volume: r.volume.unwrap_or_default(),
                issue: r.issue.unwrap_or_default(),
                first_page: r.first_page.unwrap_or_default(),
                last_page: r.last_page.unwrap_or_default(),
                unstructured: r.unstructured.unwrap_or_default(),
                asserted_by: capitalize_provider(&r.doi_asserted_by.unwrap_or_default()),
                ..Default::default()
            }
        })
        .collect();

    // Include explicit DOI identifier only when Crossref provides a DOI.
    let identifiers = if has_doi {
        vec![Identifier {
            identifier: id.clone(),
            identifier_type: "DOI".to_string(),
        }]
    } else {
        Vec::new()
    };

    let files: Vec<File> = w
        .link
        .into_iter()
        .filter(|l| l.content_type != "unspecified" && !l.url.is_empty())
        .map(|l| File {
            url: l.url,
            mime_type: l.content_type,
            ..Default::default()
        })
        .collect();

    let version = w.version.map(|v| v.version).unwrap_or_default();
    let additional_type = w.subtype.unwrap_or_default();

    Data {
        id,
        type_,
        url,
        language: w.language.unwrap_or_default(),
        provider: "Crossref".to_string(),
        title,
        additional_titles,
        contributors,
        date_published,
        container,
        publisher,
        description,
        license,
        subjects,
        funding_references,
        references,
        identifiers,
        files,
        version,
        additional_type,
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

    let mut url =
        Url::parse("https://api.crossref.org/works").expect("hardcoded Crossref URL should parse");
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

// ─── Writer ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CrOutput {
    status: &'static str,
    #[serde(rename = "message-type")]
    message_type: &'static str,
    #[serde(rename = "message-version")]
    message_version: &'static str,
    message: CrWork,
}

#[derive(Serialize)]
struct CrListOutput {
    status: &'static str,
    #[serde(rename = "message-type")]
    message_type: &'static str,
    #[serde(rename = "message-version")]
    message_version: &'static str,
    message: CrListMessage,
}

#[derive(Serialize)]
struct CrListMessage {
    #[serde(rename = "total-results")]
    total_results: usize,
    items: Vec<CrWork>,
}

#[derive(Serialize)]
struct CrVersion {
    version: String,
}

#[derive(Default, Serialize)]
struct CrWork {
    #[serde(rename = "DOI", skip_serializing_if = "String::is_empty")]
    doi: String,
    #[serde(rename = "type")]
    type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtype: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    title: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subtitle: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    author: Vec<CrAuthor>,
    #[serde(skip_serializing_if = "String::is_empty")]
    publisher: String,
    #[serde(rename = "container-title", skip_serializing_if = "Vec::is_empty")]
    container_title: Vec<String>,
    #[serde(rename = "group-title", skip_serializing_if = "Option::is_none")]
    group_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<String>,
    #[serde(rename = "ISSN", skip_serializing_if = "Vec::is_empty")]
    issn: Vec<String>,
    #[serde(rename = "abstract", skip_serializing_if = "String::is_empty")]
    abstract_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    language: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subject: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    license: Vec<CrLicense>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    funder: Vec<CrFunder>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    reference: Vec<CrReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    posted: Option<CrDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issued: Option<CrDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<CrVersion>,
    #[serde(rename = "URL", skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resource: Option<CrResource>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    link: Vec<CrLink>,
}

#[derive(Default, Serialize)]
struct CrAuthor {
    #[serde(rename = "ORCID", skip_serializing_if = "String::is_empty")]
    orcid: String,
    #[serde(rename = "authenticated-orcid", skip_serializing_if = "Option::is_none")]
    authenticated_orcid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    given: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    sequence: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    affiliation: Vec<CrAffiliation>,
}

#[derive(Default, Serialize)]
struct CrAffiliation {
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    id: Vec<CrAffiliationId>,
}

#[derive(Serialize)]
struct CrAffiliationId {
    id: String,
    #[serde(rename = "id-type")]
    id_type: String,
    #[serde(rename = "asserted-by")]
    asserted_by: String,
}

#[derive(Serialize)]
struct CrDate {
    #[serde(rename = "date-parts")]
    date_parts: Vec<Vec<i32>>,
}

#[derive(Serialize)]
struct CrLicense {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "content-version")]
    content_version: String,
}

#[derive(Serialize)]
struct CrFunder {
    #[serde(rename = "DOI", skip_serializing_if = "String::is_empty")]
    doi: String,
    name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    award: Vec<String>,
}

#[derive(Default, Serialize)]
struct CrReference {
    key: String,
    #[serde(rename = "DOI", skip_serializing_if = "String::is_empty")]
    doi: String,
    #[serde(rename = "doi-asserted-by", skip_serializing_if = "String::is_empty")]
    doi_asserted_by: String,
    #[serde(rename = "article-title", skip_serializing_if = "String::is_empty")]
    article_title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    unstructured: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    year: Option<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    volume: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issue: String,
    #[serde(rename = "first-page", skip_serializing_if = "String::is_empty")]
    first_page: String,
    #[serde(rename = "last-page", skip_serializing_if = "String::is_empty")]
    last_page: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    publisher: String,
}

#[derive(Serialize)]
struct CrResource {
    primary: CrPrimaryResource,
}

#[derive(Serialize)]
struct CrPrimaryResource {
    #[serde(rename = "URL")]
    url: String,
}

#[derive(Serialize)]
struct CrLink {
    #[serde(rename = "URL")]
    url: String,
    #[serde(rename = "content-type")]
    content_type: String,
}

fn cm_to_cr_json_type(cm: &str) -> &'static str {
    match cm {
        "Article" | "BlogPost" => "posted-content",
        "Blog" => "journal",
        "BookChapter" => "book-chapter",
        "BookSeries" => "book-series",
        "Book" => "book",
        "Component" => "component",
        "Dataset" => "dataset",
        "Dissertation" => "dissertation",
        "Grant" => "grant",
        "JournalArticle" => "journal-article",
        "JournalIssue" => "journal-issue",
        "JournalVolume" => "journal-volume",
        "Journal" => "journal",
        "PeerReview" => "peer-review",
        "ProceedingsArticle" => "proceedings-article",
        "ProceedingsSeries" => "proceedings-series",
        "Proceedings" => "proceedings",
        "ReportComponent" => "report-component",
        "ReportSeries" => "report-series",
        "Report" => "report",
        "Standard" => "standard",
        _ => "other",
    }
}

fn parse_date_to_cr(date: &str) -> Option<CrDate> {
    if date.is_empty() {
        return None;
    }
    let date_only = date.split('T').next().unwrap_or(date);
    let parts: Vec<i32> = date_only
        .split('-')
        .filter_map(|p| p.parse().ok())
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(CrDate {
            date_parts: vec![parts],
        })
    }
}

fn strip_doi_prefix(id: &str) -> String {
    id.trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/")
        .to_string()
}

fn to_cr_work(data: &Data) -> CrWork {
    let doi = strip_doi_prefix(&data.id);

    let type_ = cm_to_cr_json_type(&data.type_).to_string();

    let mut title = Vec::new();
    let mut subtitle = Vec::new();
    if !data.title.is_empty() {
        title.push(data.title.clone());
    }
    for t in &data.additional_titles {
        if t.type_ == "Subtitle" {
            subtitle.push(t.title.clone());
        } else if !t.title.is_empty() {
            title.push(t.title.clone());
        }
    }

    let author: Vec<CrAuthor> = data
        .contributors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sequence = if i == 0 { "first" } else { "additional" }.to_string();
            if c.type_ == "Organization" {
                let org_name = c.organization.as_ref().map(|o| o.name.clone()).unwrap_or_default();
                CrAuthor {
                    name: Some(org_name),
                    sequence,
                    ..Default::default()
                }
            } else if let Some(p) = &c.person {
                let orcid = p.id.clone();
                let authenticated_orcid = if orcid.is_empty() {
                    None
                } else {
                    Some(p.asserted_by == "Author")
                };
                let affiliation: Vec<CrAffiliation> = p
                    .affiliations
                    .iter()
                    .map(|a| {
                        let ids = if a.id.is_empty() {
                            Vec::new()
                        } else {
                            let id_type = if a.id.contains("ror.org") {
                                "ROR"
                            } else {
                                "other"
                            };
                            vec![CrAffiliationId {
                                id: a.id.clone(),
                                id_type: id_type.to_string(),
                                asserted_by: a.asserted_by.to_lowercase(),
                            }]
                        };
                        CrAffiliation {
                            name: a.name.clone(),
                            id: ids,
                        }
                    })
                    .collect();
                CrAuthor {
                    orcid,
                    authenticated_orcid,
                    given: if p.given_name.is_empty() { None } else { Some(p.given_name.clone()) },
                    family: if p.family_name.is_empty() { None } else { Some(p.family_name.clone()) },
                    sequence,
                    affiliation,
                    ..Default::default()
                }
            } else {
                CrAuthor {
                    sequence,
                    ..Default::default()
                }
            }
        })
        .collect();

    // posted-content uses group-title for the container name; all other types
    // use the standard container-title array.
    let is_posted_content = type_ == "posted-content";
    let (container_title, group_title) = if data.container.title.is_empty() {
        (Vec::new(), None)
    } else if is_posted_content {
        (Vec::new(), Some(data.container.title.clone()))
    } else {
        (vec![data.container.title.clone()], None)
    };

    let issn = if data.container.identifier_type == "ISSN" && !data.container.identifier.is_empty()
    {
        vec![data.container.identifier.clone()]
    } else {
        Vec::new()
    };

    let page = {
        let fp = &data.container.first_page;
        let lp = &data.container.last_page;
        if !fp.is_empty() && !lp.is_empty() {
            Some(format!("{}-{}", fp, lp))
        } else if !fp.is_empty() {
            Some(fp.clone())
        } else {
            None
        }
    };

    let license = if !data.license.url.is_empty() {
        vec![CrLicense {
            url: data.license.url.clone(),
            content_version: "vor".to_string(),
        }]
    } else {
        Vec::new()
    };

    // Group funding references by funder_id+funder_name preserving insertion order.
    let funder: Vec<CrFunder> = {
        let mut seen: Vec<(String, String, Vec<String>)> = Vec::new();
        for f in &data.funding_references {
            let key = format!("{}|{}", f.funder_id, f.funder_name);
            if let Some(entry) = seen.iter_mut().find(|e| format!("{}|{}", e.0, e.1) == key) {
                if !f.award_number.is_empty() {
                    entry.2.push(f.award_number.clone());
                }
            } else {
                let awards = if f.award_number.is_empty() {
                    Vec::new()
                } else {
                    vec![f.award_number.clone()]
                };
                seen.push((f.funder_id.clone(), f.funder_name.clone(), awards));
            }
        }
        seen.into_iter()
            .map(|(funder_id, funder_name, award)| CrFunder {
                doi: strip_doi_prefix(&funder_id),
                name: funder_name,
                award,
            })
            .collect()
    };

    let reference: Vec<CrReference> = data
        .references
        .iter()
        .map(|r| {
            let doi = strip_doi_prefix(&r.id);
            let doi_asserted_by = if doi.is_empty() {
                String::new()
            } else {
                r.asserted_by.to_lowercase()
            };
            let year = if r.publication_year.is_empty() {
                None
            } else {
                Some(r.publication_year.clone())
            };
            // Prefer the separately preserved article_title; fall back to
            // reference text (which may itself be an article title from sources
            // that never had a separate unstructured field).
            let article_title = if !r.title.is_empty() {
                r.title.clone()
            } else if r.unstructured.is_empty() {
                r.reference.clone()
            } else {
                String::new()
            };
            let unstructured = r.unstructured.clone();
            CrReference {
                key: r.key.clone(),
                doi,
                doi_asserted_by,
                article_title,
                unstructured,
                year,
                volume: r.volume.clone(),
                issue: r.issue.clone(),
                first_page: r.first_page.clone(),
                last_page: r.last_page.clone(),
                publisher: r.publisher.clone(),
            }
        })
        .collect();

    let issued = parse_date_to_cr(&data.date_published);
    let posted = if is_posted_content { parse_date_to_cr(&data.date_published) } else { None };

    let version = if data.version.is_empty() {
        None
    } else {
        Some(CrVersion { version: data.version.clone() })
    };

    let subtype = if data.additional_type.is_empty() {
        None
    } else {
        Some(data.additional_type.clone())
    };

    let resource = if data.url.is_empty() {
        None
    } else {
        Some(CrResource {
            primary: CrPrimaryResource {
                url: data.url.clone(),
            },
        })
    };

    let link: Vec<CrLink> = data
        .files
        .iter()
        .map(|f| CrLink {
            url: f.url.clone(),
            content_type: f.mime_type.clone(),
        })
        .collect();

    let doi_url = if doi.is_empty() {
        String::new()
    } else {
        format!("https://doi.org/{}", doi)
    };

    CrWork {
        doi,
        type_,
        subtype,
        title,
        subtitle,
        author,
        publisher: data.publisher.name.clone(),
        container_title,
        group_title,
        volume: if data.container.volume.is_empty() {
            None
        } else {
            Some(data.container.volume.clone())
        },
        issue: if data.container.issue.is_empty() {
            None
        } else {
            Some(data.container.issue.clone())
        },
        page,
        issn,
        abstract_text: data.description.clone(),
        language: if data.language.is_empty() {
            None
        } else {
            Some(data.language.clone())
        },
        subject: data.subjects.iter().map(|s| s.subject.clone()).collect(),
        license,
        funder,
        reference,
        posted,
        issued,
        version,
        url: doi_url,
        resource,
        link,
    }
}

/// Serialize a `Data` record as a Crossref REST API JSON response.
pub fn write(data: &Data) -> Result<Vec<u8>> {
    let output = CrOutput {
        status: "ok",
        message_type: "work",
        message_version: "1.0.0",
        message: to_cr_work(data),
    };
    serde_json::to_vec_pretty(&output).map_err(|e| Error::Parse(e.to_string()))
}

/// Serialize a list of `Data` records as a Crossref REST API work-list response.
pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let items: Vec<CrWork> = list.iter().map(to_cr_work).collect();
    let total = items.len();
    let output = CrListOutput {
        status: "ok",
        message_type: "work-list",
        message_version: "1.0.0",
        message: CrListMessage {
            total_results: total,
            items,
        },
    };
    serde_json::to_vec_pretty(&output).map_err(|e| Error::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_journal_article() {
        let input = std::fs::read_to_string(
            "tests/fixtures/commonmeta/crossref_journal_article.json",
        )
        .unwrap();
        let data = crate::formats::commonmeta::read(&input).unwrap();
        let bytes = write(&data).unwrap();
        let out: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(out["status"], "ok");
        assert_eq!(out["message-type"], "work");
        assert_eq!(out["message"]["DOI"], "10.5555/12345678");
        assert_eq!(out["message"]["type"], "journal-article");
        assert_eq!(out["message"]["title"][0], "A Study of Things");
        assert_eq!(out["message"]["publisher"], "Example Publisher");
        assert_eq!(out["message"]["volume"], "12");
        assert_eq!(out["message"]["issue"], "3");
        assert_eq!(out["message"]["page"], "100-110");
        assert_eq!(out["message"]["ISSN"][0], "1234-5678");
        assert_eq!(out["message"]["language"], "en");
        let author = &out["message"]["author"][0];
        assert_eq!(author["given"], "Ada");
        assert_eq!(author["family"], "Lovelace");
        assert_eq!(author["ORCID"], "https://orcid.org/0000-0000-0000-0000");
        assert_eq!(author["authenticated-orcid"], false);
        assert_eq!(author["sequence"], "first");
        assert_eq!(author["affiliation"][0]["name"], "Example University");
        assert_eq!(author["affiliation"][0]["id"][0]["id-type"], "ROR");
        assert_eq!(author["affiliation"][0]["id"][0]["asserted-by"], "publisher");
        let reference = &out["message"]["reference"][0];
        assert_eq!(reference["key"], "ref1");
        assert_eq!(reference["DOI"], "10.1000/xyz");
        assert_eq!(reference["doi-asserted-by"], "publisher");
        assert_eq!(out["message"]["resource"]["primary"]["URL"], "https://example.com/articles/a-study-of-things");
        assert_eq!(out["message"]["issued"]["date-parts"][0][0], 2024);
        assert_eq!(out["message"]["issued"]["date-parts"][0][1], 3);
        assert_eq!(out["message"]["issued"]["date-parts"][0][2], 15);
    }

    /// Real-world VRAIX Crossref dumps use explicit JSON `null` (not a
    /// missing key) for several optional string fields, which
    /// `#[serde(default)]` alone does not catch since default only fires
    /// when the key is absent.
    #[test]
    fn test_read_json_tolerates_null_publisher() {
        let json = r#"{"message":{
            "DOI":"10.1/a",
            "type":"journal-article",
            "title":["A Title"],
            "publisher":null
        }}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.id, "https://doi.org/10.1/a");
        assert_eq!(data.publisher.name, "");
    }

    /// Array elements can also be an explicit `null`, e.g. `"title":[null]`.
    #[test]
    fn test_read_json_tolerates_null_title_element() {
        let json = r#"{"message":{
            "DOI":"10.1/a",
            "type":"journal-article",
            "title":[null]
        }}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.title, "");
    }

    #[test]
    fn test_read_json_tolerates_null_affiliation_name() {
        let json = r#"{"message":{
            "DOI":"10.1/a",
            "type":"journal-article",
            "title":["A Title"],
            "author":[{"name":"Some Org","affiliation":[{"name":null}]}]
        }}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.contributors[0].affiliations()[0].name, "");
    }

    #[test]
    fn test_read_json_maps_subtitle_to_subtitle_title() {
        let json = r#"{"message":{
            "DOI":"10.1/a",
            "type":"journal-article",
            "title":["Main Title"],
            "subtitle":["Sub Title"]
        }}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.title, "Main Title");
        assert_eq!(data.additional_titles.len(), 1);
        assert_eq!(data.additional_titles[0].title, "Sub Title");
        assert_eq!(data.additional_titles[0].type_, "Subtitle");
    }

    #[test]
    fn test_read_json_without_doi_does_not_add_doi_identifier() {
        let json = r#"{"message":{
            "DOI":"",
            "type":"journal-article",
            "title":["A Title"],
            "resource":{"primary":{"URL":"https://example.org/article"}}
        }}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.id, "https://example.org/article");
        assert!(data.identifiers.is_empty());
    }
}
