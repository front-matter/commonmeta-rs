use chrono::DateTime;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Deserializer};

/// Deserializes a JSON null as T::default() instead of an error.
fn null_default<'de, D, T>(d: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

use crate::data::{
    Affiliation, Container, Contributor, Data, Date, Description, File, FundingReference,
    Identifier, License, Publisher, Reference, Relation, Subject, Title,
};
use crate::doi_utils::{normalize_doi, validate_doi, validate_prefix};
use crate::error::{Error, Result};
use crate::utils::validate_orcid;

// ─── Input structs ────────────────────────────────────────────────────────────

#[allow(dead_code)]
#[derive(Deserialize)]
pub struct Query {
    pub items: Vec<Content>,
    #[serde(rename = "total-results", default)]
    pub total_results: i64,
}

#[allow(dead_code)]
#[derive(Deserialize, Default)]
pub struct Content {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub doi: String,
    #[serde(default)]
    pub guid: String,
    #[serde(default)]
    pub rid: String,
    #[serde(rename = "abstract", default)]
    pub abstract_: String,
    #[serde(default)]
    pub archive_url: String,
    #[serde(default)]
    pub authors: Vec<Author>,
    #[serde(default)]
    pub blog: Blog,
    #[serde(default)]
    pub blog_name: String,
    #[serde(default)]
    pub blog_slug: String,
    #[serde(default)]
    pub content_html: String,
    #[serde(rename = "image", default)]
    pub feature_image: String,
    #[serde(default)]
    pub indexed_at: i64,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub published_at: i64,
    #[serde(default)]
    pub relationships: Vec<JfRelation>,
    #[serde(default)]
    pub reference: Vec<JfReference>,
    #[serde(default)]
    pub funding_references: Vec<JfFundingReference>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub topic: Option<u32>,
    #[serde(default)]
    pub topic_score: f64,
    #[serde(default)]
    pub images: Vec<JfImage>,
}

#[derive(Deserialize, Default)]
pub struct JfImage {
    #[serde(default)]
    pub src: String,
}

#[derive(Deserialize, Default)]
pub struct Author {
    #[serde(default)]
    pub given: String,
    #[serde(default)]
    pub family: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub affiliation: Vec<JfAffiliation>,
}

#[derive(Deserialize, Default)]
pub struct JfAffiliation {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Default)]
pub struct Blog {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    #[serde(default, deserialize_with = "null_default")]
    pub category: String,
    #[serde(default, deserialize_with = "null_default")]
    pub description: String,
    #[serde(default, deserialize_with = "null_default")]
    pub favicon: String,
    #[serde(default)]
    pub funding: JfFundingReference,
    #[serde(default, deserialize_with = "null_default")]
    pub generator: String,
    #[serde(default, deserialize_with = "null_default")]
    pub home_page_url: String,
    #[serde(default, deserialize_with = "null_default")]
    pub issn: String,
    #[serde(default, deserialize_with = "null_default")]
    pub language: String,
    #[serde(default, deserialize_with = "null_default")]
    pub license: String,
    #[serde(default, deserialize_with = "null_default")]
    pub prefix: String,
    #[serde(default, deserialize_with = "null_default")]
    pub slug: String,
    #[serde(default, deserialize_with = "null_default")]
    pub subfield: String,
    #[serde(default, deserialize_with = "null_default")]
    pub status: String,
    #[serde(default, deserialize_with = "null_default")]
    pub title: String,
    #[serde(default)]
    pub doi_reg: bool,
}

#[derive(Deserialize, Default)]
pub struct JfFundingReference {
    #[serde(rename = "funderIdentifier", default)]
    pub funder_identifier: String,
    #[serde(rename = "funderIdentifierType", default)]
    pub funder_identifier_type: String,
    #[serde(rename = "funderName", default)]
    pub funder_name: String,
    #[serde(rename = "awardNumber", default)]
    pub award_number: String,
    #[serde(rename = "awardTitle", default)]
    pub award_title: String,
    #[serde(rename = "awardUri", default)]
    pub award_uri: String,
}

#[derive(Deserialize)]
pub struct JfRelation {
    #[serde(rename = "type", default)]
    pub type_: String,
    /// Single URL form
    #[serde(default)]
    pub url: Option<String>,
    /// List URL form
    #[serde(default)]
    pub urls: Vec<String>,
}

impl JfRelation {
    fn all_urls(&self) -> Vec<&str> {
        if let Some(u) = &self.url {
            vec![u.as_str()]
        } else {
            self.urls.iter().map(String::as_str).collect()
        }
    }
}

