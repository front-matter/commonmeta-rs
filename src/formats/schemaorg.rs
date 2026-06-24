//! Schema.org JSON-LD reader and writer.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::author_utils::{
    cleanup_author, infer_contributor_type, normalize_contributor_roles, parse_affiliation_value,
    split_person_name,
};
use crate::constants as C;
use crate::data::{Contributor, Data, Identifier, Organization, Person, Publisher, Subject, Title};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::{Error, Result};
use crate::utils::{
    normalize_cc_url, normalize_id, normalize_orcid, normalize_ror, normalize_url, sanitize,
    validate_id,
};

/// Extract a language code from an `inLanguage` value, which can be either a
/// bare string (`"en"`) or an object (`{"alternateName":"en","name":"English"}`).
fn extract_in_language(v: &Option<serde_json::Value>) -> String {
    match v {
        None => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(m)) => m
            .get("alternateName")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

// ── Input structs ─────────────────────────────────────────────────────────────

/// Schema.org contributor (author / creator / editor / contributor).
#[derive(Deserialize, Default, Clone)]
pub(crate) struct SoContributor {
    #[serde(rename = "@id", default)]
    pub(crate) id: String,
    #[serde(rename = "@type", default)]
    pub(crate) type_: String,
    #[serde(rename = "givenName", default)]
    pub(crate) given_name: String,
    #[serde(rename = "familyName", default)]
    pub(crate) family_name: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) affiliation: Option<Value>,
}

/// Citation / reference entry.
#[allow(dead_code)]
#[derive(Deserialize, Default)]
struct SoCitation {
    #[serde(rename = "@id", default)]
    id: String,
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(default)]
    name: String,
}

/// Periodical (journal) embedded in a ScholarlyArticle.
#[allow(dead_code)]
#[derive(Deserialize, Default)]
struct SoPeriodical {
    #[serde(rename = "@id", default)]
    _id: String,
    #[serde(rename = "@type", default)]
    _type: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    issn: String,
}

/// Publisher embedded in Schema.org JSON-LD.
#[derive(Deserialize, Default)]
struct SoPublisher {
    #[serde(rename = "@type", default)]
    _type: String,
    #[serde(default)]
    name: String,
}

/// The main Schema.org content struct.
/// Polymorphic fields (author, identifier, keywords, version) are kept as
/// `Option<Value>` so they can be either a single object/string or an array.
#[derive(Deserialize, Default)]
struct SoContent {
    #[serde(rename = "@context", default)]
    _context: String,
    #[serde(rename = "@id", default)]
    id: String,
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(rename = "additionalType", default)]
    additional_type: String,
    // author / creator / contributor can be object or array
    #[serde(default)]
    author: Option<Value>,
    #[serde(default)]
    creator: Option<Value>,
    #[serde(default)]
    contributor: Option<Value>,
    // editor is treated separately (role = "Editor")
    #[serde(default)]
    editor: Option<Value>,
    // citation / references list
    #[allow(dead_code)]
    #[serde(default)]
    citation: Vec<SoCitation>,
    #[serde(rename = "dateCreated", default)]
    date_created: String,
    #[serde(rename = "datePublished", default)]
    date_published: String,
    #[serde(rename = "dateModified", default)]
    date_modified: String,
    #[serde(default)]
    description: String,
    // headline is an alternative title field
    #[serde(default)]
    headline: String,
    // identifier can be a single string or array
    #[serde(default)]
    identifier: Option<Value>,
    // inLanguage can be a string ("en") or an object {"@type":"Language","alternateName":"en"}
    #[serde(rename = "inLanguage", default)]
    in_language: Option<Value>,
    // keywords can be comma-separated string or array
    #[serde(default)]
    keywords: Option<Value>,
    #[serde(default)]
    license: String,
    #[serde(default)]
    name: String,
    #[allow(dead_code)]
    #[serde(default)]
    periodical: Option<SoPeriodical>,
    #[serde(default)]
    publisher: Option<SoPublisher>,
    #[serde(default)]
    url: String,
    // version can be string or number
    #[serde(default)]
    version: Option<Value>,
}

