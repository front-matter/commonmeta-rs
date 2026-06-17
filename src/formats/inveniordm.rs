use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::{
    Affiliation, Container, Contributor, Data, Description, File, FundingReference, Identifier,
    License, Publisher, Reference, Relation, Subject, Title,
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
    parent: Parent,
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
    #[allow(dead_code)]
    volume: String,
    #[serde(default)]
    #[allow(dead_code)]
    issue: String,
    #[serde(default)]
    #[allow(dead_code)]
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
    match id {
        "annotationcollection" => "Collection",
        "book" => "Book",
        "conferencepaper" => "ProceedingsArticle",
        "datamanagementplan" => "OutputManagementPlan",
        "dataset" => "Dataset",
        "drawing" | "figure" | "image" | "photo" | "plot" => "Image",
        "lesson" => "InteractiveResource",
        "patent" => "Patent",
        "peerreview" => "PeerReview",
        "physicalobject" => "PhysicalObject",
        "poster" => "Presentation",
        "presentation" => "Presentation",
        "preprint" => "Article",
        "publication" => "JournalArticle",
        "publication-annotationcollection" => "Collection",
        "publication-article" => "JournalArticle",
        "publication-blogpost" => "BlogPost",
        "publication-book" => "Book",
        "publication-conferencepaper" => "ProceedingsArticle",
        "publication-conferenceproceeding" => "Proceedings",
        "publication-datamanagementplan" => "OutputManagementPlan",
        "publication-datapaper" => "JournalArticle",
        "publication-dissertation" => "Dissertation",
        "publication-journal" => "Journal",
        "publication-other" => "Other",
        "publication-patent" => "Patent",
        "publication-peerreview" => "PeerReview",
        "publication-preprint" => "Article",
        "publication-report" => "Report",
        "publication-section" => "BookChapter",
        "publication-standard" => "Standard",
        "publication-technicalnote" => "Report",
        "publication-thesis" => "Dissertation",
        "publication-workingpaper" => "Report",
        "report" => "Report",
        "section" => "BookChapter",
        "software" => "Software",
        "software-computationalnotebook" => "ComputationalNotebook",
        "softwaredocumentation" => "Software",
        "taxonomictreatment" => "Collection",
        "technicalnote" => "Report",
        "thesis" => "Dissertation",
        "video" => "Audiovisual",
        "workflow" => "Workflow",
        "workingpaper" => "Report",
        "other" => "Other",
        _ => "",
    }
}

fn license_mapping(id: &str) -> &'static str {
    match id {
        "cc-by-3.0" => "CC-BY-3.0",
        "cc-by-4.0" => "CC-BY-4.0",
        "cc-by-nc-3.0" => "CC-BY-NC-3.0",
        "cc-by-nc-4.0" => "CC-BY-NC-4.0",
        "cc-by-nc-nd-3.0" => "CC-BY-NC-ND-3.0",
        "cc-by-nc-nd-4.0" => "CC-BY-NC-ND-4.0",
        "cc-by-nc-sa-3.0" => "CC-BY-NC-SA-3.0",
        "cc-by-nc-sa-4.0" => "CC-BY-NC-SA-4.0",
        "cc-by-nd-3.0" => "CC-BY-ND-3.0",
        "cc-by-nd-4.0" => "CC-BY-ND-4.0",
        "cc-by-sa-3.0" => "CC-BY-SA-3.0",
        "cc-by-sa-4.0" => "CC-BY-SA-4.0",
        "cc0-1.0" => "CC0-1.0",
        "mit" => "MIT",
        "apache-2.0" => "Apache-2.0",
        "gpl-3.0" => "GPL-3.0",
        _ => "",
    }
}

