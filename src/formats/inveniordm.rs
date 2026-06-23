use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::author_utils::{
    cleanup_author, infer_contributor_type, normalize_contributor_roles, parse_affiliation_value,
    split_person_name,
};
use crate::constants as C;
use crate::data::{
    Citation, Container, Contributor, Data, Description, File, FundingReference, Identifier,
    Organization, Person, Publisher, Reference, Relation, Subject,
};
use crate::doi_utils::normalize_doi;
use crate::error::{Error, Result};
use crate::utils::{
    get_language, issn_as_url, normalize_id, normalize_orcid, normalize_ror, normalize_url,
    sanitize,
};

// ── API response structs ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct Content {
    // id can be string or int (Zenodo uses int)
    id: Option<Value>,
    // top-level doi field (Zenodo compatibility)
    #[serde(default)]
    doi: String,
    // Zenodo conceptdoi for IsVersionOf relation
    #[serde(default)]
    conceptdoi: String,
    #[serde(default)]
    parent: Parent,
    #[serde(default)]
    pids: Pids,
    links: Option<ContentLinks>,
    // ISO 8601 updated timestamp
    #[serde(default)]
    updated: String,
    metadata: MetadataJSON,
    #[serde(rename = "custom_fields", default)]
    custom_fields: CustomFields,
    // top-level files list (new InvenioRDM format)
    #[serde(default)]
    files: Option<Value>,
}

#[derive(Deserialize, Default)]
struct ContentLinks {
    #[serde(rename = "self_html", default)]
    self_html: String,
}

#[derive(Deserialize, Default)]
struct Parent {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    communities: Communities,
}

#[derive(Deserialize, Default)]
struct Pids {
    #[serde(default)]
    doi: Doi,
}

#[derive(Deserialize, Default)]
struct Doi {
    #[serde(default)]
    identifier: String,
}

#[derive(Deserialize, Default)]
struct MetadataJSON {
    #[serde(rename = "resource_type", default)]
    resource_type: ResourceType,
    #[serde(default)]
    creators: Vec<Creator>,
    #[serde(default)]
    contributors: Vec<Creator>,
    #[serde(default)]
    funding: Vec<Funding>,
    #[serde(default)]
    grants: Vec<Grant>,
    // dates type field can be struct or plain string — use raw Value
    #[serde(default)]
    dates: Vec<DateJSON>,
    #[serde(default)]
    description: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    identifiers: Vec<InvenioIdentifier>,
    #[serde(default)]
    keywords: Vec<Value>,
    #[serde(default)]
    language: String,
    #[serde(default)]
    languages: Vec<Language>,
    // old license field (Zenodo)
    license: Option<OldLicense>,
    #[serde(default)]
    publisher: String,
    #[serde(rename = "publication_date", default)]
    publication_date: String,
    #[serde(default)]
    references: Vec<InvenioReference>,
    #[serde(rename = "related_identifiers", default)]
    related_identifiers: Vec<RelatedIdentifier>,
    #[serde(default)]
    rights: Vec<Right>,
    #[serde(default)]
    subjects: Vec<Subject_>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize, Default)]
struct ResourceType {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subtype: String,
    #[serde(rename = "type", default)]
    type_: String,
}

#[derive(Deserialize, Default)]
struct Creator {
    #[serde(rename = "person_or_org", default)]
    person_or_org: PersonOrOrg,
    #[serde(default)]
    affiliations: Vec<InvenioAffiliation>,
    // contributor role (metadata.contributors only)
    role: Option<ContributorRole>,
    // Zenodo legacy fields
    #[serde(default)]
    name: String,
    #[serde(default)]
    orcid: String,
    #[serde(default)]
    affiliation: String,
}

#[derive(Deserialize, Default)]
struct ContributorRole {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize, Default)]
struct PersonOrOrg {
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "given_name", default)]
    given_name: String,
    #[serde(rename = "family_name", default)]
    family_name: String,
    #[serde(default)]
    identifiers: Vec<InvenioIdentifier>,
}

#[derive(Deserialize, Default)]
struct InvenioAffiliation {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default, Clone)]
struct InvenioIdentifier {
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    scheme: String,
}

#[derive(Deserialize, Default)]
struct Funding {
    #[serde(default)]
    funder: Funder,
    #[serde(default)]
    award: Award,
}

#[derive(Deserialize, Default)]
struct Funder {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct Award {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    number: String,
    title: Option<AwardTitle>,
    #[serde(default)]
    identifiers: Vec<InvenioIdentifier>,
}

#[derive(Deserialize, Default)]
struct AwardTitle {
    #[serde(default)]
    en: String,
}

#[derive(Deserialize, Default)]
struct Grant {
    #[serde(default)]
    code: String,
    #[serde(default)]
    funder: LegacyFunder,
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
}

#[derive(Deserialize, Default)]
struct LegacyFunder {
    #[serde(default)]
    doi: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct DateJSON {
    #[serde(default)]
    date: String,
    // type can be {"id": "..."} or a plain string
    #[serde(rename = "type")]
    type_: Option<Value>,
}

#[derive(Deserialize, Default)]
struct Language {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize, Default)]
struct OldLicense {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize, Default)]
struct InvenioReference {
    #[serde(default)]
    reference: String,
    #[serde(default)]
    scheme: String,
    #[serde(default)]
    identifier: String,
}

#[derive(Deserialize, Default)]
struct RelatedIdentifier {
    #[serde(default)]
    identifier: String,
    #[serde(default)]
    scheme: String,
    #[serde(rename = "relation_type", default)]
    relation_type: RelationType,
    // Zenodo legacy: plain string relation type
    #[serde(default)]
    relation: String,
}

#[derive(Deserialize, Default)]
struct RelationType {
    #[serde(default)]
    id: String,
}

#[derive(Deserialize, Default)]
struct Right {
    #[serde(default)]
    id: String,
    #[serde(default)]
    #[allow(dead_code)]
    props: RightProps,
}

#[derive(Deserialize, Default)]
struct RightProps {
    #[allow(dead_code)]
    #[serde(default)]
    url: String,
}

#[derive(Deserialize, Default)]
struct Subject_ {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    scheme: String,
}

#[derive(Deserialize, Default)]
struct Communities {
    #[serde(default)]
    default: String,
    #[serde(default)]
    entries: Vec<Community>,
}

#[derive(Deserialize, Default)]
struct Community {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    #[allow(dead_code)]
    metadata: CommunityMetadata,
}

#[derive(Deserialize, Default)]
struct CommunityMetadata {
    #[serde(rename = "type", default)]
    #[allow(dead_code)]
    type_: CommunityType,
}

#[derive(Deserialize, Default)]
struct CommunityType {
    #[serde(default)]
    #[allow(dead_code)]
    id: String,
}

#[derive(Deserialize, Default)]
struct CustomFields {
    #[serde(rename = "journal:journal", default)]
    journal: Journal,
    #[serde(rename = "rs:content_html", default)]
    content_html: String,
    #[serde(rename = "rs:image", default)]
    feature_image: String,
    #[serde(rename = "rs:generator", default)]
    generator: String,
    #[serde(rename = "rs:citations", default)]
    citations: Vec<InvenioReference>,
}

#[derive(Deserialize, Default)]
struct Journal {
    #[serde(default)]
    title: String,
    #[serde(default)]
    issn: String,
    #[serde(default)]
    volume: String,
    #[serde(default)]
    issue: String,
    #[serde(default)]
    pages: String,
}

#[derive(Deserialize, Default)]
struct ContentFile {
    #[serde(default)]
    bucket: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    checksum: String,
    links: Option<FileLinks>,
    #[serde(default)]
    size: i64,
    #[serde(rename = "type", default)]
    type_: String,
}

#[derive(Deserialize, Default)]
struct FileLinks {
    #[serde(rename = "self", default)]
    self_: String,
}