// ── Contributor helpers ───────────────────────────────────────────────────────

/// Deserialise a `Value` that is either a single SoContributor object or an
/// array of them.
pub(crate) fn value_to_contributors(v: &Value) -> Vec<SoContributor> {
    if v.is_null() {
        return vec![];
    }
    if let Ok(single) = serde_json::from_value::<SoContributor>(v.clone()) {
        // Only treat as a single contributor if it has at least one name field
        if !single.name.is_empty()
            || !single.given_name.is_empty()
            || !single.family_name.is_empty()
        {
            return vec![single];
        }
    }
    serde_json::from_value::<Vec<SoContributor>>(v.clone()).unwrap_or_default()
}

/// Mirror of Go `GetContributor`.
pub(crate) fn get_contributor(v: SoContributor, default_role: &str) -> Contributor {
    let mut id = String::new();

    // Resolve @id — try ORCID first, then ROR
    if !v.id.is_empty() {
        let orcid = normalize_orcid(&v.id);
        if !orcid.is_empty() {
            id = orcid;
        } else {
            let ror = normalize_ror(&v.id);
            if !ror.is_empty() {
                id = ror;
            }
        }
    }

    let mut name = cleanup_author(Some(&v.name)).unwrap_or_default();
    let mut given_name = v.given_name.clone();
    let mut family_name = v.family_name.clone();

    // Only treat explicit Person/Organization values as real contributor types.
    let raw_type = match v.type_.as_str() {
        "Person" | "Organization" => v.type_.as_str(),
        _ => "",
    };

    let mut type_ = infer_contributor_type(
        raw_type,
        &id,
        &given_name,
        &family_name,
        &name,
        None,
    );
    if type_.is_empty() {
        type_ = "Organization".to_string();
    }

    // Split combined name for Persons
    if type_ == "Person" && !name.is_empty() && given_name.is_empty() && family_name.is_empty() {
        let (given, family, remainder) = split_person_name(&name);
        if !given.is_empty() || !family.is_empty() {
            given_name = given;
            family_name = family;
            name = String::new();
        } else {
            name = remainder;
        }
    }

    // Affiliation
    let affiliations = if let Some(aff_val) = &v.affiliation {
        // Can be a single object/string or an array
        if let Ok(list) = serde_json::from_value::<Vec<Value>>(aff_val.clone()) {
            list.iter().filter_map(parse_affiliation_value).collect()
        } else {
            parse_affiliation_value(aff_val).into_iter().collect()
        }
    } else {
        vec![]
    };

    let raw_roles = vec![v.type_.clone()];
    let roles = normalize_contributor_roles(&raw_roles, default_role);

    if type_ == "Person" {
        Contributor::person(
            Person {
                id,
                given_name,
                family_name,
                affiliations,
                asserted_by: String::new(),
            },
            roles,
        )
    } else {
        Contributor::organization(Organization { id, name, asserted_by: String::new() }, roles)
    }
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn from_content(content: SoContent) -> Data {
    // ID and Type computed up front for struct init
    let id = normalize_id(&content.id);
    let cm_type = C::so_to_cm(&content.type_);
    let type_ = if cm_type.is_empty() {
        "WebPage".to_string()
    } else {
        cm_type.to_string()
    };
    let mut data = Data {
        id,
        type_,
        additional_type: content.additional_type.clone(),
        ..Data::default()
    };

    let idents: Vec<String> = match &content.identifier {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    };

    // If @id is absent, recover canonical DOI id from identifier values.
    if data.id.is_empty()
        && let Some(doi_ident) = idents
            .iter()
            .find_map(|raw| validate_doi(raw).as_deref().map(normalize_doi))
    {
        data.id = doi_ident;
    }

    // Contributors from author/creator (role = Author)
    let author_val = content.author.or(content.creator).unwrap_or(Value::Null);
    for v in value_to_contributors(&author_val) {
        if v.name.is_empty() && v.given_name.is_empty() && v.family_name.is_empty() {
            continue;
        }
        let contrib = get_contributor(v, "Author");
        let dup = !contrib.id().is_empty()
            && data
                .contributors
                .iter()
                .any(|c| !c.id().is_empty() && c.id() == contrib.id());
        if !dup {
            data.contributors.push(contrib);
        }
    }

    // Contributors from contributor (role = Author unless type matches)
    if let Some(ref contrib_val) = content.contributor {
        for v in value_to_contributors(contrib_val) {
            if v.name.is_empty() && v.given_name.is_empty() && v.family_name.is_empty() {
                continue;
            }
            let contrib = get_contributor(v, "Author");
            let dup = !contrib.id().is_empty()
                && data
                    .contributors
                    .iter()
                    .any(|c| !c.id().is_empty() && c.id() == contrib.id());
            if !dup {
                data.contributors.push(contrib);
            }
        }
    }

    // Contributors from editor (role = Editor)
    if let Some(ref editor_val) = content.editor {
        for v in value_to_contributors(editor_val) {
            if v.name.is_empty() && v.given_name.is_empty() && v.family_name.is_empty() {
                continue;
            }
            let contrib = get_contributor(v, "Editor");
            let dup = !contrib.id().is_empty()
                && data
                    .contributors
                    .iter()
                    .any(|c| !c.id().is_empty() && c.id() == contrib.id());
            if !dup {
                data.contributors.push(contrib);
            }
        }
    }

    // Dates
    if !content.date_published.is_empty() {
        data.date_published = content.date_published.clone();
    }
    if !content.date_modified.is_empty() {
        data.date_updated = content.date_modified.clone();
    }
    if !content.date_created.is_empty() {
        data.dates.created = content.date_created.clone();
    }

    // Description
    if !content.description.is_empty() {
        data.description = sanitize(&content.description);
    }

    // Identifiers
    if let Some(doi) = validate_doi(&data.id) {
        data.identifiers.push(Identifier {
            identifier: normalize_doi(&doi),
            identifier_type: "DOI".to_string(),
        });
    }
    for id_str in &idents {
        let (identifier, identifier_type) = validate_id(id_str);
        if !identifier.is_empty() {
            let identifier = if identifier_type == "DOI" {
                normalize_doi(&identifier)
            } else {
                identifier
            };
            let duplicate = data
                .identifiers
                .iter()
                .any(|i| i.identifier == identifier && i.identifier_type == identifier_type);
            if !duplicate {
                data.identifiers.push(Identifier {
                    identifier,
                    identifier_type: identifier_type.to_string(),
                });
            }
        }
    }

    // Language — inLanguage can be "en" or {"@type":"Language","alternateName":"en","name":"English"}
    data.language = extract_in_language(&content.in_language);

    // License
    if !content.license.is_empty() {
        let (url, ok) = normalize_cc_url(&content.license);
        let url = if ok { url } else { content.license.clone() };
        data.license = crate::spdx::from_url(&url);
    }

    // Provider — inferred from DOI RA
    if let Some(bare) = validate_doi(&data.id) {
        use crate::doi_utils::get_doi_ra_sync;
        if let Some(ra) = get_doi_ra_sync(&bare) {
            data.provider = ra;
        }
    }

    // Publisher
    if let Some(pub_) = content.publisher
        && !pub_.name.is_empty()
    {
        data.publisher = Publisher {
            name: pub_.name,
            ..Default::default()
        };
    }

    // Subjects (keywords — string or array)
    let keywords: Vec<String> = match &content.keywords {
        Some(Value::String(s)) => s.split(',').map(|k| k.trim().to_string()).collect(),
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    };
    for kw in keywords {
        if !kw.is_empty() {
            data.subjects.push(Subject {
                subject: kw,
                ..Default::default()
            });
        }
    }

    // Titles
    if !content.name.is_empty() {
        data.title = content.name.clone();
    }
    if !content.headline.is_empty() {
        if data.title.is_empty() {
            data.title = content.headline.clone();
        } else {
            data.additional_titles.push(Title {
                title: content.headline.clone(),
                type_: "Subtitle".to_string(),
                ..Default::default()
            });
        }
    }

    // URL
    if let Some(url) = normalize_url(&content.url, true, false) {
        data.url = url;
    }

    // Version — string or number
    if let Some(v) = &content.version {
        data.version = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => format!("{}", n),
            _ => String::new(),
        };
    }

    data
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Read a Schema.org JSON-LD string directly into `Data`.
pub fn read_json(input: &str) -> Result<Data> {
    let content: SoContent =
        serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_content(content))
}