/// Valid Commonmeta relation types (from Python COMMONMETA_RELATION_TYPES).
fn is_valid_relation_type(t: &str) -> bool {
    matches!(
        t,
        "IsNewVersionOf"
            | "IsPreviousVersionOf"
            | "IsVersionOf"
            | "HasVersion"
            | "IsPartOf"
            | "HasPart"
            | "IsVariantFormOf"
            | "IsOriginalFormOf"
            | "IsIdenticalTo"
            | "IsTranslationOf"
            | "HasReview"
            | "IsReviewOf"
            | "IsPreprintOf"
            | "HasPreprint"
            | "IsSupplementTo"
    )
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

    let mut type_ = match v.person_or_org.type_.as_str() {
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
                type_ = "Person".to_string();
                break;
            }
            "ror" | "ROR" => {
                id = normalize_ror(&ni.identifier);
                type_ = "Organization".to_string();
                break;
            }
            _ => {}
        }
    }

    let name = v.person_or_org.name.clone();
    let mut given_name = v.person_or_org.given_name.clone();
    let mut family_name = v.person_or_org.family_name.clone();

    if type_.is_empty() {
        type_ = if !given_name.is_empty() || !family_name.is_empty() {
            "Person".to_string()
        } else {
            "Organization".to_string()
        };
    }

    // Split "Family, Given" for Person type when only name is provided
    let mut name_out = name.clone();
    if type_ == "Person"
        && !name_out.is_empty()
        && given_name.is_empty()
        && family_name.is_empty()
    {
        if let Some(comma) = name_out.find(',') {
            given_name = name_out[comma + 1..].trim().to_string();
            family_name = name_out[..comma].trim().to_string();
            name_out = String::new();
        }
    }

    let affiliations = v
        .affiliations
        .iter()
        .filter_map(|a| {
            let aff_id = normalize_ror(&a.id);
            if aff_id.is_empty() && a.name.is_empty() {
                None
            } else {
                Some(Affiliation {
                    id: aff_id,
                    name: a.name.clone(),
                    ..Default::default()
                })
            }
        })
        .collect();

    Contributor {
        id,
        type_,
        name: name_out,
        given_name,
        family_name,
        affiliations,
        contributor_roles: vec![default_role.to_string()],
    }
}