// ── Type mappings ─────────────────────────────────────────────────────────────

fn invenio_to_cm_type(id: &str) -> &'static str {
    C::inveniordm_to_cm(id)
}

fn is_valid_relation_type(t: &str) -> bool {
    C::COMMONMETA_RELATION_TYPES.contains(&t)
}

/// InvenioRDM lowercase relation_type.id → Commonmeta CamelCase type.
fn invenio_to_cm_relation(id: &str) -> &'static str {
    match id {
        "iscitedby" => "IsCitedBy",
        "cites" => "Cites",
        "issupplementto" => "IsSupplementTo",
        "issupplementedby" => "IsSupplementedBy",
        "iscontinuedby" => "IsContinuedBy",
        "continues" => "Continues",
        "isnewversionof" => "IsNewVersionOf",
        "ispreviousversion" | "ispreviousversionof" => "IsPreviousVersionOf",
        "ispartof" => "IsPartOf",
        "haspart" => "HasPart",
        "isreferencedby" => "IsReferencedBy",
        "references" => "References",
        "isdocumentedby" => "IsDocumentedBy",
        "documents" => "Documents",
        "iscompiledby" => "IsCompiledBy",
        "compiles" => "Compiles",
        "isvariantformof" => "IsVariantFormOf",
        "isoriginalformof" => "IsOriginalFormOf",
        "isidenticalto" => "IsIdenticalTo",
        "istranslationof" => "IsTranslationOf",
        "isreviewedby" => "HasReview",
        "reviews" => "IsReviewOf",
        "ispreprintof" => "IsPreprintOf",
        "haspreprint" => "HasPreprint",
        "isderivedfrom" => "IsDerivedFrom",
        "issourceof" => "IsSourceOf",
        "describes" => "Describes",
        "isdescribedby" => "IsDescribedBy",
        "ismetadatafor" => "IsMetadataFor",
        "hasmetadata" => "HasMetadata",
        "isannotatedby" => "IsAnnotatedBy",
        "annotates" => "Annotates",
        "iscorrectedby" => "IsCorrectedBy",
        "corrects" => "Corrects",
        _ => "",
    }
}

fn is_reference_relation(id: &str) -> bool {
    matches!(id, "cites" | "references")
}

/// Rogue Scholar DOI prefixes (Crossref-registered).
fn is_rogue_scholar_doi(doi: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "10.13003", "10.53731", "10.54900", "10.59347", "10.59348", "10.59349", "10.59350",
        "10.63485", "10.63517", "10.64000", "10.64395", "10.65527",
    ];
    PREFIXES.iter().any(|p| doi.contains(p))
}

// ── Contributor conversion ────────────────────────────────────────────────────

fn get_contributor(v: &Creator, default_role: &str) -> Contributor {
    // Detect Zenodo legacy format: name directly on creator, no person_or_org
    if !v.name.is_empty()
        && v.person_or_org.name.is_empty()
        && v.person_or_org.family_name.is_empty()
    {
        return get_zenodo_contributor(v, default_role);
    }

    let raw_type = match v.person_or_org.type_.as_str() {
        "personal" => "Person",
        "organizational" => "Organization",
        _ => "",
    }
    .to_string();

    let mut id = String::new();
    for ni in &v.person_or_org.identifiers {
        match ni.scheme.as_str() {
            "orcid" => {
                id = normalize_orcid(&ni.identifier);
                break;
            }
            "ror" | "ROR" => {
                id = normalize_ror(&ni.identifier);
                break;
            }
            _ => {}
        }
    }

    let name = cleanup_author(Some(&v.person_or_org.name)).unwrap_or(v.person_or_org.name.clone());
    let mut given_name = v.person_or_org.given_name.clone();
    let mut family_name = v.person_or_org.family_name.clone();

    let mut type_ = infer_contributor_type(
        &raw_type,
        &id,
        &given_name,
        &family_name,
        &name,
        None,
    );

    if type_.is_empty() {
        type_ = "Organization".to_string();
    }

    // Split "Family, Given" for Person type when only name is provided
    let mut name_out = name.clone();
    if type_ == "Person" && !name_out.is_empty() && given_name.is_empty() && family_name.is_empty() {
        let (given, family, remainder) = split_person_name(&name_out);
        if !given.is_empty() || !family.is_empty() {
            given_name = given;
            family_name = family;
            name_out = String::new();
        } else {
            name_out = remainder;
        }
    }

    let affiliations = v
        .affiliations
        .iter()
        .filter_map(|a| {
            let value = serde_json::json!({"id": a.id, "name": a.name});
            parse_affiliation_value(&value)
        })
        .collect();

    let roles = normalize_contributor_roles(&[default_role.to_string()], default_role);

    if type_ == "Person" {
        Contributor::person(
            Person { id, given_name, family_name, affiliations },
            roles,
        )
    } else {
        Contributor::organization(
            Organization { id, name: name_out },
            roles,
        )
    }
}

fn get_zenodo_contributor(v: &Creator, default_role: &str) -> Contributor {
    let mut id = String::new();

    if !v.orcid.is_empty() {
        id = normalize_orcid(&v.orcid);
    }

    let cleaned_name = cleanup_author(Some(&v.name)).unwrap_or(v.name.clone());
    let (given_name, family_name, name) = split_person_name(&cleaned_name);

    let mut type_ = infer_contributor_type("", &id, &given_name, &family_name, &cleaned_name, None);
    if type_.is_empty() {
        type_ = "Organization".to_string();
    }

    let mut family_name_out = family_name;
    let mut name_out = name;
    if type_ == "Person" && family_name_out.is_empty() && !name_out.is_empty() {
        family_name_out = name_out.clone();
        name_out = String::new();
    }

    let affiliations = if !v.affiliation.is_empty() {
        parse_affiliation_value(&Value::String(v.affiliation.clone()))
            .into_iter()
            .collect()
    } else {
        vec![]
    };

    let roles = normalize_contributor_roles(&[default_role.to_string()], default_role);

    if type_ == "Person" {
        Contributor::person(
            Person { id, given_name, family_name: family_name_out, affiliations },
            roles,
        )
    } else {
        Contributor::organization(
            Organization { id, name: name_out },
            roles,
        )
    }
}

// ── Reference helpers ─────────────────────────────────────────────────────────

fn normalize_reference_id(scheme: &str, identifier: &str) -> String {
    if identifier.is_empty() {
        return String::new();
    }
    match scheme {
        "doi" => normalize_doi(identifier),
        "url" => normalize_url(identifier, true, false).unwrap_or_default(),
        _ => normalize_id(identifier),
    }
}

// ── Relation helpers ──────────────────────────────────────────────────────────

fn normalize_relation_id(scheme: &str, identifier: &str) -> String {
    if identifier.is_empty() {
        return String::new();
    }
    match scheme {
        "doi" => normalize_doi(identifier),
        _ => normalize_url(identifier, true, false).unwrap_or_else(|| normalize_id(identifier)),
    }
}

fn parse_pages_range(pages: &str) -> (String, String) {
    let trimmed = pages.trim();
    if trimmed.is_empty() {
        return (String::new(), String::new());
    }

    for sep in ["--", "-", "–", "—"] {
        if let Some(idx) = trimmed.find(sep) {
            let first = trimmed[..idx].trim().to_string();
            let last = trimmed[idx + sep.len()..].trim().to_string();
            return (first, last);
        }
    }

    (trimmed.to_string(), String::new())
}