/// Fetch a URL, extract its JSON-LD, and parse into `Data`.
///
/// falls back to `<meta>` tags, then dispatches to Crossref or DataCite when the
/// embedded DOI belongs to one of those registrars.
pub fn fetch(url: &str) -> Result<Data> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .redirect(reqwest::redirect::Policy::limited(5))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let html = client
        .get(url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;

    let content = extract_content(&html)?;

    // If DOI belongs to a known RA, hand off to that reader
    if let Some(bare) = validate_doi(&content.id) {
        use crate::doi_utils::get_doi_ra_sync;
        if let Some(ra) = get_doi_ra_sync(&bare) {
            match ra.as_str() {
                "Crossref" => return crate::formats::crossref::fetch(&content.id),
                "DataCite" => return crate::formats::datacite::fetch(&content.id),
                _ => {}
            }
        }
    }

    Ok(from_content(content))
}

/// Extract Schema.org JSON-LD (+ meta-tag fallbacks) from raw HTML.
fn extract_content(html: &str) -> Result<SoContent> {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);

    // ── JSON-LD ───────────────────────────────────────────────────────────────
    let script_sel = Selector::parse("script[type='application/ld+json']").expect("valid selector");
    let mut content = SoContent::default();
    for el in doc.select(&script_sel) {
        let text = el.text().collect::<String>();
        if let Ok(parsed) = serde_json::from_str::<SoContent>(&text) {
            content = parsed;
            break;
        }
    }

    // ── @id / DOI fallbacks ───────────────────────────────────────────────────
    if content.id.is_empty() {
        let meta_names = [
            "meta[name='citation_doi']",
            "meta[name='dc.identifier']",
            "meta[name='DC.identifier']",
            "meta[name='bepress_citation_doi']",
        ];
        for sel_str in &meta_names {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
                && let Some(val) = el.value().attr("content")
            {
                content.id = val.to_string();
                break;
            }
        }
    }

    // ── Type fallback ─────────────────────────────────────────────────────────
    if content.type_.is_empty() {
        let type_metas = [
            "meta[property='og:type']",
            "meta[name='dc.type']",
            "meta[name='DC.type']",
        ];
        for sel_str in &type_metas {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
                && let Some(val) = el.value().attr("content")
            {
                content.type_ = val.to_string();
                break;
            }
        }
    }

    // ── Name / headline fallbacks ─────────────────────────────────────────────
    if content.name.is_empty() {
        let name_metas = [
            "meta[name='citation_title']",
            "meta[name='dc.title']",
            "meta[name='DC.title']",
            "meta[property='og:title']",
            "meta[name='twitter:title']",
        ];
        for sel_str in &name_metas {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
            {
                let val = el.value().attr("content").unwrap_or("").trim().to_string();
                if !val.is_empty() {
                    content.name = val;
                    break;
                }
            }
        }
    }

    // ── Description fallbacks ─────────────────────────────────────────────────
    if content.description.is_empty() {
        let desc_metas = [
            "meta[name='citation_abstract']",
            "meta[name='dc.description']",
            "meta[property='og:description']",
            "meta[name='twitter:description']",
        ];
        for sel_str in &desc_metas {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
            {
                let val = el.value().attr("content").unwrap_or("").trim().to_string();
                if !val.is_empty() {
                    content.description = val;
                    break;
                }
            }
        }
    }

    // ── Date published fallbacks ──────────────────────────────────────────────
    if content.date_published.is_empty() {
        let date_metas = [
            "meta[name='citation_publication_date']",
            "meta[name='citation_date']",
            "meta[name='dc.date']",
            "meta[property='article:published_time']",
        ];
        for sel_str in &date_metas {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
                && let Some(val) = el.value().attr("content")
            {
                content.date_published = val.to_string();
                break;
            }
        }
    }

    // ── Date modified fallbacks ───────────────────────────────────────────────
    if content.date_modified.is_empty() {
        let mod_metas = [
            "meta[name='og:updated_time']",
            "meta[name='article:modified_time']",
        ];
        for sel_str in &mod_metas {
            if let Ok(sel) = Selector::parse(sel_str)
                && let Some(el) = doc.select(&sel).next()
                && let Some(val) = el.value().attr("content")
            {
                content.date_modified = val.to_string();
                break;
            }
        }
    }

    // ── Language fallback ─────────────────────────────────────────────────────
    if extract_in_language(&content.in_language).is_empty()
        && let Ok(sel) = Selector::parse("html")
        && let Some(el) = doc.select(&sel).next()
        && let Some(lang) = el.value().attr("lang")
    {
        content.in_language = Some(Value::String(lang.to_string()));
    }

    // ── License fallback ──────────────────────────────────────────────────────
    if content.license.is_empty()
        && let Ok(sel) = Selector::parse("link[rel='license']")
        && let Some(el) = doc.select(&sel).next()
        && let Some(href) = el.value().attr("href")
    {
        content.license = href.to_string();
    }

    // ── author/creator synonyms ───────────────────────────────────────────────
    if content.author.is_none()
        && let Some(creator) = content.creator.take()
    {
        content.author = Some(creator);
    }

    Ok(content)
}