#[derive(Deserialize)]
pub struct JfReference {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub unstructured: String,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

const ROGUE_SCHOLAR_CROSSREF_PREFIXES: &[&str] = &[
    "10.13003", "10.53731", "10.54900", "10.57689", "10.59347",
    "10.59348", "10.59349", "10.59350", "10.63485", "10.64000",
];

const ROGUE_SCHOLAR_DATACITE_PREFIXES: &[&str] = &[
    "10.5438", "10.34732", "10.57689", "10.58079", "10.60804",
];

const RELATION_TYPES: &[&str] = &[
    "IsPartOf", "HasPart", "IsVariantFormOf", "IsOriginalFormOf", "IsIdenticalTo",
    "IsTranslationOf", "IsReviewOf", "HasReview", "IsPreprintOf", "HasPreprint",
    "IsSupplementTo", "IsSupplementedBy",
];

fn is_rogue_scholar_doi(doi: &str) -> bool {
    let prefix = match validate_prefix(doi) {
        Some(p) => p,
        None => return false,
    };
    ROGUE_SCHOLAR_CROSSREF_PREFIXES.contains(&prefix.as_str())
        || ROGUE_SCHOLAR_DATACITE_PREFIXES.contains(&prefix.as_str())
}

fn unix_to_iso(ts: i64) -> String {
    if ts == 0 {
        return String::new();
    }
    DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_default()
}

fn normalize_orcid(url: &str) -> String {
    if let Some(id) = validate_orcid(url) {
        return format!("https://orcid.org/{}", id);
    }
    String::new()
}

fn issn_as_url(issn: &str) -> String {
    if issn.is_empty() {
        return String::new();
    }
    format!("https://portal.issn.org/resource/ISSN/{}", issn)
}

fn community_slug_as_url(slug: &str) -> String {
    if slug.is_empty() {
        return String::new();
    }
    format!("https://rogue-scholar.org/api/communities/{}", slug)
}

fn url_to_spdx(url: &str) -> String {
    match url {
        u if u.contains("/licenses/by/4.0") => "CC-BY-4.0",
        u if u.contains("/licenses/by/3.0") => "CC-BY-3.0",
        u if u.contains("/licenses/by-sa/4.0") => "CC-BY-SA-4.0",
        u if u.contains("/licenses/by-sa/3.0") => "CC-BY-SA-3.0",
        u if u.contains("/licenses/by-nc/4.0") => "CC-BY-NC-4.0",
        u if u.contains("/licenses/by-nc-sa/4.0") => "CC-BY-NC-SA-4.0",
        u if u.contains("/licenses/by-nd/4.0") => "CC-BY-ND-4.0",
        u if u.contains("publicdomain/zero/1.0") => "CC0-1.0",
        _ => "",
    }
    .to_string()
}

fn sanitize(html: &str) -> String {
    lazy_static! {
        static ref TAG_RE: Regex = Regex::new(r"<[^>]+>").unwrap();
    }
    TAG_RE.replace_all(html, "").trim().to_string()
}

/// Split "Family, Given" or "Given Family" into (given, family, org_name).
fn parse_name(name: &str) -> (String, String, String) {
    let name = name.trim();
    if name.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    // "Family, Given" format
    if let Some(comma) = name.find(',') {
        let family = name[..comma].trim().to_string();
        let given = name[comma + 1..].trim().to_string();
        return (given, family, String::new());
    }
    // "Given Family" — last token is family
    let parts: Vec<&str> = name.splitn(2, ' ').collect();
    if parts.len() == 2 {
        return (parts[0].to_string(), parts[1].to_string(), String::new());
    }
    // Single token — treat as organization
    (String::new(), String::new(), name.to_string())
}


// ─── Core reader ─────────────────────────────────────────────────────────────

pub fn read(content: &Content) -> Result<Data> {
    let mut data = Data::default();

    // ── URL ──
    data.url = if content.blog.status == "archived" && !content.archive_url.is_empty() {
        normalize_url(&content.archive_url)
    } else if matches!(content.blog.status.as_str(), "active" | "expired") {
        normalize_url(&content.url)
    } else {
        normalize_url(&content.url)
    };

    // ── ID ──
    if !content.doi.is_empty() {
        data.id = normalize_doi(&content.doi);
    } else if !content.guid.is_empty() && !content.blog.prefix.is_empty() {
        // Python: validate_doi_from_guid(prefix, guid[:-2], checksum=False)
        // Try treating the GUID as a DOI directly (after stripping last 2 chars checksum)
        let trimmed = if content.guid.len() > 2 { &content.guid[..content.guid.len() - 2] } else { "" };
        let candidate = normalize_doi(trimmed);
        if !candidate.is_empty() {
            if let Some(p) = validate_prefix(&candidate) {
                if p == content.blog.prefix {
                    data.id = content.guid.clone();
                }
            }
        }
    }
    if data.id.is_empty() && !content.blog.prefix.is_empty() {
        data.id = crate::doi_utils::encode_doi(&content.blog.prefix);
    }
    if data.id.is_empty() {
        data.id = data.url.clone();
    }

    data.type_ = "BlogPost".to_string();

    // ── Container ──
    let (identifier, identifier_type) = if !content.blog.issn.is_empty() {
        (content.blog.issn.clone(), "ISSN".to_string())
    } else if !content.blog_slug.is_empty() {
        (
            format!("https://rogue-scholar.org/blogs/{}", content.blog_slug),
            "URL".to_string(),
        )
    } else {
        (content.blog.home_page_url.clone(), "URL".to_string())
    };

    data.container = Container {
        type_: "Blog".to_string(),
        title: content.blog.title.clone(),
        identifier,
        identifier_type: identifier_type.clone(),
        platform: content.blog.generator.clone(),
        ..Default::default()
    };

    // ── IsPartOf relations ──
    if !content.blog_slug.is_empty() {
        data.relations.push(Relation {
            id: community_slug_as_url(&content.blog_slug),
            type_: "IsPartOf".to_string(),
        });
    }
    if !content.blog.issn.is_empty() {
        data.relations.push(Relation {
            id: issn_as_url(&content.blog.issn),
            type_: "IsPartOf".to_string(),
        });
    }

    // ── Contributors ──
    for author in &content.authors {
        let (given, family, org_name) = if author.given.is_empty() && author.family.is_empty() {
            if author.name.is_empty() {
                continue;
            }
            parse_name(&author.name)
        } else {
            (author.given.clone(), author.family.clone(), String::new())
        };

        let type_ = if !given.is_empty() { "Person" } else { "Organization" }.to_string();
        let id = normalize_orcid(&author.url);

        let affiliations: Vec<Affiliation> = author
            .affiliation
            .iter()
            .filter(|a| !a.name.is_empty())
            .map(|a| Affiliation { id: a.id.clone(), name: a.name.clone(), ..Default::default() })
            .collect();

        data.contributors.push(Contributor {
            id,
            type_,
            name: org_name,
            given_name: given,
            family_name: family,
            affiliations,
            contributor_roles: vec!["Author".to_string()],
        });
    }

    // ── Dates ──
    data.date = Date {
        published: unix_to_iso(content.published_at),
        updated: unix_to_iso(content.updated_at),
        ..Default::default()
    };

    // ── Description ──
    let description = if !content.abstract_.is_empty() {
        sanitize(&content.abstract_)
    } else {
        sanitize(&content.summary)
    };
    if !description.is_empty() {
        data.descriptions.push(Description {
            description,
            type_: "Abstract".to_string(),
            language: String::new(),
        });
    }

    // ── Files: feature image + images array (Python style) ──
    if !content.feature_image.is_empty() && normalize_url(&content.feature_image) != "" {
        data.files.push(File {
            url: content.feature_image.clone(),
            ..Default::default()
        });
    }
    for img in &content.images {
        if !img.src.is_empty() {
            data.files.push(File { url: img.src.clone(), ..Default::default() });
        }
    }
    // Deduplicate files by URL
    data.files.dedup_by(|a, b| a.url == b.url);

    // ── Provider ──
    let is_rs_doi = is_rogue_scholar_doi(&data.id);
    let rs_url_in_container = identifier_type == "URL"
        && data.container.identifier.contains("rogue-scholar.org");
    if is_rs_doi || rs_url_in_container {
        data.provider = "Crossref".to_string();
    }

    // ── Funding references ──
    data.funding_references = get_funding_references(content);

    // ── Identifiers: only guid (Python only stores guid) ──
    if !content.guid.is_empty() {
        data.identifiers.push(Identifier {
            identifier: content.guid.clone(),
            identifier_type: "GUID".to_string(),
        });
    }

    // ── Language ──
    data.language = content.language.clone();

    // ── License ──
    let license_url = normalize_license_url(&content.blog.license);
    let spdx_id = url_to_spdx(&license_url);
    data.license = License { id: spdx_id, url: license_url };

    // ── Publisher: Front Matter for Rogue Scholar DOIs or rogue-scholar.org container ──
    if is_rs_doi || rs_url_in_container {
        data.publisher = Publisher { name: "Front Matter".to_string(), ..Default::default() };
    }

    // ── Relations from `relationships` ──
    for rel in &content.relationships {
        if !RELATION_TYPES.contains(&rel.type_.as_str()) {
            continue;
        }
        for u in rel.all_urls() {
            let normalized = normalize_url(u);
            if !normalized.is_empty() {
                data.relations.push(Relation { id: normalized, type_: rel.type_.clone() });
            }
        }
    }

    // ── References ──
    for r in &content.reference {
        // Normalise ID: DOI takes priority over plain URL
        let id = if !r.id.is_empty() {
            if validate_doi(&r.id).is_some() {
                normalize_doi(&r.id)
            } else {
                normalize_url(&r.id)
            }
        } else {
            String::new()
        };

        let reference = Reference {
            key: r.key.clone(),
            id,
            type_: r.type_.clone(),
            unstructured: r.unstructured.clone(),
            ..Default::default()
        };
        let dup_key = !r.key.is_empty() && data.references.iter().any(|x| x.key == r.key);
        let dup_id =
            !reference.id.is_empty() && data.references.iter().any(|x| x.id == reference.id);
        if !dup_key && !dup_id {
            data.references.push(reference);
        }
    }

    // ── Subjects: OpenAlex subfield from blog.subfield + tags ──
    if !content.blog.subfield.is_empty() {
        // Subfield label will be looked up externally; store subject with id when available
        data.subjects.push(Subject { subject: content.blog.subfield.clone() });
    }
    for tag in &content.tags {
        data.subjects.push(Subject { subject: tag.clone() });
    }

    // ── Title ──
    data.titles.push(Title { title: sanitize(&content.title), ..Default::default() });

    // ── Version ──
    data.version = if content.version.is_empty() {
        "v1".to_string()
    } else {
        content.version.clone()
    };

    data.content_html = content.content_html.clone();
    data.feature_image = content.feature_image.clone();

    Ok(data)
}

fn get_funding_references(content: &Content) -> Vec<FundingReference> {
    let mut refs: Vec<FundingReference> = Vec::new();

    if !content.blog.funding.funder_name.is_empty() {
        refs.push(FundingReference {
            funder_name: content.blog.funding.funder_name.clone(),
            funder_identifier: content.blog.funding.funder_identifier.clone(),
            funder_identifier_type: content.blog.funding.funder_identifier_type.clone(),
            award_title: content.blog.funding.award_title.clone(),
            award_number: content.blog.funding.award_number.clone(),
            award_uri: content.blog.funding.award_uri.clone(),
        });
    }

    if !content.funding_references.is_empty() {
        for v in &content.funding_references {
            refs.push(FundingReference {
                funder_name: v.funder_name.clone(),
                funder_identifier: v.funder_identifier.clone(),
                funder_identifier_type: v.funder_identifier_type.clone(),
                award_title: v.award_title.clone(),
                award_number: v.award_number.clone(),
                award_uri: v.award_uri.clone(),
            });
        }
        return refs;
    }

    // Funding from HasAward relationships
    for rel in &content.relationships {
        if rel.type_ != "HasAward" {
            continue;
        }
        let urls: Vec<&str> = rel.all_urls();
        if urls.len() == 1 {
            let u = urls[0];
            let prefix = validate_prefix(u).unwrap_or_default();
            let is_cordis = url::Url::parse(u)
                .map(|p| p.host_str() == Some("cordis.europa.eu"))
                .unwrap_or(false);
            if prefix == "10.3030" || is_cordis {
                let award_number = url::Url::parse(u)
                    .ok()
                    .and_then(|p| p.path_segments().and_then(|s| s.last().map(String::from)))
                    .unwrap_or_default();
                refs.push(FundingReference {
                    funder_name: "European Commission".to_string(),
                    funder_identifier: "https://ror.org/00k4n6c32".to_string(),
                    funder_identifier_type: "ROR".to_string(),
                    award_number,
                    award_uri: u.to_string(),
                    ..Default::default()
                });
            }
        } else if urls.len() == 2 {
            let funder_url = urls[0];
            let award_url = urls[1];
            let prefix = validate_prefix(funder_url).unwrap_or_default();
            if prefix == "10.13039" {
                let (funder_name, funder_id, funder_id_type) =
                    if funder_url == "https://doi.org/10.13039/100000001" {
                        (
                            "National Science Foundation".to_string(),
                            "https://ror.org/021nxhr62".to_string(),
                            "ROR".to_string(),
                        )
                    } else {
                        (String::new(), String::new(), String::new())
                    };
                let award_number = extract_award_number(award_url);
                refs.push(FundingReference {
                    funder_name,
                    funder_identifier: funder_id,
                    funder_identifier_type: funder_id_type,
                    award_number,
                    award_uri: award_url.to_string(),
                    ..Default::default()
                });
            } else if crate::utils::validate_ror(funder_url).is_some() {
                let award_number = extract_award_number(award_url);
                refs.push(FundingReference {
                    funder_identifier: funder_url.to_string(),
                    funder_identifier_type: "ROR".to_string(),
                    award_number,
                    award_uri: award_url.to_string(),
                    ..Default::default()
                });
            }
        }
    }

    refs
}

fn extract_award_number(u: &str) -> String {
    url::Url::parse(u)
        .ok()
        .and_then(|p| {
            // Prefer ?awd_id= query param, fall back to last path segment
            p.query_pairs()
                .find(|(k, _)| k == "awd_id")
                .map(|(_, v)| v.into_owned())
                .or_else(|| p.path_segments().and_then(|s| s.last().map(String::from)))
        })
        .unwrap_or_default()
}

fn normalize_url(u: &str) -> String {
    match url::Url::parse(u) {
        Ok(p) if p.scheme() == "http" || p.scheme() == "https" => {
            // Remove trailing slash
            let s = p.as_str();
            s.trim_end_matches('/').to_string()
        }
        _ => String::new(),
    }
}

fn normalize_license_url(u: &str) -> String {
    let s = u
        .replace("http://", "https://")
        .trim_end_matches('/')
        .to_string();
    // Replace creativecommons.org/licenses with canonical legalcode URL
    if s.contains("creativecommons.org") && !s.ends_with("legalcode") {
        return format!("{}/legalcode", s);
    }
    s
}

// ─── Public entry points ──────────────────────────────────────────────────────

/// Parse a single JSON Feed item (as returned by the Rogue Scholar API).
pub fn read_json(json: &str) -> Result<Data> {
    let content: Content =
        serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))?;
    read(&content)
}