/// Map InvenioRDM relation_type.id to Commonmeta type.
/// Falls back to capitalizing the first letter (Python behaviour).
fn map_relation_type(raw: &str) -> String {
    let mapped = invenio_to_cm_relation(raw);
    if !mapped.is_empty() {
        return mapped.to_string();
    }
    // Fallback: capitalize first letter (handles already-CamelCase values)
    if raw.is_empty() {
        return String::new();
    }
    let mut chars = raw.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn from_content(content: Content) -> Data {
    let mut data = Data {
        // ID
        id: if !content.doi.is_empty() {
            normalize_doi(&content.doi)
        } else {
            normalize_doi(&content.pids.doi.identifier)
        },
        ..Data::default()
    };

    // Type: Python prefers resource_type.type then resource_type.id
    let rt = &content.metadata.resource_type;
    let type_id = if !rt.type_.is_empty() {
        &rt.type_
    } else if !rt.id.is_empty() {
        &rt.id
    } else {
        &rt.subtype
    };
    let cm_type = invenio_to_cm_type(type_id);
    data.type_ = if cm_type.is_empty() {
        "Other".to_string()
    } else {
        cm_type.to_string()
    };

    // Detect host from links.self_html for Zenodo-specific handling
    let self_html = content
        .links
        .as_ref()
        .map(|l| l.self_html.as_str())
        .unwrap_or("");
    let host = url::Url::parse(self_html)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let is_zenodo = host == "zenodo.org";
    let is_rogue_scholar = is_rogue_scholar_doi(&data.id);

    // URL
    // For Rogue Scholar: URL from metadata.identifiers with scheme "url"
    // Otherwise: links.self_html
    if is_rogue_scholar {
        if let Some(url_id) = content
            .metadata
            .identifiers
            .iter()
            .find(|i| i.scheme == "url")
        {
            data.url = normalize_url(&url_id.identifier, true, false).unwrap_or_default();
        }
    } else if !self_html.is_empty() {
        data.url = normalize_url(self_html, true, false).unwrap_or_default();
    }

    // Container
    if is_zenodo {
        let container_type = if data.type_ == "Dataset" {
            "DataRepository"
        } else {
            "Repository"
        };
        data.container = Container {
            identifier: "https://www.re3data.org/repository/r3d100010468".to_string(),
            identifier_type: "URL".to_string(),
            type_: container_type.to_string(),
            title: "Zenodo".to_string(),
            ..Default::default()
        };
        data.publisher = Publisher {
            name: "Zenodo".to_string(),
            ..Default::default()
        };
    } else if is_rogue_scholar {
        let slug = content
            .parent
            .communities
            .entries
            .first()
            .map(|e| e.slug.as_str())
            .unwrap_or("");
        let issn = &content.custom_fields.journal.issn;
        let (identifier, identifier_type) = if !issn.is_empty() {
            (issn.clone(), "ISSN".to_string())
        } else if !slug.is_empty() {
            (
                format!("https://rogue-scholar.org/communities/{}", slug),
                "URL".to_string(),
            )
        } else {
            (String::new(), String::new())
        };
        let (first_page, last_page) = parse_pages_range(&content.custom_fields.journal.pages);
        data.container = Container {
            type_: "Blog".to_string(),
            title: content.custom_fields.journal.title.clone(),
            identifier,
            identifier_type,
            platform: content.custom_fields.generator.clone(),
            volume: content.custom_fields.journal.volume.clone(),
            issue: content.custom_fields.journal.issue.clone(),
            first_page,
            last_page,
            ..Default::default()
        };
        data.publisher = Publisher {
            name: "Front Matter".to_string(),
            ..Default::default()
        };
    } else if !content.custom_fields.journal.title.is_empty()
        || !content.custom_fields.journal.issn.is_empty()
    {
        let issn = &content.custom_fields.journal.issn;
        let (identifier, identifier_type) = if !issn.is_empty() {
            (issn.clone(), "ISSN".to_string())
        } else {
            (String::new(), String::new())
        };
        let (first_page, last_page) = parse_pages_range(&content.custom_fields.journal.pages);
        data.container = Container {
            type_: "Periodical".to_string(),
            title: content.custom_fields.journal.title.clone(),
            identifier,
            identifier_type,
            platform: content.custom_fields.generator.clone(),
            volume: content.custom_fields.journal.volume.clone(),
            issue: content.custom_fields.journal.issue.clone(),
            first_page,
            last_page,
            ..Default::default()
        };
    }

    // Publisher (from metadata, if not already set by Zenodo/Rogue Scholar logic)
    if data.publisher.name.is_empty() && !content.metadata.publisher.is_empty() {
        data.publisher = Publisher {
            name: content.metadata.publisher.clone(),
            ..Default::default()
        };
    }

    // BlogPost override: Article → BlogPost when publisher is "Front Matter"
    if data.type_ == "Article" && data.publisher.name == "Front Matter" {
        data.type_ = "BlogPost".to_string();
    }

    // Contributors from metadata.creators (all get "Author" role)
    for v in &content.metadata.creators {
        let contributor = get_contributor(v, "Author");
        let already = data
            .contributors
            .iter()
            .any(|e| !e.id().is_empty() && e.id() == contributor.id());
        if !already {
            data.contributors.push(contributor);
        }
    }
    // Contributors from metadata.contributors (with role from role.id)
    for v in &content.metadata.contributors {
        let role = v
            .role
            .as_ref()
            .map(|r| {
                let mut s = r.id.clone();
                if let Some(first) = s.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
                s
            })
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Other".to_string());
        let contributor = get_contributor(v, &role);
        let already = data
            .contributors
            .iter()
            .any(|e| !e.id().is_empty() && e.id() == contributor.id());
        if !already {
            data.contributors.push(contributor);
        }
    }

    // Dates: Python handles "issued" → published and "updated" → updated only
    for d in &content.metadata.dates {
        let t = date_type_str(&d.type_);
        match t.as_str() {
            "issued" => data.date_published = d.date.clone(),
            "updated" => data.date_updated = d.date.clone(),
            _ => {}
        }
    }
    if data.date_published.is_empty() && !content.metadata.publication_date.is_empty() {
        data.date_published = content.metadata.publication_date.clone();
    }
    // Fallback for updated: top-level meta.updated (strip milliseconds)
    if data.date_updated.is_empty() && !content.updated.is_empty() {
        data.date_updated = strip_milliseconds(&content.updated);
    }

    // Descriptions: description (Abstract) + notes (Other)
    if !content.metadata.description.is_empty() {
        data.description = sanitize(&content.metadata.description);
    }
    if !content.metadata.notes.is_empty() {
        data.additional_descriptions.push(Description {
            description: sanitize(&content.metadata.notes),
            type_: "Other".to_string(),
            ..Default::default()
        });
    }

    // Feature image
    if !content.custom_fields.feature_image.is_empty() {
        data.image = content.custom_fields.feature_image.clone();
    }

    // Files from top-level `files` list
    if let Some(files_val) = &content.files
        && let Ok(files_enabled) = serde_json::from_value::<FilesEnabled>(files_val.clone())
        && files_enabled.enabled
        && let Ok(entries) = serde_json::from_value::<FilesWithEntries>(files_val.clone())
    {
        for f in entries.entries.values() {
            if let Ok(cf) = serde_json::from_value::<ContentFile>(f.clone()) {
                let url = cf
                    .links
                    .as_ref()
                    .map(|l| l.self_.clone())
                    .unwrap_or_default();
                if !url.is_empty() {
                    let mime_type = if !cf.type_.is_empty() {
                        format!("application/{}", cf.type_)
                    } else {
                        String::new()
                    };
                    data.files.push(File {
                        bucket: cf.bucket,
                        key: cf.key,
                        checksum: cf.checksum,
                        url,
                        size: cf.size,
                        mime_type,
                    });
                }
            }
        }
    }

    // Funding references
    if !content.metadata.funding.is_empty() {
        for v in &content.metadata.funding {
            let funder_identifier = normalize_ror(&v.funder.id);
            let funder_identifier_type = if !funder_identifier.is_empty() {
                "ROR".to_string()
            } else {
                String::new()
            };
            let award_number = v.award.number.clone();
            let award_title = v
                .award
                .title
                .as_ref()
                .map(|t| t.en.clone())
                .unwrap_or_default();
            // award URI: from award.identifiers[0], normalize DOI if possible
            let raw_award_uri = v
                .award
                .identifiers
                .first()
                .map(|i| i.identifier.as_str())
                .unwrap_or("");
            let award_uri = if !raw_award_uri.is_empty() {
                let doi = normalize_doi(raw_award_uri);
                if !doi.is_empty() {
                    doi
                } else {
                    normalize_url(raw_award_uri, true, false).unwrap_or_default()
                }
            } else {
                String::new()
            };
            data.funding_references.push(FundingReference {
                funder_id: funder_identifier,
                funder_identifier_type,
                funder_name: v.funder.name.clone(),
                award_number,
                award_title,
                award_id: award_uri,
            });
        }
    } else if !content.metadata.grants.is_empty() {
        for v in &content.metadata.grants {
            let funder_identifier = normalize_doi(&v.funder.doi);
            let funder_identifier_type = if !funder_identifier.is_empty() {
                "Crossref Funder ID".to_string()
            } else {
                String::new()
            };
            let award_uri = normalize_url(&v.url, true, false).unwrap_or_default();
            data.funding_references.push(FundingReference {
                funder_id: funder_identifier,
                funder_identifier_type,
                funder_name: v.funder.name.clone(),
                award_number: v.code.clone(),
                award_title: v.title.clone(),
                award_id: award_uri,
            });
        }
    }

    // Identifiers: DOI first, then only doi/uuid/guid schemes from metadata
    if !data.id.is_empty() {
        data.identifiers.push(Identifier {
            identifier: data.id.clone(),
            identifier_type: "DOI".to_string(),
        });
    }
    for v in &content.metadata.identifiers {
        if v.scheme == "url" {
            // URL goes into data.url, not identifiers
            continue;
        }
        let identifier_type = match v.scheme.as_str() {
            "doi" => "DOI",
            "uuid" => "UUID",
            "guid" => "GUID",
            _ => continue,
        };
        data.identifiers.push(Identifier {
            identifier: v.identifier.clone(),
            identifier_type: identifier_type.to_string(),
        });
    }
    // RID from record id field
    if let Some(id_val) = &content.id
        && let Some(s) = id_val.as_str()
        && !s.is_empty()
    {
        data.identifiers.push(Identifier {
            identifier: s.to_string(),
            identifier_type: "RID".to_string(),
        });
    }

    // Language: metadata.language first, then metadata.languages[0].id
    if !content.metadata.language.is_empty() {
        data.language = get_language(&content.metadata.language, "iso639-1");
    } else if !content.metadata.languages.is_empty() {
        data.language = get_language(&content.metadata.languages[0].id, "iso639-1");
    }

    // License from rights list (new) or legacy license field
    if !content.metadata.rights.is_empty() {
        data.license = crate::spdx::from_id(&content.metadata.rights[0].id);
    } else if let Some(lic) = &content.metadata.license
        && !lic.id.is_empty()
    {
        data.license = crate::spdx::from_id(&lic.id);
    }

    // Provider
    data.provider = if is_rogue_scholar {
        "Crossref".to_string()
    } else {
        "DataCite".to_string()
    };

    // Subjects: metadata.subjects + metadata.keywords merged
    for v in &content.metadata.subjects {
        if v.id.contains("openalex.org") {
            let id_part = v.id.rsplit('/').next().unwrap_or("");
            if let Some((id, subject)) = crate::vocabularies::lookup_openalex_subject(id_part) {
                let subj = Subject { id, subject, ..Default::default() };
                if !data.subjects.contains(&subj) {
                    data.subjects.push(subj);
                }
            }
        } else {
            let s = subject_string(v);
            if s.is_empty() {
                continue;
            }
            let subj = Subject { subject: s, ..Default::default() };
            if !data.subjects.contains(&subj) {
                data.subjects.push(subj);
            }
        }
    }
    for kw in &content.metadata.keywords {
        let s = match kw {
            Value::String(s) => s.clone(),
            _ => continue,
        };
        if s.is_empty() {
            continue;
        }
        let subj = Subject { subject: s, ..Default::default() };
        if !data.subjects.contains(&subj) {
            data.subjects.push(subj);
        }
    }

    // References from metadata.references; fall back to related_identifiers if empty
    if !content.metadata.references.is_empty() {
        for v in &content.metadata.references {
            let id = normalize_reference_id(&v.scheme, &v.identifier);
            data.references.push(Reference {
                id,
                unstructured: v.reference.clone(),
                ..Default::default()
            });
        }
    } else {
        for v in &content.metadata.related_identifiers {
            let relation_id = relation_type_id(v);
            if is_reference_relation(&relation_id) {
                let id = normalize_relation_id(&v.scheme, &v.identifier);
                if !id.is_empty() {
                    data.references.push(Reference {
                        id,
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Citations (works that cite this resource) from custom_fields.rs:citations
    for v in &content.custom_fields.citations {
        let id = normalize_reference_id(&v.scheme, &v.identifier);
        data.citations.push(Citation {
            id,
            citation: v.reference.clone(),
            ..Default::default()
        });
    }

    // Relations from related_identifiers (excluding references)
    for v in &content.metadata.related_identifiers {
        let relation_id = relation_type_id(v);
        if is_reference_relation(&relation_id) {
            continue;
        }
        let id = normalize_relation_id(&v.scheme, &v.identifier);
        if id.is_empty() {
            continue;
        }
        let type_ = map_relation_type(&relation_id);
        if !type_.is_empty() && is_valid_relation_type(&type_) {
            let rel = Relation { id, type_ };
            if !data.relations.contains(&rel) {
                data.relations.push(rel);
            }
        }
    }

    // IsVersionOf relations
    if !content.conceptdoi.is_empty() {
        let id = normalize_doi(&content.conceptdoi);
        if !id.is_empty() {
            data.relations.push(Relation {
                id,
                type_: "IsVersionOf".to_string(),
            });
        }
    } else if data.id.contains("10.59350") && !content.parent.communities.default.is_empty() {
        let parent_id = &content.parent.communities.default;
        let id = normalize_doi(&format!("10.59350/{}", parent_id));
        if !id.is_empty() {
            data.relations.push(Relation {
                id,
                type_: "IsVersionOf".to_string(),
            });
        }
    }

    // ISSN IsPartOf relation for Rogue Scholar
    if is_rogue_scholar && !content.custom_fields.journal.issn.is_empty() {
        let issn_url = issn_as_url(&content.custom_fields.journal.issn);
        let rel = Relation {
            id: issn_url,
            type_: "IsPartOf".to_string(),
        };
        if !data.relations.contains(&rel) {
            data.relations.push(rel);
        }
    }

    // Title
    if !content.metadata.title.is_empty() {
        data.title = sanitize(&content.metadata.title);
    }

    // Version
    data.version = content.metadata.version.clone();

    // Full-text HTML
    if !content.custom_fields.content_html.is_empty() {
        data.content = content.custom_fields.content_html.clone();
    }

    data
}

// ── Small helpers ─────────────────────────────────────────────────────────────

fn date_type_str(type_: &Option<Value>) -> String {
    match type_ {
        Some(Value::Object(m)) => m
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn strip_milliseconds(ts: &str) -> String {
    // "2024-01-15T12:34:56.789012+00:00" → "2024-01-15T12:34:56+00:00"
    if let Some(dot) = ts.find('.') {
        let rest = &ts[dot + 1..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| i + dot + 1)
            .unwrap_or(ts.len());
        return format!("{}{}", &ts[..dot], &ts[end..]);
    }
    ts.to_string()
}

fn relation_type_id(v: &RelatedIdentifier) -> String {
    if !v.relation_type.id.is_empty() {
        v.relation_type.id.clone()
    } else {
        v.relation.to_lowercase()
    }
}

fn subject_string(v: &Subject_) -> String {
    if v.subject.is_empty() {
        return String::new();
    }
    match v.scheme.as_str() {
        "FOS" => format!("FOS: {}", v.subject),
        "Domains" => format!("Domain: {}", v.subject),
        "Fields" => format!("Field: {}", v.subject),
        "Subfields" => format!("Subfield: {}", v.subject),
        "Topics" => format!("Topic: {}", v.subject),
        _ => v.subject.clone(),
    }
}

// Minimal structs for parsing the `files` field
#[derive(Deserialize, Default)]
struct FilesEnabled {
    #[serde(default)]
    enabled: bool,
}

#[derive(Deserialize, Default)]
struct FilesWithEntries {
    #[serde(default)]
    entries: std::collections::HashMap<String, Value>,
}

// ── Writer ────────────────────────────────────────────────────────────────────

// ── Output structs ────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct OutInveniordm {
    pids: OutPids,
    access: OutAccess,
    files: OutFiles,
    metadata: OutMetadata,
    #[serde(
        rename = "custom_fields",
        skip_serializing_if = "OutCustomFields::is_empty"
    )]
    custom_fields: OutCustomFields,
}

#[derive(Serialize, Default)]
struct OutPids {
    #[serde(rename = "doi")]
    doi: OutDoi,
}

#[derive(Serialize, Default)]
struct OutDoi {
    identifier: String,
    provider: String,
}

#[derive(Serialize, Default)]
struct OutAccess {
    record: String,
    files: String,
}

#[derive(Serialize, Default)]
struct OutFiles {
    enabled: bool,
}

#[derive(Serialize, Default)]
struct OutMetadata {
    resource_type: OutResourceType,
    creators: Vec<OutCreator>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contributors: Vec<OutContributor>,
    title: String,
    publication_date: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    publisher: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifiers: Vec<OutIdentifier>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dates: Vec<OutDate>,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    funding: Vec<OutFunding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    languages: Vec<OutLanguage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subjects: Vec<OutSubject>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rights: Vec<OutRight>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    references: Vec<OutReference>,
    #[serde(rename = "related_identifiers", skip_serializing_if = "Vec::is_empty")]
    related_identifiers: Vec<OutRelatedIdentifier>,
    #[serde(skip_serializing_if = "String::is_empty")]
    version: String,
}

#[derive(Serialize, Default)]
struct OutResourceType {
    id: String,
}

#[derive(Serialize, Default)]
struct OutCreator {
    person_or_org: OutPersonOrOrg,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    affiliations: Vec<OutAffiliation>,
}

#[derive(Serialize, Default)]
struct OutPersonOrOrg {
    #[serde(rename = "type")]
    type_: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(rename = "given_name", skip_serializing_if = "String::is_empty")]
    given_name: String,
    #[serde(rename = "family_name", skip_serializing_if = "String::is_empty")]
    family_name: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifiers: Vec<OutIdentifier>,
}

#[derive(Serialize, Default)]
struct OutContributor {
    person_or_org: OutPersonOrOrg,
    role: OutTypeId,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    affiliations: Vec<OutAffiliation>,
}

#[derive(Serialize, Default)]
struct OutAffiliation {
    #[serde(skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
}

#[derive(Serialize, Default)]
struct OutIdentifier {
    identifier: String,
    scheme: String,
}

#[derive(Serialize, Default)]
struct OutDate {
    date: String,
    #[serde(rename = "type")]
    type_: OutTypeId,
}

#[derive(Serialize, Default)]
struct OutTypeId {
    id: String,
}

#[derive(Serialize, Default)]
struct OutFunding {
    funder: OutFunder,
    #[serde(skip_serializing_if = "OutAward::is_empty")]
    award: OutAward,
}

#[derive(Serialize, Default)]
struct OutFunder {
    #[serde(skip_serializing_if = "String::is_empty")]
    id: String,
    name: String,
}

#[derive(Serialize, Default)]
struct OutAward {
    #[serde(skip_serializing_if = "String::is_empty")]
    number: String,
    #[serde(skip_serializing_if = "OutAwardTitle::is_empty")]
    title: OutAwardTitle,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifiers: Vec<OutIdentifier>,
}

impl OutAward {
    fn is_empty(&self) -> bool {
        self.number.is_empty() && self.title.is_empty() && self.identifiers.is_empty()
    }
}

#[derive(Serialize, Default)]
struct OutAwardTitle {
    #[serde(skip_serializing_if = "String::is_empty")]
    en: String,
}

impl OutAwardTitle {
    fn is_empty(&self) -> bool {
        self.en.is_empty()
    }
}

#[derive(Serialize, Default)]
struct OutLanguage {
    id: String,
}

#[derive(Serialize, Default)]
struct OutSubject {
    subject: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    scheme: String,
}

#[derive(Serialize, Default)]
struct OutRight {
    id: String,
}

#[derive(Serialize, Default)]
struct OutReference {
    reference: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    scheme: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    identifier: String,
}

#[derive(Serialize, Default)]
struct OutRelatedIdentifier {
    identifier: String,
    scheme: String,
    relation_type: OutTypeId,
}

#[derive(Serialize, Default)]
struct OutCustomFields {
    #[serde(
        rename = "journal:journal",
        skip_serializing_if = "OutJournal::is_empty"
    )]
    journal: OutJournal,
    #[serde(rename = "rs:content_html", skip_serializing_if = "String::is_empty")]
    content_html: String,
    #[serde(rename = "rs:image", skip_serializing_if = "String::is_empty")]
    feature_image: String,
    #[serde(rename = "feed:generator", skip_serializing_if = "String::is_empty")]
    generator: String,
}

impl OutCustomFields {
    fn is_empty(&self) -> bool {
        self.journal.is_empty()
            && self.content_html.is_empty()
            && self.feature_image.is_empty()
            && self.generator.is_empty()
    }
}

#[derive(Serialize, Default)]
struct OutJournal {
    #[serde(skip_serializing_if = "String::is_empty")]
    title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issn: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    volume: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issue: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pages: String,
}

impl OutJournal {
    fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.issn.is_empty()
            && self.volume.is_empty()
            && self.issue.is_empty()
            && self.pages.is_empty()
    }
}

// ── Writer type mappings ──────────────────────────────────────────────────────

fn cm_to_invenio_type(cm: &str) -> &'static str {
    C::cm_to_inveniordm(cm)
}

fn cm_to_invenio_identifier(cm: &str) -> &'static str {
    match cm {
        "Ark" => "ark",
        "arXiv" => "arxiv",
        "Bibcode" => "ads",
        "CrossrefFunderID" => "crossreffunderid",
        "DOI" => "doi",
        "EAN13" => "ean13",
        "EISSN" => "eissn",
        "GRID" => "grid",
        "Handle" => "handle",
        "IGSN" => "igsn",
        "ISBN" => "isbn",
        "ISNI" => "isni",
        "ISSN" => "issn",
        "ISTC" => "istc",
        "LISSN" => "lissn",
        "LSID" => "lsid",
        "PMID" => "pmid",
        "PURL" => "purl",
        "UPC" => "upc",
        "URL" => "url",
        "URN" => "urn",
        "W3ID" => "w3id",
        "GUID" => "guid",
        "UUID" => "uuid",
        "Other" => "other",
        _ => "",
    }
}