// ── Writer ────────────────────────────────────────────────────────────────────

// ── Output structs ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OutOrganization {
    #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(rename = "@type")]
    type_: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
}

#[derive(Serialize)]
struct OutContributor {
    #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(rename = "@type")]
    type_: &'static str,
    #[serde(rename = "givenName", skip_serializing_if = "String::is_empty")]
    given_name: String,
    #[serde(rename = "familyName", skip_serializing_if = "String::is_empty")]
    family_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    affiliation: Option<OutOrganization>,
}

#[derive(Serialize)]
struct OutCitation {
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@type")]
    type_: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
}

#[derive(Serialize, Default)]
struct OutPeriodical {
    #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(rename = "@type")]
    type_: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issn: String,
}

#[derive(Serialize)]
struct OutDataCatalog {
    #[serde(rename = "@id", skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(rename = "@type")]
    type_: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
}

#[derive(Serialize)]
struct OutMediaObject {
    #[serde(rename = "@type")]
    type_: &'static str,
    #[serde(rename = "contentUrl")]
    content_url: String,
    #[serde(rename = "encodingFormat", skip_serializing_if = "String::is_empty")]
    encoding_format: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    sha256: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    size: String,
}

#[derive(Serialize)]
struct OutProvider {
    #[serde(rename = "@type")]
    type_: &'static str,
    name: String,
}