fn get_zenodo_contributor(v: &Creator, default_role: &str) -> Contributor {
    let mut id = String::new();
    let mut type_ = String::new();

    if !v.orcid.is_empty() {
        id = normalize_orcid(&v.orcid);
        type_ = "Person".to_string();
    }

    let (given_name, family_name, name) = parse_name(&v.name);

    if type_.is_empty() {
        type_ = if !given_name.is_empty() || !family_name.is_empty() {
            "Person".to_string()
        } else {
            "Organization".to_string()
        };
    }

    let mut family_name_out = family_name;
    let mut name_out = name;
    if type_ == "Person" && family_name_out.is_empty() && !name_out.is_empty() {
        family_name_out = name_out.clone();
        name_out = String::new();
    }

    let affiliations = if !v.affiliation.is_empty() {
        vec![Affiliation {
            name: v.affiliation.clone(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

    Contributor {
        id,
        type_,
        name: name_out,
        given_name,
        family_name: family_name_out,
        affiliations,
        contributor_roles: vec![default_role.to_string()],
    }
}

fn parse_name(name: &str) -> (String, String, String) {
    if let Some(comma) = name.find(',') {
        let family = name[..comma].trim().to_string();
        let given = name[comma + 1..].trim().to_string();
        (given, family, String::new())
    } else {
        (String::new(), String::new(), name.to_string())
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
    let mut data = Data::default();

    // ID
    data.id = if !content.doi.is_empty() {
        normalize_doi(&content.doi)
    } else {
        normalize_doi(&content.pids.doi.identifier)
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
        data.container = Container {
            type_: "Blog".to_string(),
            title: content.custom_fields.journal.title.clone(),
            identifier,
            identifier_type,
            platform: content.custom_fields.generator.clone(),
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
        data.container = Container {
            type_: "Periodical".to_string(),
            title: content.custom_fields.journal.title.clone(),
            identifier,
            identifier_type,
            platform: content.custom_fields.generator.clone(),
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
            .any(|e| !e.id.is_empty() && e.id == contributor.id);
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
            .any(|e| !e.id.is_empty() && e.id == contributor.id);
        if !already {
            data.contributors.push(contributor);
        }
    }

    // Dates: Python handles "issued" → published and "updated" → updated only
    for d in &content.metadata.dates {
        let t = date_type_str(&d.type_);
        match t.as_str() {
            "issued" => data.date.published = d.date.clone(),
            "updated" => data.date.updated = d.date.clone(),
            _ => {}
        }
    }
    if data.date.published.is_empty() && !content.metadata.publication_date.is_empty() {
        data.date.published = content.metadata.publication_date.clone();
    }
    // Fallback for updated: top-level meta.updated (strip milliseconds)
    if data.date.updated.is_empty() && !content.updated.is_empty() {
        data.date.updated = strip_milliseconds(&content.updated);
    }

    // Descriptions: description (Abstract) + notes (Other)
    for (i, text) in [&content.metadata.description, &content.metadata.notes]
        .iter()
        .enumerate()
    {
        if !text.is_empty() {
            data.descriptions.push(Description {
                description: sanitize(text),
                type_: if i == 0 {
                    "Abstract".to_string()
                } else {
                    "Other".to_string()
                },
                ..Default::default()
            });
        }
    }

    // Feature image
    if !content.custom_fields.feature_image.is_empty() {
        data.feature_image = content.custom_fields.feature_image.clone();
    }

    // Files from top-level `files` list
    if let Some(files_val) = &content.files {
        if let Ok(files_enabled) = serde_json::from_value::<FilesEnabled>(files_val.clone()) {
            if files_enabled.enabled {
                if let Ok(entries) = serde_json::from_value::<FilesWithEntries>(files_val.clone())
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
                funder_identifier,
                funder_identifier_type,
                funder_name: v.funder.name.clone(),
                award_number,
                award_title,
                award_uri,
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
                funder_identifier,
                funder_identifier_type,
                funder_name: v.funder.name.clone(),
                award_number: v.code.clone(),
                award_title: v.title.clone(),
                award_uri,
            });
        }
    }

    // Identifiers: DOI first, then only doi/uuid/guid schemes from metadata
    data.identifiers.push(Identifier {
        identifier: data.id.clone(),
        identifier_type: "DOI".to_string(),
    });
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
    if let Some(id_val) = &content.id {
        if let Some(s) = id_val.as_str() {
            if !s.is_empty() {
                data.identifiers.push(Identifier {
                    identifier: s.to_string(),
                    identifier_type: "RID".to_string(),
                });
            }
        }
    }

    // Language: metadata.language first, then metadata.languages[0].id
    if !content.metadata.language.is_empty() {
        data.language = get_language(&content.metadata.language, "iso639-1");
    } else if !content.metadata.languages.is_empty() {
        data.language = get_language(&content.metadata.languages[0].id, "iso639-1");
    }

    // License from rights list (new) or legacy license field
    if !content.metadata.rights.is_empty() {
        let r = &content.metadata.rights[0];
        let license_id = license_mapping(&r.id).to_string();
        data.license = License {
            id: license_id,
            url: String::new(),
        };
    } else if let Some(lic) = &content.metadata.license {
        if !lic.id.is_empty() {
            data.license = License {
                id: license_mapping(&lic.id).to_string(),
                url: String::new(),
            };
        }
    }

    // Provider
    data.provider = if is_rogue_scholar {
        "Crossref".to_string()
    } else {
        "DataCite".to_string()
    };

    // Subjects: metadata.subjects + metadata.keywords merged
    for v in &content.metadata.subjects {
        let s = subject_string(v);
        if s.is_empty() {
            continue;
        }
        let subj = Subject { subject: s };
        if !data.subjects.contains(&subj) {
            data.subjects.push(subj);
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
        let subj = Subject { subject: s };
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

    // Citations from custom_fields.rs:citations
    for v in &content.custom_fields.citations {
        let id = normalize_reference_id(&v.scheme, &v.identifier);
        data.references.push(Reference {
            id,
            unstructured: v.reference.clone(),
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
            let rel = Relation {
                id,
                type_,
            };
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
        data.titles.push(Title {
            title: sanitize(&content.metadata.title),
            ..Default::default()
        });
    }

    // Version
    data.version = content.metadata.version.clone();

    // Full-text HTML
    if !content.custom_fields.content_html.is_empty() {
        data.content_html = content.custom_fields.content_html.clone();
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
    #[serde(rename = "custom_fields", skip_serializing_if = "OutCustomFields::is_empty")]
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
    #[serde(rename = "journal:journal", skip_serializing_if = "OutJournal::is_empty")]
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
    match cm {
        "Article" => "publication-preprint",
        "Audiovisual" => "video",
        "BlogPost" => "publication-blogpost",
        "Book" => "publication-book",
        "BookChapter" => "publication-section",
        "Collection" => "publication-annotationcollection",
        "ComputationalNotebook" => "software-computationalnotebook",
        "Dataset" => "dataset",
        "Dissertation" => "publication-thesis",
        "Document" => "publication",
        "Entry" => "publication",
        "Event" => "event",
        "Figure" => "image-figure",
        "Image" => "image",
        "Instrument" => "other",
        "Journal" => "publication-journal",
        "JournalArticle" => "publication-article",
        "LegalDocument" => "publication",
        "Manuscript" => "publication",
        "Map" => "other",
        "Patent" => "patent",
        "PersonalCommunication" => "publication",
        "PhysicalObject" => "physicalobject",
        "Post" => "publication",
        "Poster" => "poster",
        "Presentation" => "presentation",
        "ProceedingsArticle" => "publication-conferencepaper",
        "Proceedings" => "publication-conferenceproceeding",
        "Report" => "publication-report",
        "Review" => "publication-peerreview",
        "Software" => "software",
        "Sound" => "audio",
        "Standard" => "publication-standard",
        "WebPage" => "publication",
        "Workflow" => "workflow",
        "Other" => "other",
        _ => "other",
    }
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
    match cm {
        "Editor" => "editor",
        "Supervisor" => "supervisor",
        "DataManager" => "datamanager",
        "DataCollector" => "datacollector",
        "DataCurator" => "datacurator",
        "Distributor" => "distributor",
        "Funder" => "funder",
        "HostingInstitution" => "hostinginstitution",
        "Producer" => "producer",
        "ProjectLeader" => "projectleader",
        "ProjectManager" => "projectmanager",
        "ProjectMember" => "projectmember",
        "RelatedPerson" => "relatedperson",
        "Researcher" => "researcher",
        "RightsHolder" => "rightsholder",
        "Sponsor" => "sponsor",
        "WorkPackageLeader" => "workpackageleader",
        "ContactPerson" => "contactperson",
        _ => "",
    }
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
    use crate::utils::{get_language, url_to_spdx, validate_id, validate_orcid, validate_ror};

    let mut out = OutInveniordm::default();

    // DOI
    let doi = validate_doi(&data.id).unwrap_or_default();
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
    out.metadata.title = data
        .titles
        .first()
        .map(|t| t.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "No title".to_string());

    // Publication date
    out.metadata.publication_date = if !data.date.published.is_empty() {
        parse_date(&data.date.published)
    } else if !data.date.available.is_empty() {
        parse_date(&data.date.available)
    } else if !data.date.created.is_empty() {
        parse_date(&data.date.created)
    } else {
        String::new()
    };

    // Creators (contributors with "Author" role)
    if data.contributors.iter().any(|c| c.contributor_roles.contains(&"Author".to_string())) {
        for v in &data.contributors {
            if !v.contributor_roles.contains(&"Author".to_string()) {
                continue;
            }
            let mut identifiers = vec![];
            if !v.id.is_empty() {
                if let Some(orcid) = validate_orcid(&v.id) {
                    identifiers.push(OutIdentifier {
                        identifier: orcid,
                        scheme: "orcid".to_string(),
                    });
                }
            }

            let mut affiliations = vec![];
            for a in &v.affiliations {
                let aff_id = validate_ror(&a.id)
                    .unwrap_or_default();
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
                    name: v.name.clone(),
                    given_name: v.given_name.clone(),
                    family_name: v.family_name.clone(),
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
        for role in &v.contributor_roles {
            if role == "Author" {
                continue;
            }
            let role_id = cm_to_invenio_contributor_role(role);
            if role_id.is_empty() {
                continue;
            }

            let mut identifiers = vec![];
            if !v.id.is_empty() {
                if let Some(orcid) = validate_orcid(&v.id) {
                    identifiers.push(OutIdentifier {
                        identifier: orcid,
                        scheme: "orcid".to_string(),
                    });
                }
            }

            let mut affiliations = vec![];
            if v.type_ == "Person" {
                for a in &v.affiliations {
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
                    name: v.name.clone(),
                    given_name: v.given_name.clone(),
                    family_name: v.family_name.clone(),
                    identifiers,
                },
                role: OutTypeId { id: role_id.to_string() },
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
    out.custom_fields.content_html = data.content_html.clone();
    out.custom_fields.feature_image = data.feature_image.clone();

    // Identifiers: skip the primary DOI, add URL separately
    for v in &data.identifiers {
        let scheme = cm_to_invenio_identifier(&v.identifier_type);
        if scheme.is_empty() {
            continue;
        }
        // skip the record's own DOI
        if v.identifier_type == "DOI" && normalize_id_for_doi(&v.identifier) == normalize_id_for_doi(&data.id) {
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
        ("created",     &data.date.created),
        ("submitted",   &data.date.submitted),
        ("accepted",    &data.date.accepted),
        ("issued",      &data.date.published),   // "published" → "issued"
        ("updated",     &data.date.updated),
        ("other",       &data.date.accessed),    // "accessed" → "other"
        ("available",   &data.date.available),
        ("copyrighted", &data.date.copyrighted),
        ("collected",   &data.date.collected),
        ("valid",       &data.date.valid),
        ("withdrawn",   &data.date.withdrawn),
        ("other",       &data.date.other),
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
    if let Some(d) = data.descriptions.first() {
        out.metadata.description = d.description.clone();
    }

    // Funding references
    for v in &data.funding_references {
        let ror_id = if v.funder_identifier_type == "Crossref Funder ID" {
            // Crossref Funder IDs are not ROR IDs; no conversion available
            String::new()
        } else {
            let (funder_id, funder_id_type) = validate_id(&v.funder_identifier);
            if funder_id_type == "ROR" {
                validate_ror(&funder_id).unwrap_or_default()
            } else {
                String::new()
            }
        };

        let funder = OutFunder {
            id: ror_id,
            name: v.funder_name.clone(),
        };

        let award = if !v.award_number.is_empty() || !v.award_title.is_empty() || !v.award_uri.is_empty() {
            let mut identifiers = vec![];
            if !v.award_uri.is_empty() {
                let (award_id, award_id_type) = validate_id(&v.award_uri);
                let scheme = cm_to_invenio_identifier(award_id_type);
                if !award_id.is_empty() && !scheme.is_empty() {
                    identifiers.push(OutIdentifier {
                        identifier: award_id,
                        scheme: scheme.to_string(),
                    });
                }
            }
            OutAward {
                number: v.award_number.clone(),
                title: OutAwardTitle { en: v.award_title.clone() },
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
        url_to_spdx(&data.license.url).to_lowercase()
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
            // Build from title + year
            let mut u = if !v.title.is_empty() {
                v.title.clone()
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

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let content: Content =
        serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_content(content))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let payload = convert(data);
    serde_json::to_vec(&payload).map_err(|e| Error::Parse(e.to_string()))
}

/// Fetch an InvenioRDM record by URL (e.g. `https://rogue-scholar.org/records/7zrtf-jkc81`).
pub fn fetch(url: &str) -> Result<Data> {
    let parsed = url::Url::parse(url).map_err(|e| Error::Parse(e.to_string()))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::Parse("missing host in URL".to_string()))?;
    let record_id = parsed
        .path_segments()
        .and_then(|mut segs| {
            segs.find(|s| !s.is_empty() && *s != "records" && *s != "api")
        })
        .ok_or_else(|| Error::Parse("cannot extract record ID from URL".to_string()))?
        .to_string();

    let api_url = format!("https://{}/api/records/{}", host, record_id);
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;
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