fn cm_to_invenio_contributor_role(cm: &str) -> &'static str {
    let r = C::cm_to_inveniordm_role(cm);
    if r == "other" { "" } else { r }
}

fn cm_to_invenio_relation(cm: &str) -> &'static str {
    match cm {
        "IsCitedBy" => "iscitedby",
        "Cites" => "cites",
        "IsSupplementTo" => "issupplementto",
        "IsSupplementedBy" => "issupplementedby",
        "IsContinuedBy" => "iscontinuedby",
        "Continues" => "continues",
        "IsNewVersionOf" => "isnewversionof",
        "IsPreviousVersion" | "IsPreviousVersionOf" => "ispreviousversion",
        "IsPartOf" => "ispartof",
        "HasPart" => "haspart",
        "IsReferencedBy" => "isreferencedby",
        "References" => "references",
        "IsDocumentedBy" => "isdocumentedby",
        "Documents" => "documents",
        "IsCompiledBy" => "iscompiledby",
        "Compiles" => "compiles",
        "IsVariantFormOf" => "isvariantformof",
        "IsOriginalFormOf" => "isoriginalformof",
        "IsIdenticalTo" => "isidenticalto",
        "IsReviewOf" => "reviews",
        "HasReview" => "isreviewedby",
        "IsDerivedFrom" => "isderivedfrom",
        "IsSourceOf" => "issourceof",
        "Describes" => "describes",
        "IsDescribedBy" => "isdescribedby",
        "IsMetadataFor" => "ismetadatafor",
        "HasMetadata" => "hasmetadata",
        "IsAnnotatedBy" => "isannotatedby",
        "Annotates" => "annotates",
        "IsCorrectedBy" => "iscorrectedby",
        "Corrects" => "corrects",
        "IsVersionOf" => "isversionof",
        "HasVersion" => "hasversion",
        "IsTranslationOf" => "istranslationof",
        "IsPreprintOf" => "ispreviousversionof",
        "HasPreprint" => "haspreprint",
        _ => "",
    }
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn convert(data: &Data) -> OutInveniordm {
    use crate::doi_utils::validate_doi;
    use crate::utils::{get_language, validate_id, validate_orcid, validate_ror};

    let mut out = OutInveniordm::default();

    // DOI
    let doi = doi_from_identifiers(data)
        .or_else(|| validate_doi(&data.id))
        .unwrap_or_default();
    let provider = if is_rogue_scholar_doi(&data.id) {
        "crossref"
    } else {
        "external"
    };
    out.pids.doi = OutDoi {
        identifier: doi,
        provider: provider.to_string(),
    };

    // Access
    out.access = OutAccess {
        record: "public".to_string(),
        files: "public".to_string(),
    };
    out.files = OutFiles { enabled: false };

    // Resource type
    out.metadata.resource_type = OutResourceType {
        id: cm_to_invenio_type(&data.type_).to_string(),
    };

    // Title
    out.metadata.title = if !data.title.is_empty() {
        data.title.clone()
    } else {
        "No title".to_string()
    };

    // Publication date
    out.metadata.publication_date = if !data.date_published.is_empty() {
        parse_date(&data.date_published)
    } else if !data.dates.available.is_empty() {
        parse_date(&data.dates.available)
    } else if !data.dates.created.is_empty() {
        parse_date(&data.dates.created)
    } else {
        String::new()
    };

    // Creators (contributors with "Author" role)
    if data
        .contributors
        .iter()
        .any(|c| c.roles.contains(&"Author".to_string()))
    {
        for v in &data.contributors {
            if !v.roles.contains(&"Author".to_string()) {
                continue;
            }
            let mut identifiers = vec![];
            if !v.id().is_empty()
                && let Some(orcid) = validate_orcid(v.id())
            {
                identifiers.push(OutIdentifier {
                    identifier: orcid,
                    scheme: "orcid".to_string(),
                });
            }

            let mut affiliations = vec![];
            for a in v.affiliations() {
                let aff_id = validate_ror(&a.id).unwrap_or_default();
                let aff = OutAffiliation {
                    id: aff_id,
                    name: a.name.clone(),
                };
                let duplicate = affiliations
                    .iter()
                    .any(|e: &OutAffiliation| !e.id.is_empty() && e.id == aff.id);
                if !duplicate {
                    affiliations.push(aff);
                }
            }

            // type_: "Person"→"personal", "Organization"→"organizational"
            let ptype = match v.type_.as_str() {
                "Person" => "personal",
                "Organization" => "organizational",
                _ => "organizational",
            };

            out.metadata.creators.push(OutCreator {
                person_or_org: OutPersonOrOrg {
                    type_: ptype.to_string(),
                    name: if v.type_ == "Organization" { v.name() } else { String::new() },
                    given_name: v.given_name().to_string(),
                    family_name: v.family_name().to_string(),
                    identifiers,
                },
                affiliations,
            });
        }
    } else {
        // Placeholder when no authors
        out.metadata.creators.push(OutCreator {
            person_or_org: OutPersonOrOrg {
                type_: "organizational".to_string(),
                name: "No author".to_string(),
                ..Default::default()
            },
            affiliations: vec![],
        });
    }

    // Contributors (non-Author roles)
    for v in &data.contributors {
        for role in &v.roles {
            if role == "Author" {
                continue;
            }
            let role_id = cm_to_invenio_contributor_role(role);
            if role_id.is_empty() {
                continue;
            }

            let mut identifiers = vec![];
            if !v.id().is_empty()
                && let Some(orcid) = validate_orcid(v.id())
            {
                identifiers.push(OutIdentifier {
                    identifier: orcid,
                    scheme: "orcid".to_string(),
                });
            }

            let mut affiliations = vec![];
            if v.type_ == "Person" {
                for a in v.affiliations() {
                    let aff_id = validate_ror(&a.id).unwrap_or_default();
                    affiliations.push(OutAffiliation {
                        id: aff_id,
                        name: a.name.clone(),
                    });
                }
            }

            let ptype = match v.type_.as_str() {
                "Person" => "personal",
                "Organization" => "organizational",
                _ => "organizational",
            };

            out.metadata.contributors.push(OutContributor {
                person_or_org: OutPersonOrOrg {
                    type_: ptype.to_string(),
                    name: if v.type_ == "Organization" { v.name() } else { String::new() },
                    given_name: v.given_name().to_string(),
                    family_name: v.family_name().to_string(),
                    identifiers,
                },
                role: OutTypeId {
                    id: role_id.to_string(),
                },
                affiliations,
            });
            break; // use first non-Author role only
        }
    }

    // Publisher
    out.metadata.publisher = data.publisher.name.clone();

    // Container → custom_fields.journal:journal
    // Only include journal title when container type is Journal, Periodical, or Blog
    let container_type = data.container.type_.as_str();
    if !data.container.title.is_empty()
        && matches!(container_type, "Journal" | "Periodical" | "Blog")
    {
        out.custom_fields.journal.title = data.container.title.clone();
    }
    if !data.container.platform.is_empty() {
        out.custom_fields.generator = data.container.platform.clone();
    }
    if !data.container.volume.is_empty() {
        out.custom_fields.journal.volume = data.container.volume.clone();
    }
    if !data.container.issue.is_empty() {
        out.custom_fields.journal.issue = data.container.issue.clone();
    }
    if !data.container.first_page.is_empty() {
        out.custom_fields.journal.pages = container_pages(&data.container);
    }
    if !data.container.identifier.is_empty() && data.container.identifier_type == "ISSN" {
        out.custom_fields.journal.issn = data.container.identifier.clone();
    }

    // Optional custom fields
    out.custom_fields.content_html = data.content.clone();
    out.custom_fields.feature_image = data.image.clone();

    // Identifiers: skip the primary DOI, add URL separately
    for v in &data.identifiers {
        let scheme = cm_to_invenio_identifier(&v.identifier_type);
        if scheme.is_empty() {
            continue;
        }
        // skip the record's own DOI
        if v.identifier_type == "DOI"
            && normalize_id_for_doi(&v.identifier) == normalize_id_for_doi(&data.id)
        {
            continue;
        }
        out.metadata.identifiers.push(OutIdentifier {
            identifier: v.identifier.clone(),
            scheme: scheme.to_string(),
        });
    }
    // Add URL as identifier
    if !data.url.is_empty() {
        out.metadata.identifiers.push(OutIdentifier {
            identifier: data.url.clone(),
            scheme: "url".to_string(),
        });
    }

    // Dates: iterate over Date fields
    let date_fields: &[(&str, &str)] = &[
        ("created", &data.dates.created),
        ("submitted", &data.dates.submitted),
        ("accepted", &data.dates.accepted),
        ("issued", &data.date_published), // "published" → "issued"
        ("updated", &data.date_updated),
        ("other", &data.dates.accessed), // "accessed" → "other"
        ("available", &data.dates.available),
        ("copyrighted", &data.dates.copyrighted),
        ("collected", &data.dates.collected),
        ("valid", &data.dates.valid),
        ("withdrawn", &data.dates.withdrawn),
        ("other", &data.dates.other),
    ];
    for (id, date) in date_fields {
        if !date.is_empty() {
            out.metadata.dates.push(OutDate {
                date: date.to_string(),
                type_: OutTypeId { id: id.to_string() },
            });
        }
    }

    // Description
    if !data.description.is_empty() {
        out.metadata.description = data.description.clone();
    }

    // Funding references
    for v in &data.funding_references {
        let ror_id = if v.funder_identifier_type == "Crossref Funder ID" {
            // Crossref Funder IDs are not ROR IDs; no conversion available
            String::new()
        } else {
            let (validated_id, funder_id_type) = validate_id(&v.funder_id);
            if funder_id_type == "ROR" {
                validate_ror(&validated_id).unwrap_or_default()
            } else {
                String::new()
            }
        };

        let funder = OutFunder {
            id: ror_id,
            name: v.funder_name.clone(),
        };

        let award =
            if !v.award_number.is_empty() || !v.award_title.is_empty() || !v.award_id.is_empty() {
                let mut identifiers = vec![];
                if !v.award_id.is_empty() {
                    let (award_id_val, award_id_type) = validate_id(&v.award_id);
                    let scheme = cm_to_invenio_identifier(award_id_type);
                    if !award_id_val.is_empty() && !scheme.is_empty() {
                        identifiers.push(OutIdentifier {
                            identifier: award_id_val,
                            scheme: scheme.to_string(),
                        });
                    }
                }
                OutAward {
                    number: v.award_number.clone(),
                    title: OutAwardTitle {
                        en: v.award_title.clone(),
                    },
                    identifiers,
                }
            } else {
                OutAward::default()
            };

        out.metadata.funding.push(OutFunding { funder, award });
    }

    // Language
    if !data.language.is_empty() {
        let lang3 = get_language(&data.language, "iso639-3");
        if !lang3.is_empty() {
            out.metadata.languages.push(OutLanguage { id: lang3 });
        }
    }

    // Subjects
    for v in &data.subjects {
        out.metadata.subjects.push(OutSubject {
            subject: v.subject.clone(),
            ..Default::default()
        });
    }

    // License
    let right_id = if !data.license.id.is_empty() {
        data.license.id.to_lowercase()
    } else if !data.license.url.is_empty() {
        crate::spdx::from_url(&data.license.url).id.to_lowercase()
    } else {
        String::new()
    };
    if !right_id.is_empty() {
        out.metadata.rights.push(OutRight { id: right_id });
    }

    // References
    for v in &data.references {
        let (ref_id, ref_id_type) = validate_id(&v.id);
        let scheme = cm_to_invenio_identifier(ref_id_type).to_string();
        let unstructured = if v.unstructured.is_empty() {
            // Build from reference + year
            let mut u = if !v.reference.is_empty() {
                v.reference.clone()
            } else {
                "Unknown title".to_string()
            };
            if !v.publication_year.is_empty() {
                u.push_str(&format!(" ({}).", v.publication_year));
            }
            u
        } else {
            let mut u = v.unstructured.clone();
            // Remove duplicate ID from unstructured text
            if !v.id.is_empty() {
                u = u.replace(&v.id, "");
            }
            u.trim_end().to_string()
        };
        out.metadata.references.push(OutReference {
            reference: unstructured,
            scheme,
            identifier: ref_id,
        });
    }

    // Relations (exclude IsPartOf — captured in container/communities)
    for v in &data.relations {
        if v.type_ == "IsPartOf" {
            continue;
        }
        let (rel_id, id_type) = validate_id(&v.id);
        let scheme = cm_to_invenio_identifier(id_type);
        let relation_type = cm_to_invenio_relation(&v.type_);
        if !rel_id.is_empty() && !scheme.is_empty() && !relation_type.is_empty() {
            out.metadata.related_identifiers.push(OutRelatedIdentifier {
                identifier: rel_id,
                scheme: scheme.to_string(),
                relation_type: OutTypeId {
                    id: relation_type.to_string(),
                },
            });
        }
    }

    // Version
    out.metadata.version = data.version.clone();

    out
}