#[derive(Serialize)]
struct OutPublisher {
    #[serde(rename = "@type")]
    type_: &'static str,
    name: String,
}

#[derive(Serialize)]
struct OutPayload {
    #[serde(rename = "@context")]
    context: &'static str,
    #[serde(rename = "@id")]
    id: String,
    #[serde(rename = "@type")]
    type_: String,
    #[serde(rename = "additionalType", skip_serializing_if = "String::is_empty")]
    additional_type: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    author: Vec<OutContributor>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    editor: Vec<OutContributor>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    citation: Vec<OutCitation>,
    #[serde(
        rename = "includedInDataCatalog",
        skip_serializing_if = "Option::is_none"
    )]
    included_in_data_catalog: Option<OutDataCatalog>,
    #[serde(skip_serializing_if = "Option::is_none")]
    periodical: Option<OutPeriodical>,
    #[serde(rename = "dateCreated", skip_serializing_if = "String::is_empty")]
    date_created: String,
    #[serde(rename = "datePublished", skip_serializing_if = "String::is_empty")]
    date_published: String,
    #[serde(rename = "dateModified", skip_serializing_if = "String::is_empty")]
    date_modified: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    headline: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    distribution: Vec<OutMediaObject>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    encoding: Vec<OutMediaObject>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifier: Vec<String>,
    #[serde(rename = "inLanguage", skip_serializing_if = "String::is_empty")]
    in_language: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    keywords: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    license: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(rename = "pageStart", skip_serializing_if = "String::is_empty")]
    page_start: String,
    #[serde(rename = "pageEnd", skip_serializing_if = "String::is_empty")]
    page_end: String,
    provider: OutProvider,
    publisher: OutPublisher,
    #[serde(skip_serializing_if = "String::is_empty")]
    url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    version: String,
}