/// Fetch a post from the Rogue Scholar API by UUID, DOI, or API URL.
pub fn fetch(id: &str) -> Result<Data> {
    let api_url = build_api_url(id)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("commonmeta-rs/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;

    let resp = client
        .get(&api_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(Error::Http(format!("HTTP {}", resp.status())));
    }

    let text = resp.text().map_err(|e| Error::Http(e.to_string()))?;
    read_json(&text)
}

fn build_api_url(id: &str) -> Result<String> {
    use crate::utils::validate_id;
    let (_, id_type) = validate_id(id);
    match id_type {
        "JSONFEEDID" => Ok(id.to_string()),
        "DOI" => {
            let bare = validate_doi(id).ok_or_else(|| Error::InvalidId(id.to_string()))?;
            Ok(format!("https://api.rogue-scholar.org/posts/{}", bare))
        }
        "UUID" => Ok(format!("https://api.rogue-scholar.org/posts/{}", id)),
        _ => Err(Error::InvalidId(id.to_string())),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn load_fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jsonfeed")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn parse_blog_post() {
        let json = load_fixture("jsonfeed_blog_post.json");
        let data = read_json(&json).unwrap();
        assert_eq!(data.type_, "BlogPost");
        assert!(!data.id.is_empty(), "id should be set");
        assert!(!data.titles.is_empty(), "should have a title");
    }

    #[test]
    fn unix_timestamp_conversion() {
        assert_eq!(unix_to_iso(1711238400), "2024-03-24T00:00:00Z");
        assert_eq!(unix_to_iso(0), "");
    }

    #[test]
    fn parse_name_formats() {
        let (g, f, o) = parse_name("Lovelace, Ada");
        assert_eq!(f, "Lovelace");
        assert_eq!(g, "Ada");
        assert!(o.is_empty());

        let (g2, f2, o2) = parse_name("Ada Lovelace");
        assert_eq!(g2, "Ada");
        assert_eq!(f2, "Lovelace");
        assert!(o2.is_empty());

        // Single token → org
        let (g3, f3, o3) = parse_name("Anthropic");
        assert!(g3.is_empty());
        assert!(f3.is_empty());
        assert_eq!(o3, "Anthropic");
    }

    #[test]
    fn spdx_mapping() {
        assert_eq!(url_to_spdx("https://creativecommons.org/licenses/by/4.0/legalcode"), "CC-BY-4.0");
        assert_eq!(url_to_spdx("https://creativecommons.org/publicdomain/zero/1.0/legalcode"), "CC0-1.0");
        assert_eq!(url_to_spdx("https://example.com/unknown"), "");
    }
}