fn parse_date(d: &str) -> String {
    // Return up to the date portion (first 10 chars if ISO 8601)
    if d.len() >= 10 {
        d[..10].to_string()
    } else {
        d.to_string()
    }
}

fn container_pages(c: &crate::data::Container) -> String {
    if !c.first_page.is_empty() && !c.last_page.is_empty() {
        format!("{}-{}", c.first_page, c.last_page)
    } else {
        c.first_page.clone()
    }
}

fn normalize_id_for_doi(id: &str) -> String {
    // Strip https://doi.org/ prefix for comparison
    id.trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .to_lowercase()
}

fn doi_from_identifiers(data: &Data) -> Option<String> {
    data.identifiers
        .iter()
        .find(|id| id.identifier_type == "DOI" && !id.identifier.is_empty())
        .and_then(|id| crate::doi_utils::validate_doi(&id.identifier))
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let content: Content = serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_content(content))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let payload = convert(data);
    serde_json::to_vec(&payload).map_err(|e| Error::Parse(e.to_string()))
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let payloads: Vec<OutInveniordm> = list.iter().map(convert).collect();
    serde_json::to_vec_pretty(&payloads).map_err(|e| Error::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_json_maps_journal_container_details() {
        let json = r#"{
                    "doi": "10.5555/example",
                    "parent": {},
                    "pids": {},
                    "metadata": {
                        "resource_type": {"id": "publication-article"},
                        "title": "Example"
                    },
                    "custom_fields": {
                        "journal:journal": {
                            "title": "Journal of Examples",
                            "issn": "1234-5678",
                            "volume": "12",
                            "issue": "3",
                            "pages": "100-110"
                        }
                    }
                }"#;

        let data = read_json(json).unwrap();
        assert_eq!(data.container.title, "Journal of Examples");
        assert_eq!(data.container.identifier, "1234-5678");
        assert_eq!(data.container.identifier_type, "ISSN");
        assert_eq!(data.container.volume, "12");
        assert_eq!(data.container.issue, "3");
        assert_eq!(data.container.first_page, "100");
        assert_eq!(data.container.last_page, "110");
    }

    #[test]
    fn test_write_prefers_doi_identifier_over_id() {
        let data = Data {
            id: "https://example.org/not-a-doi".to_string(),
            identifiers: vec![Identifier {
                identifier: "https://doi.org/10.5555/identifier-doi".to_string(),
                identifier_type: "DOI".to_string(),
            }],
            title: "Example".to_string(),
            ..Data::default()
        };

        let out = write(&data).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["pids"]["doi"]["identifier"], "10.5555/identifier-doi");
    }
}