// ── Conversion ────────────────────────────────────────────────────────────────

fn convert(data: &crate::data::Data) -> OutPayload {
    // Authors and editors
    let mut authors: Vec<OutContributor> = Vec::new();
    let mut editors: Vec<OutContributor> = Vec::new();

    for c in &data.contributors {
        let is_author = c.roles.contains(&"Author".to_string());
        let is_editor = c.roles.contains(&"Editor".to_string());
        if !is_author && !is_editor {
            continue;
        }

        let affiliation = c.affiliations().first().map(|a| OutOrganization {
            id: a.id.clone(),
            type_: "Organization",
            name: a.name.clone(),
        });

        let out = if c.type_ == "Organization" {
            OutContributor {
                id: c.id().to_string(),
                type_: "Organization",
                given_name: String::new(),
                family_name: String::new(),
                name: c.name(),
                affiliation: None,
            }
        } else {
            OutContributor {
                id: c.id().to_string(),
                type_: "Person",
                given_name: c.given_name().to_string(),
                family_name: c.family_name().to_string(),
                name: String::new(),
                affiliation,
            }
        };

        if is_author {
            authors.push(out);
        } else {
            editors.push(out);
        }
    }

    // Citations from references
    let citation: Vec<OutCitation> = data
        .references
        .iter()
        .filter(|r| !r.id.is_empty())
        .map(|r| {
            let type_ = if r.type_ == "JournalArticle" {
                "ScholarlyArticle".to_string()
            } else {
                "CreativeWork".to_string()
            };
            OutCitation {
                id: r.id.clone(),
                type_,
                name: r.reference.clone(),
            }
        })
        .collect();

    // Container → periodical or data catalog
    let (included_in_data_catalog, periodical) = if data.type_ == "Dataset" {
        let cat = OutDataCatalog {
            id: data.container.identifier.clone(),
            type_: "DataCatalog",
            name: data.container.title.clone(),
        };
        (Some(cat), None)
    } else {
        let (id, issn) = if data.container.identifier_type == "ISSN" {
            (String::new(), data.container.identifier.clone())
        } else {
            (data.container.identifier.clone(), String::new())
        };
        let p = OutPeriodical {
            id,
            type_: "Periodical",
            name: data.container.title.clone(),
            issn,
        };
        // Only include periodical if it has something meaningful
        if p.name.is_empty() && p.issn.is_empty() && p.id.is_empty() {
            (None, None)
        } else {
            (None, Some(p))
        }
    };

    // Files → MediaObject
    let media_objects: Vec<OutMediaObject> = data
        .files
        .iter()
        .map(|f| OutMediaObject {
            type_: "MediaObject",
            content_url: f.url.clone(),
            encoding_format: f.mime_type.clone(),
            name: f.key.clone(),
            sha256: f.checksum.clone(),
            size: if f.size > 0 {
                f.size.to_string()
            } else {
                String::new()
            },
        })
        .collect();

    let (distribution, encoding) = if data.type_ == "Dataset" {
        (media_objects, vec![])
    } else {
        (vec![], media_objects)
    };

    // Identifiers
    let mut identifier: Vec<String> = data.identifiers.iter().map(|i| i.identifier.clone()).collect();
    if let Some(doi) = validate_doi(&data.id) {
        let doi_url = normalize_doi(&doi);
        if !identifier.iter().any(|i| i == &doi_url) {
            identifier.push(doi_url);
        }
    }

    // Keywords
    let keywords = if data.subjects.is_empty() {
        String::new()
    } else {
        data.subjects
            .iter()
            .filter(|s| !s.subject.is_empty())
            .map(|s| s.subject.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Title
    let name = data.title.clone();
    let headline = data
        .additional_titles
        .iter()
        .find(|t| !t.title.is_empty() && t.type_ == "Subtitle")
        .map(|t| t.title.clone())
        .unwrap_or_default();

    // Description
    let description = data.description.clone();

    OutPayload {
        context: "http://schema.org",
        id: data.id.clone(),
        type_: C::cm_to_so(&data.type_).to_string(),
        additional_type: data.additional_type.clone(),
        author: authors,
        editor: editors,
        citation,
        included_in_data_catalog,
        periodical,
        date_created: data.dates.created.clone(),
        date_published: data.date_published.clone(),
        date_modified: data.date_updated.clone(),
        description,
        distribution,
        encoding,
        identifier,
        in_language: data.language.clone(),
        keywords,
        license: data.license.url.clone(),
        headline,
        name,
        page_start: data.container.first_page.clone(),
        page_end: data.container.last_page.clone(),
        provider: OutProvider {
            type_: "Organization",
            name: data.provider.clone(),
        },
        publisher: OutPublisher {
            type_: "Organization",
            name: data.publisher.name.clone(),
        },
        url: data.url.clone(),
        version: data.version.clone(),
    }
}

pub fn write(data: &crate::data::Data) -> crate::error::Result<Vec<u8>> {
    let payload = convert(data);
    serde_json::to_vec(&payload).map_err(|e| crate::error::Error::Serialize(e.to_string()))
}

pub fn write_all(list: &[crate::data::Data]) -> crate::error::Result<Vec<u8>> {
    let payloads: Vec<OutPayload> = list.iter().map(convert).collect();
    serde_json::to_vec_pretty(&payloads).map_err(|e| crate::error::Error::Serialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemaorg_reader_adds_doi_identifier_from_id() {
        let input = r#"{
          "@context": "https://schema.org",
          "@id": "https://doi.org/10.5555/12345678",
          "@type": "ScholarlyArticle",
          "name": "A Study of Things"
        }"#;

        let data = read_json(input).unwrap();
        assert!(data.identifiers.iter().any(|i| {
            i.identifier_type == "DOI" && i.identifier == "https://doi.org/10.5555/12345678"
        }));
    }

    #[test]
    fn schemaorg_reader_maps_headline_as_subtitle_when_name_present() {
        let input = r#"{
          "@context": "https://schema.org",
          "@type": "ScholarlyArticle",
          "name": "Main Title",
          "headline": "Subtitle Text"
        }"#;

        let data = read_json(input).unwrap();
        assert_eq!(data.title, "Main Title");
        assert_eq!(data.additional_titles.len(), 1);
        assert_eq!(data.additional_titles[0].title, "Subtitle Text");
        assert_eq!(data.additional_titles[0].type_, "Subtitle");
    }

    #[test]
    fn schemaorg_writer_prefers_primary_title_and_sets_headline_from_subtitle() {
        let data = Data {
            type_: "JournalArticle".to_string(),
            title: "Primary Title".to_string(),
            additional_titles: vec![Title {
                title: "Only Subtitle".to_string(),
                type_: "Subtitle".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };

        let out = write(&data).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["name"], "Primary Title");
        assert_eq!(json["headline"], "Only Subtitle");
    }
}