/// Fetch an InvenioRDM record by URL (e.g. `https://rogue-scholar.org/records/7zrtf-jkc81`).
pub fn fetch(url: &str) -> Result<Data> {
    let parsed = url::Url::parse(url).map_err(|e| Error::Parse(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Parse("missing host in URL".to_string()))?;
    let record_id = parsed
        .path_segments()
        .and_then(|mut segs| segs.find(|s| !s.is_empty() && *s != "records" && *s != "api"))
        .ok_or_else(|| Error::Parse("cannot extract record ID from URL".to_string()))?
        .to_string();

    let api_url = format!("https://{}/api/records/{}", host, record_id);
    let client = build_client()?;
    let json = client
        .get(&api_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;
    read_json(&json)
}

// ── Push / registration (create, update, publish) ─────────────────────────────
//
// This is a deliberately scoped port of Go's `inveniordm.UpsertAll`: it
// creates-or-updates a draft record and publishes it. It does not implement
// community auto-association (subject/blog community lookup) or the Rogue
// Scholar legacy-record callback, both of which depend on Go-only
// infrastructure (embedded vocabulary files, the `roguescholar` package)
// that has no equivalent in commonmeta-rs.

/// The outcome of pushing a single record to InvenioRDM.
#[derive(Debug, Default, Clone, Serialize)]
pub struct PushResult {
    /// The commonmeta `Data.id` (typically a DOI URL).
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub doi: String,
    /// The InvenioRDM record ID, once known.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub record_id: String,
    /// "published", "draft", or a "failed_*" status.
    pub status: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub updated: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn build_client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))
}

/// Search InvenioRDM for an existing record by DOI. Returns the record ID if found.
fn search_by_doi(
    doi: &str,
    host: &str,
    client: &reqwest::blocking::Client,
) -> Result<Option<String>> {
    let escaped = crate::doi_utils::escape_doi(doi);
    let url = format!("https://{}/api/records?q=doi:{}", host, escaped);
    let body: Value = client
        .get(&url)
        .header("Content-Type", "application/json")
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .json()
        .map_err(|e| Error::Http(e.to_string()))?;

    let total = body
        .get("hits")
        .and_then(|h| h.get("total"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if total == 0 {
        return Ok(None);
    }
    Ok(body
        .get("hits")
        .and_then(|h| h.get("hits"))
        .and_then(|hits| hits.get(0))
        .and_then(|first| first.get("id"))
        .and_then(Value::as_str)
        .map(|s| s.to_string()))
}

fn create_draft_record(
    body: &[u8],
    host: &str,
    token: &str,
    client: &reqwest::blocking::Client,
) -> Result<(String, String, String)> {
    let url = format!("https://{}/api/records", host);
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(body.to_vec())
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;

    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| Error::Http(e.to_string()))?;
    if status == 429 {
        return Err(Error::Http("rate limited".to_string()));
    }
    if status != 201 {
        return Err(Error::Http(format!(
            "failed to create draft record: {}",
            text
        )));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
    Ok((
        v.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        v.get("created")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        v.get("updated")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    ))
}

fn edit_published_record(
    record_id: &str,
    host: &str,
    token: &str,
    client: &reqwest::blocking::Client,
) -> Result<()> {
    let url = format!("https://{}/api/records/{}/draft", host, record_id);
    client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;
    Ok(())
}

fn update_draft_record(
    record_id: &str,
    body: &[u8],
    host: &str,
    token: &str,
    client: &reqwest::blocking::Client,
) -> Result<()> {
    let url = format!("https://{}/api/records/{}/draft", host, record_id);
    client
        .put(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .body(body.to_vec())
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;
    Ok(())
}

fn publish_draft_record(
    record_id: &str,
    host: &str,
    token: &str,
    client: &reqwest::blocking::Client,
) -> Result<(String, String)> {
    let url = format!(
        "https://{}/api/records/{}/draft/actions/publish",
        host, record_id
    );
    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .map_err(|e| Error::Http(e.to_string()))?;

    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| Error::Http(e.to_string()))?;
    if status != 202 {
        return Err(Error::Http(format!(
            "failed to publish draft record: {}",
            text
        )));
    }
    let v: Value = serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
    Ok((
        v.get("created")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        v.get("updated")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    ))
}

/// Create-or-update, then publish, a single record in InvenioRDM.
///
/// If a record with the same DOI already exists (checked via the InvenioRDM
/// search API), its published version is reopened as a draft and updated;
/// otherwise a new draft is created. Either way, the draft is published
/// before returning.
pub fn upsert(data: &Data, host: &str, token: &str) -> PushResult {
    let mut result = PushResult {
        id: data.id.clone(),
        ..Default::default()
    };

    let doi = match crate::doi_utils::validate_doi(&data.id) {
        Some(d) => d,
        None => {
            result.status = "failed_missing_doi".to_string();
            return result;
        }
    };
    result.doi = doi.clone();

    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            result.status = "failed".to_string();
            result.message = Some(e.to_string());
            return result;
        }
    };

    let body = match write(data) {
        Ok(b) => b,
        Err(e) => {
            result.status = "failed".to_string();
            result.message = Some(e.to_string());
            return result;
        }
    };

    let existing = match search_by_doi(&doi, host, &client) {
        Ok(id) => id,
        Err(e) => {
            result.status = "failed_search".to_string();
            result.message = Some(e.to_string());
            return result;
        }
    };

    let record_id = match existing {
        None => match create_draft_record(&body, host, token, &client) {
            Ok((id, created, updated)) => {
                result.created = created;
                result.updated = updated;
                id
            }
            Err(e) => {
                result.status = "failed_create_draft".to_string();
                result.message = Some(e.to_string());
                return result;
            }
        },
        Some(id) => {
            if let Err(e) = edit_published_record(&id, host, token, &client) {
                result.status = "failed_edit_published".to_string();
                result.message = Some(e.to_string());
                return result;
            }
            if let Err(e) = update_draft_record(&id, &body, host, token, &client) {
                result.status = "failed_update_draft".to_string();
                result.message = Some(e.to_string());
                return result;
            }
            id
        }
    };
    result.record_id = record_id.clone();

    match publish_draft_record(&record_id, host, token, &client) {
        Ok((created, updated)) => {
            if !created.is_empty() {
                result.created = created;
            }
            result.updated = updated;
            result.status = "published".to_string();
        }
        Err(e) => {
            result.status = "failed_publish".to_string();
            result.message = Some(e.to_string());
        }
    }

    result
}

/// Create-or-update, then publish, a list of records in InvenioRDM.
pub fn upsert_all(list: &[Data], host: &str, token: &str) -> Vec<PushResult> {
    list.iter().map(|data| upsert(data, host, token)).collect()
}

#[cfg(test)]
mod push_tests {
    use super::*;

    #[test]
    fn test_upsert_rejects_missing_doi() {
        let data = Data {
            id: "https://example.com/not-a-doi".to_string(),
            ..Data::default()
        };
        let result = upsert(&data, "example.invenio.host", "fake-token");
        assert_eq!(result.status, "failed_missing_doi");
        assert!(result.record_id.is_empty());
    }

    #[test]
    fn test_upsert_rejects_empty_id() {
        let data = Data::default();
        let result = upsert(&data, "example.invenio.host", "fake-token");
        assert_eq!(result.status, "failed_missing_doi");
    }

    #[test]
    fn test_upsert_all_empty_list() {
        let results = upsert_all(&[], "example.invenio.host", "fake-token");
        assert!(results.is_empty());
    }

    #[test]
    fn test_push_result_serialization_omits_empty_fields() {
        let result = PushResult {
            id: "https://doi.org/10.1/a".to_string(),
            status: "failed_missing_doi".to_string(),
            ..Default::default()
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"id\""));
        assert!(json.contains("\"status\""));
        assert!(!json.contains("\"doi\""));
        assert!(!json.contains("\"record_id\""));
        assert!(!json.contains("\"message\""));
    }
}
