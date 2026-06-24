use chrono::Utc;
use quick_xml::de::from_str as xml_from_str;
use quick_xml::se::Serializer;
use rand::RngExt;
use serde::{Deserialize, Serialize};

use crate::author_utils::normalize_contributor_roles;
use crate::data::{
    Affiliation, Container, Contributor, Data, Description, FundingReference, Identifier, License,
    Organization as DataOrganization, Person as DataPerson, Publisher, Reference, Relation, Subject,
    Title,
};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::{Error, Result};
use crate::utils::{
    community_slug_as_url, dedupe_slice, issn_as_url, normalize_cc_url, normalize_orcid,
    normalize_ror, sanitize, title_case, validate_id,
};

// ── XML output structs ────────────────────────────────────────────────────────
// Field names drive XML element names; `@` prefix = attribute; `$text` = char data.
// Namespace-prefixed elements use `#[serde(rename = "prefix:name")]`.

#[derive(Serialize)]
#[serde(rename = "doi_batch")]
struct DoiBatch {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "@xmlns:ai")]
    xmlns_ai: &'static str,
    #[serde(rename = "@xmlns:rel")]
    xmlns_rel: &'static str,
    #[serde(rename = "@xmlns:fr")]
    xmlns_fr: &'static str,
    #[serde(rename = "@version")]
    version: &'static str,
    head: Head,
    body: Body,
}

#[derive(Serialize)]
struct Head {
    doi_batch_id: String,
    timestamp: String,
    depositor: Depositor,
    registrant: String,
}

#[derive(Serialize)]
struct Depositor {
    depositor_name: String,
    email_address: String,
}

#[derive(Serialize, Default)]
struct Body {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    posted_content: Vec<PostedContent>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    journal: Vec<Journal>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dissertation: Vec<Dissertation>,
}

// ── PostedContent (Article / BlogPost) ───────────────────────────────────────

#[derive(Serialize)]
struct PostedContent {
    #[serde(rename = "@type")]
    type_: String,
    #[serde(rename = "@language", skip_serializing_if = "str::is_empty")]
    language: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    group_title: String,
    #[serde(skip_serializing_if = "Contributors::is_empty")]
    contributors: Contributors,
    titles: Titles,
    posted_date: PostedDate,
    #[serde(skip_serializing_if = "Option::is_none")]
    institution: Option<Institution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_number: Option<ItemNumber>,
    #[serde(rename = "jats:abstract", skip_serializing_if = "Vec::is_empty")]
    abstract_: Vec<JatsAbstract>,
    #[serde(rename = "fr:program", skip_serializing_if = "Option::is_none")]
    funding_program: Option<FrProgram>,
    #[serde(rename = "ai:program", skip_serializing_if = "Option::is_none")]
    license_program: Option<AiProgram>,
    #[serde(rename = "rel:program", skip_serializing_if = "Option::is_none")]
    relations_program: Option<RelProgram>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_info: Option<VersionInfo>,
    doi_data: DoiData,
    #[serde(skip_serializing_if = "Option::is_none")]
    citation_list: Option<CitationList>,
}

// ── Journal (JournalArticle) ──────────────────────────────────────────────────

#[derive(Serialize)]
struct Journal {
    journal_metadata: JournalMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_issue: Option<JournalIssue>,
    journal_article: JournalArticle,
}

#[derive(Serialize)]
struct JournalMetadata {
    #[serde(rename = "@language", skip_serializing_if = "str::is_empty")]
    language: String,
    full_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    issn: Option<Issn>,
}

#[derive(Serialize)]
struct JournalIssue {
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_date: Option<PublicationDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    journal_volume: Option<JournalVolume>,
    #[serde(skip_serializing_if = "str::is_empty")]
    issue: String,
}

#[derive(Serialize)]
struct JournalVolume {
    volume: String,
}

#[derive(Serialize)]
struct JournalArticle {
    #[serde(rename = "@publication_type")]
    publication_type: &'static str,
    #[serde(rename = "@language", skip_serializing_if = "str::is_empty")]
    language: String,
    titles: Titles,
    #[serde(skip_serializing_if = "Contributors::is_empty")]
    contributors: Contributors,
    #[serde(rename = "jats:abstract", skip_serializing_if = "Vec::is_empty")]
    abstract_: Vec<JatsAbstract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    publication_date: Option<PublicationDate>,
    #[serde(rename = "fr:program", skip_serializing_if = "Option::is_none")]
    funding_program: Option<FrProgram>,
    #[serde(rename = "ai:program", skip_serializing_if = "Option::is_none")]
    license_program: Option<AiProgram>,
    #[serde(rename = "rel:program", skip_serializing_if = "Option::is_none")]
    relations_program: Option<RelProgram>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version_info: Option<VersionInfo>,
    doi_data: DoiData,
    #[serde(skip_serializing_if = "Option::is_none")]
    citation_list: Option<CitationList>,
}

// ── Dissertation ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Dissertation {
    #[serde(rename = "@language", skip_serializing_if = "str::is_empty")]
    language: String,
    #[serde(rename = "@publication_type")]
    publication_type: &'static str,
    titles: Titles,
    #[serde(skip_serializing_if = "Option::is_none")]
    approval_date: Option<PublicationDate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    institution: Option<Institution>,
    doi_data: DoiData,
}

// ── Contributors ──────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct Contributors {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    person_name: Vec<PersonName>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    organization: Vec<Organization>,
}

impl Contributors {
    fn is_empty(&self) -> bool {
        self.person_name.is_empty() && self.organization.is_empty()
    }
}

#[derive(Serialize)]
struct PersonName {
    #[serde(rename = "@contributor_role")]
    contributor_role: String,
    #[serde(rename = "@sequence")]
    sequence: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    given_name: String,
    surname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    affiliations: Option<Affiliations>,
    #[serde(rename = "ORCID", skip_serializing_if = "str::is_empty")]
    orcid: String,
}

#[derive(Serialize)]
struct Organization {
    #[serde(rename = "@contributor_role")]
    contributor_role: String,
    #[serde(rename = "@sequence")]
    sequence: &'static str,
    #[serde(rename = "$text")]
    name: String,
}

#[derive(Serialize)]
struct Affiliations {
    institution: Vec<Institution>,
}

// ── Institution ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Institution {
    #[serde(skip_serializing_if = "str::is_empty")]
    institution_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    institution_id: Option<InstitutionId>,
}

#[derive(Serialize)]
struct InstitutionId {
    #[serde(rename = "@type")]
    type_: String,
    #[serde(rename = "$text")]
    text: String,
}

// ── Titles ────────────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct Titles {
    #[serde(skip_serializing_if = "str::is_empty")]
    title: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    subtitle: String,
}

// ── Dates ─────────────────────────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct PostedDate {
    #[serde(rename = "@media_type")]
    media_type: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    month: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    day: String,
    year: String,
}

#[derive(Serialize, Default)]
struct PublicationDate {
    #[serde(rename = "@media_type", skip_serializing_if = "str::is_empty")]
    media_type: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    month: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    day: String,
    year: String,
}

// ── Abstract (JATS) ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct JatsAbstract {
    #[serde(rename = "@xmlns:jats")]
    xmlns_jats: &'static str,
    #[serde(rename = "jats:p")]
    p: Vec<JatsP>,
}

#[derive(Serialize)]
struct JatsP {
    #[serde(rename = "$text")]
    text: String,
}

// ── Programs (namespaced) ─────────────────────────────────────────────────────

// AccessIndicators: ai:program / ai:license_ref
#[derive(Serialize)]
struct AiProgram {
    #[serde(rename = "@name")]
    name: &'static str,
    #[serde(rename = "ai:license_ref", skip_serializing_if = "Vec::is_empty")]
    license_refs: Vec<AiLicenseRef>,
}

#[derive(Serialize)]
struct AiLicenseRef {
    #[serde(rename = "@applies_to")]
    applies_to: &'static str,
    #[serde(rename = "$text")]
    text: String,
}

// FundRef: fr:program / fr:assertion
#[derive(Serialize)]
struct FrProgram {
    #[serde(rename = "@name")]
    name: &'static str,
    #[serde(rename = "fr:assertion", skip_serializing_if = "Vec::is_empty")]
    assertions: Vec<FrAssertion>,
}

#[derive(Serialize)]
struct FrAssertion {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "$text", skip_serializing_if = "str::is_empty")]
    text: String,
    #[serde(rename = "fr:assertion", skip_serializing_if = "Vec::is_empty")]
    nested: Vec<FrAssertion>,
}

// Relations: rel:program / rel:related_item / rel:inter_work_relation / rel:intra_work_relation
#[derive(Serialize)]
struct RelProgram {
    #[serde(rename = "@name")]
    name: &'static str,
    #[serde(rename = "rel:related_item", skip_serializing_if = "Vec::is_empty")]
    related_items: Vec<RelRelatedItem>,
}

#[derive(Serialize)]
struct RelRelatedItem {
    #[serde(
        rename = "rel:inter_work_relation",
        skip_serializing_if = "Option::is_none"
    )]
    inter_work: Option<RelWorkRelation>,
    #[serde(
        rename = "rel:intra_work_relation",
        skip_serializing_if = "Option::is_none"
    )]
    intra_work: Option<RelWorkRelation>,
}

#[derive(Serialize)]
struct RelWorkRelation {
    #[serde(rename = "@relationship-type")]
    relationship_type: String,
    #[serde(rename = "@identifier-type")]
    identifier_type: String,
    #[serde(rename = "$text")]
    text: String,
}

// ── Version info ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct VersionInfo {
    version: String,
}

// ── DOI data ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct DoiData {
    doi: String,
    resource: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    collection: Option<Collection>,
}

#[derive(Serialize)]
struct Collection {
    #[serde(rename = "@property")]
    property: &'static str,
    item: Vec<Item>,
}

#[derive(Serialize)]
struct Item {
    resource: ResourceEl,
}

#[derive(Serialize)]
struct ResourceEl {
    #[serde(rename = "@mime_type")]
    mime_type: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct Issn {
    #[serde(rename = "@media_type")]
    media_type: &'static str,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct ItemNumber {
    #[serde(rename = "@item_number_type")]
    item_number_type: &'static str,
    #[serde(rename = "$text")]
    text: String,
}

// ── Citations ─────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct CitationList {
    citation: Vec<Citation>,
}

#[derive(Serialize)]
struct Citation {
    #[serde(rename = "@key")]
    key: String,
    #[serde(rename = "doi", skip_serializing_if = "str::is_empty")]
    doi: String,
    #[serde(rename = "journal_title", skip_serializing_if = "str::is_empty")]
    journal_title: String,
    #[serde(rename = "author", skip_serializing_if = "str::is_empty")]
    author: String,
    #[serde(rename = "volume", skip_serializing_if = "str::is_empty")]
    volume: String,
    #[serde(rename = "first_page", skip_serializing_if = "str::is_empty")]
    first_page: String,
    #[serde(rename = "cYear", skip_serializing_if = "str::is_empty")]
    c_year: String,
    #[serde(rename = "article_title", skip_serializing_if = "str::is_empty")]
    article_title: String,
    #[serde(
        rename = "unstructured_citation",
        skip_serializing_if = "str::is_empty"
    )]
    unstructured_citation: String,
}

// ── Relation type lists ────────────────────────────────

const INTER_WORK_RELATION_TYPES: &[&str] = &[
    "IsPartOf",
    "HasPart",
    "IsReviewOf",
    "HasReview",
    "IsRelatedMaterial",
    "HasRelatedMaterial",
];

const INTRA_WORK_RELATION_TYPES: &[&str] = &[
    "IsIdenticalTo",
    "IsPreprintOf",
    "HasPreprint",
    "IsTranslationOf",
    "HasTranslation",
    "IsVersionOf",
];

// Allowed contributor roles for Crossref (Python: allowed_roles)
const ALLOWED_CONTRIBUTOR_ROLES: &[&str] = &["Author", "Editor", "Reviewer", "Translator"];

// ── Helpers ───────────────────────────────────────────────────────────────────

fn generate_batch_id() -> String {
    let mut rng = rand::rng();
    let b: [u8; 16] = rng.random();
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
        u16::from_be_bytes([b[4], b[5]]),
        u16::from_be_bytes([b[6], b[7]]) & 0x0fff,
        (u16::from_be_bytes([b[8], b[9]]) & 0x3fff) | 0x8000,
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15],
    )
}

fn parse_date_parts(date: &str) -> (String, String, String) {
    let parts: Vec<&str> = date.splitn(3, '-').collect();
    (
        parts.first().copied().unwrap_or("").to_string(),
        parts.get(1).copied().unwrap_or("").to_string(),
        parts.get(2).copied().unwrap_or("").to_string(),
    )
}

fn to_camel_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

fn map_contributor_role(roles: &[String]) -> String {
    for role in roles {
        let cr_role = match role.as_str() {
            "Author" => Some("author"),
            "Editor" => Some("editor"),
            "Reviewer" => Some("reviewer"),
            "Translator" => Some("translator"),
            _ => None,
        };
        if let Some(r) = cr_role {
            return r.to_string();
        }
    }
    "author".to_string()
}

fn is_allowed_contributor(roles: &[String]) -> bool {
    if roles.is_empty() {
        return true; // treat empty roles as "Author" (backwards compat)
    }
    roles
        .iter()
        .any(|r| ALLOWED_CONTRIBUTOR_ROLES.contains(&r.as_str()))
}

// ── Shared builders ───────────────────────────────────────────────────────────

fn build_contributors(data: &Data) -> Contributors {
    let mut person_names: Vec<PersonName> = Vec::new();
    let mut organizations: Vec<Organization> = Vec::new();
    let mut seq_index = 0usize;

    for c in &data.contributors {
        if !is_allowed_contributor(&c.roles) {
            continue;
        }
        let role = map_contributor_role(&c.roles);
        let seq: &'static str = if seq_index == 0 {
            "first"
        } else {
            "additional"
        };
        seq_index += 1;

        if c.type_ == "Organization" {
            organizations.push(Organization {
                contributor_role: role,
                sequence: seq,
                name: c.name(),
            });
        } else if !c.given_name().is_empty() || !c.family_name().is_empty() {
            let affiliations = if c.affiliations().is_empty() {
                None
            } else {
                let inst: Vec<Institution> = c
                    .affiliations()
                    .iter()
                    .filter(|a| !a.name.is_empty())
                    .map(|a| Institution {
                        institution_name: a.name.clone(),
                        institution_id: if !a.id.is_empty() {
                            Some(InstitutionId {
                                type_: "ror".to_string(),
                                text: a.id.clone(),
                            })
                        } else {
                            None
                        },
                    })
                    .collect();
                if inst.is_empty() {
                    None
                } else {
                    Some(Affiliations { institution: inst })
                }
            };
            person_names.push(PersonName {
                contributor_role: role,
                sequence: seq,
                given_name: c.given_name().to_string(),
                surname: c.family_name().to_string(),
                affiliations,
                orcid: c.id().to_string(),
            });
        }
    }

    Contributors {
        person_name: person_names,
        organization: organizations,
    }
}

fn build_titles(data: &Data) -> Titles {
    let title = data.title.clone();

    let subtitle = data
        .additional_titles
        .iter()
        .find(|t| !t.title.is_empty() && t.type_ == "Subtitle")
        .map(|t| t.title.clone())
        .unwrap_or_default();

    Titles { title, subtitle }
}

fn doi_for_crossref(data: &Data) -> String {
    if let Some(identifier) = data
        .identifiers
        .iter()
        .find(|i| i.identifier_type == "DOI" && !i.identifier.is_empty())
        .map(|i| i.identifier.clone())
    {
        return validate_doi(&identifier).unwrap_or_default();
    }
    validate_doi(&data.id).unwrap_or_default()
}

fn build_abstract(data: &Data) -> Vec<JatsAbstract> {
    let mut result = Vec::new();
    if !data.description.is_empty() {
        result.push(JatsAbstract {
            xmlns_jats: "http://www.ncbi.nlm.nih.gov/JATS1",
            p: vec![JatsP { text: data.description.clone() }],
        });
    }
    for d in &data.additional_descriptions {
        if d.type_ == "Abstract" || d.type_ == "Other" {
            result.push(JatsAbstract {
                xmlns_jats: "http://www.ncbi.nlm.nih.gov/JATS1",
                p: vec![JatsP { text: d.description.clone() }],
            });
        }
    }
    result
}

fn build_funding_program(data: &Data) -> Option<FrProgram> {
    if data.funding_references.is_empty() {
        return None;
    }

    let funders_with_awards: Vec<_> = data
        .funding_references
        .iter()
        .filter(|f| !f.award_number.is_empty())
        .collect();
    let unique_funders: std::collections::HashSet<_> = data
        .funding_references
        .iter()
        .filter_map(|f| {
            if !f.funder_id.is_empty() {
                Some(f.funder_id.as_str())
            } else if !f.funder_name.is_empty() {
                Some(f.funder_name.as_str())
            } else {
                None
            }
        })
        .collect();
    let use_groups = !funders_with_awards.is_empty() && unique_funders.len() > 1;

    let mut assertions: Vec<FrAssertion> = Vec::new();
    for fr in &data.funding_references {
        let mut group: Vec<FrAssertion> = Vec::new();

        let (_, id_type) = if !fr.funder_id.is_empty() {
            validate_id(&fr.funder_id)
        } else {
            (String::new(), "")
        };

        let funder_assertion = if !fr.funder_id.is_empty() && id_type == "ROR" {
            FrAssertion {
                name: "ror".to_string(),
                text: fr.funder_id.clone(),
                nested: vec![],
            }
        } else if !fr.funder_id.is_empty() && id_type == "Crossref Funder ID" {
            FrAssertion {
                name: "funder_name".to_string(),
                text: fr.funder_name.clone(),
                nested: vec![FrAssertion {
                    name: "funder_identifier".to_string(),
                    text: fr.funder_id.clone(),
                    nested: vec![],
                }],
            }
        } else {
            FrAssertion {
                name: "funder_name".to_string(),
                text: fr.funder_name.clone(),
                nested: vec![],
            }
        };
        group.push(funder_assertion);

        if !fr.award_number.is_empty() {
            group.push(FrAssertion {
                name: "award_number".to_string(),
                text: fr.award_number.clone(),
                nested: vec![],
            });
        }

        if use_groups {
            assertions.push(FrAssertion {
                name: "fundgroup".to_string(),
                text: String::new(),
                nested: group,
            });
        } else {
            assertions.extend(group);
        }
    }

    Some(FrProgram {
        name: "fundref",
        assertions,
    })
}

fn build_license_program(data: &Data) -> Option<AiProgram> {
    if data.license.url.is_empty() {
        return None;
    }
    Some(AiProgram {
        name: "AccessIndicators",
        license_refs: vec![
            AiLicenseRef {
                applies_to: "vor",
                text: data.license.url.clone(),
            },
            AiLicenseRef {
                applies_to: "tdm",
                text: data.license.url.clone(),
            },
        ],
    })
}

fn build_relations_program(data: &Data) -> Option<RelProgram> {
    if data.relations.is_empty() {
        return None;
    }

    let mut related_items: Vec<RelRelatedItem> = Vec::new();
    for rel in &data.relations {
        if rel.id.is_empty() || rel.type_.is_empty() {
            continue;
        }

        // Determine identifier type
        let (id, id_type_raw) = validate_id(&rel.id);
        let identifier_type = if id_type_raw == "URL" {
            "uri".to_string()
        } else if id_type_raw == "DOI" {
            "doi".to_string()
        } else if id_type_raw == "ISSN" {
            "issn".to_string()
        } else {
            id_type_raw.to_lowercase()
        };

        if id.is_empty() {
            continue;
        }

        if INTER_WORK_RELATION_TYPES.contains(&rel.type_.as_str()) {
            related_items.push(RelRelatedItem {
                inter_work: Some(RelWorkRelation {
                    relationship_type: to_camel_case(&rel.type_),
                    identifier_type,
                    text: id,
                }),
                intra_work: None,
            });
        } else if INTRA_WORK_RELATION_TYPES.contains(&rel.type_.as_str()) {
            related_items.push(RelRelatedItem {
                inter_work: None,
                intra_work: Some(RelWorkRelation {
                    relationship_type: to_camel_case(&rel.type_),
                    identifier_type,
                    text: id,
                }),
            });
        }
    }

    if related_items.is_empty() {
        return None;
    }

    Some(RelProgram {
        name: "relations",
        related_items,
    })
}

fn build_version_info(data: &Data) -> Option<VersionInfo> {
    if data.version.is_empty() {
        return None;
    }
    Some(VersionInfo {
        version: data.version.clone(),
    })
}

fn build_doi_data(data: &Data) -> DoiData {
    let doi = doi_for_crossref(data);
    let mut items: Vec<Item> = vec![Item {
        resource: ResourceEl {
            mime_type: "text/html".to_string(),
            text: data.url.clone(),
        },
    }];
    for f in &data.files {
        if f.mime_type.is_empty() || f.url.is_empty() {
            continue;
        }
        if !items.iter().any(|i| i.resource.text == f.url) {
            items.push(Item {
                resource: ResourceEl {
                    mime_type: f.mime_type.clone(),
                    text: f.url.clone(),
                },
            });
        }
    }
    DoiData {
        doi,
        resource: data.url.clone(),
        collection: Some(Collection {
            property: "text-mining",
            item: items,
        }),
    }
}

fn build_citation_list(data: &Data) -> Option<CitationList> {
    if data.references.is_empty() {
        return None;
    }
    let citations: Vec<Citation> = data
        .references
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let key = if r.key.is_empty() {
                format!("ref{}", i + 1)
            } else {
                r.key.clone()
            };
            let doi_str = validate_doi(&r.id).unwrap_or_default();

            // if no doi, append URL/id to unstructured
            let unstructured = if doi_str.is_empty() && !r.id.is_empty() {
                let base = r.unstructured.trim();
                if !base.is_empty() && !base.ends_with(&r.id) {
                    format!("{} {}", base, r.id)
                } else if base.is_empty() {
                    r.id.clone()
                } else {
                    r.unstructured.clone()
                }
            } else {
                r.unstructured.clone()
            };

            Citation {
                key,
                doi: doi_str,
                journal_title: String::new(),
                author: String::new(),
                volume: r.volume.clone(),
                first_page: r.first_page.clone(),
                c_year: r.publication_year.clone(),
                article_title: r.reference.clone(),
                unstructured_citation: unstructured,
            }
        })
        .collect();

    if citations.is_empty() {
        None
    } else {
        Some(CitationList {
            citation: citations,
        })
    }
}

fn build_item_number(data: &Data) -> Option<ItemNumber> {
    for id in &data.identifiers {
        if id.identifier_type.to_uppercase() == "UUID" {
            return Some(ItemNumber {
                item_number_type: "uuid",
                text: id.identifier.replace('-', ""),
            });
        }
    }
    None
}

fn build_posted_date(data: &Data) -> PostedDate {
    let (year, month, day) = parse_date_parts(&data.date_published);
    PostedDate {
        media_type: "online",
        year,
        month,
        day,
    }
}

fn build_publication_date(data: &Data, media_type: &str) -> Option<PublicationDate> {
    if data.date_published.is_empty() {
        return None;
    }
    let (year, month, day) = parse_date_parts(&data.date_published);
    Some(PublicationDate {
        media_type: media_type.to_string(),
        year,
        month,
        day,
    })
}

fn build_issn(data: &Data) -> Option<Issn> {
    if data.container.identifier_type.to_uppercase() == "ISSN"
        && !data.container.identifier.is_empty()
    {
        return Some(Issn {
            media_type: "electronic",
            text: data.container.identifier.clone(),
        });
    }
    for id in &data.identifiers {
        if id.identifier_type.to_uppercase() == "ISSN" {
            return Some(Issn {
                media_type: "electronic",
                text: id.identifier.clone(),
            });
        }
    }
    None
}

fn build_institution(data: &Data) -> Option<Institution> {
    if data.publisher.name.is_empty() {
        return None;
    }
    Some(Institution {
        institution_name: data.publisher.name.clone(),
        institution_id: None,
    })
}

// ── Convert Data → Body ───────────────────────────────────────────────────────

fn convert(data: &Data) -> Body {
    let mut body = Body::default();

    let contributors = build_contributors(data);
    let titles = build_titles(data);
    let abstract_ = build_abstract(data);
    let funding_program = build_funding_program(data);
    let license_program = build_license_program(data);
    let relations_program = build_relations_program(data);
    let version_info = build_version_info(data);
    let doi_data = build_doi_data(data);
    let citation_list = build_citation_list(data);

    match data.type_.as_str() {
        "Article" | "BlogPost" | "Preprint" => {
            let posted_type = if data.type_ == "Preprint" {
                "preprint".to_string()
            } else if data.additional_type.is_empty() {
                "other".to_string()
            } else {
                data.additional_type.clone()
            };
            body.posted_content.push(PostedContent {
                type_: posted_type,
                language: data.language.clone(),
                group_title: data.container.title.clone(),
                contributors,
                titles,
                posted_date: build_posted_date(data),
                institution: build_institution(data),
                item_number: build_item_number(data),
                abstract_,
                funding_program,
                license_program,
                relations_program,
                version_info,
                doi_data,
                citation_list,
            });
        }
        "JournalArticle" => {
            let journal_issue = {
                let volume = if data.container.volume.is_empty() {
                    None
                } else {
                    Some(JournalVolume {
                        volume: data.container.volume.clone(),
                    })
                };
                let pub_date = build_publication_date(data, "");
                if volume.is_some() || !data.container.issue.is_empty() || pub_date.is_some() {
                    Some(JournalIssue {
                        publication_date: pub_date,
                        journal_volume: volume,
                        issue: data.container.issue.clone(),
                    })
                } else {
                    None
                }
            };
            body.journal.push(Journal {
                journal_metadata: JournalMetadata {
                    language: data.language.clone(),
                    full_title: data.container.title.clone(),
                    issn: build_issn(data),
                },
                journal_issue,
                journal_article: JournalArticle {
                    publication_type: "full_text",
                    language: data.language.clone(),
                    contributors,
                    titles,
                    abstract_,
                    publication_date: build_publication_date(data, "online"),
                    funding_program,
                    license_program,
                    relations_program,
                    version_info,
                    doi_data,
                    citation_list,
                },
            });
        }
        "Dissertation" => {
            body.dissertation.push(Dissertation {
                language: data.language.clone(),
                publication_type: "thesis",
                titles,
                approval_date: build_publication_date(data, ""),
                institution: build_institution(data),
                doi_data,
            });
        }
        _ => {}
    }

    body
}

fn build_doi_batch(body: Body) -> DoiBatch {
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    DoiBatch {
        xmlns: "http://www.crossref.org/schema/5.4.0",
        xmlns_ai: "http://www.crossref.org/AccessIndicators.xsd",
        xmlns_rel: "http://www.crossref.org/relations.xsd",
        xmlns_fr: "http://www.crossref.org/fundref.xsd",
        version: "5.4.0",
        head: Head {
            doi_batch_id: generate_batch_id(),
            timestamp,
            depositor: Depositor {
                depositor_name: String::new(),
                email_address: String::new(),
            },
            registrant: String::new(),
        },
        body,
    }
}

fn serialize_doi_batch(doi_batch: DoiBatch) -> Result<Vec<u8>> {
    let mut buf = String::new();
    let mut ser = Serializer::new(&mut buf);
    ser.indent(' ', 2);
    doi_batch
        .serialize(ser)
        .map_err(|e| Error::Serialize(e.to_string()))?;

    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", buf);
    Ok(xml.into_bytes())
}

// ── Public write function ─────────────────────────────────────────────────────

pub fn write(data: &Data) -> Result<Vec<u8>> {
    serialize_doi_batch(build_doi_batch(convert(data)))
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let mut body = Body::default();
    for data in list {
        let part = convert(data);
        body.posted_content.extend(part.posted_content);
        body.journal.extend(part.journal);
        body.dissertation.extend(part.dissertation);
    }
    serialize_doi_batch(build_doi_batch(body))
}

// ── XML input structs (Crossref API "unixsd" format) ─────────────────────────
// Element names match the literal XML names including namespace prefixes.
// quick-xml serde: `@` = attribute, `$text` = character data.

#[derive(Deserialize, Default, Clone)]
struct XmlCrossrefResult {
    query_result: XmlQueryResult,
}

#[derive(Deserialize, Default, Clone)]
struct XmlQueryResult {
    body: XmlQueryBody,
}

#[derive(Deserialize, Default, Clone)]
struct XmlQueryBody {
    query: XmlQuery,
}

#[derive(Deserialize, Default, Clone)]
struct XmlQuery {
    #[serde(rename = "@status", default)]
    status: String,
    #[serde(default)]
    doi: XmlDoi,
    #[serde(rename = "crm-item", default)]
    crm_items: Vec<XmlCrmItem>,
    #[serde(default)]
    doi_record: XmlDOIRecord,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDoi {
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCrmItem {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDOIRecord {
    #[serde(default)]
    crossref: XmlContent,
}

#[derive(Deserialize, Default, Clone)]
struct XmlContent {
    #[serde(default)]
    journal: Option<XmlJournal>,
    #[serde(default)]
    posted_content: Option<XmlPostedContent>,
    #[serde(default)]
    dissertation: Option<XmlDissertation>,
    #[serde(default)]
    book: Option<XmlBook>,
    #[serde(default)]
    conference: Option<XmlConference>,
    #[serde(default)]
    database: Option<XmlDatabase>,
    #[serde(default)]
    peer_review: Option<XmlPeerReview>,
    #[serde(default)]
    sa_component: Option<XmlSAComponent>,
}

// ── Journal ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlJournal {
    #[serde(default)]
    journal_metadata: XmlJournalMetadata,
    #[serde(default)]
    journal_issue: XmlJournalIssue,
    #[serde(default)]
    journal_article: XmlJournalArticle,
}

#[derive(Deserialize, Default, Clone)]
struct XmlJournalMetadata {
    #[serde(rename = "@language", default)]
    language: String,
    #[serde(default)]
    full_title: String,
    #[serde(default)]
    issn: Vec<XmlIssn>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlIssn {
    #[serde(rename = "@media_type", default)]
    media_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlJournalIssue {
    #[allow(dead_code)]
    #[serde(default)]
    publication_date: Vec<XmlPublicationDate>,
    #[serde(default)]
    journal_volume: XmlJournalVolume,
    #[serde(default)]
    issue: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlJournalVolume {
    #[serde(default)]
    volume: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlJournalArticle {
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(rename = "abstract", default)]
    abstract_: Vec<XmlJatsAbstract>,
    #[serde(default)]
    publication_date: Vec<XmlPublicationDate>,
    #[serde(default)]
    publisher_item: XmlPublisherItem,
    #[serde(default)]
    program: Vec<XmlProgram>,
    #[serde(default)]
    crossmark: Option<XmlCrossmark>,
    #[serde(default)]
    archive_locations: XmlArchiveLocations,
    #[serde(default)]
    doi_data: XmlDOIData,
    #[serde(default)]
    citation_list: XmlCitationList,
}

// ── PostedContent ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlPostedContent {
    #[allow(dead_code)]
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(rename = "@language", default)]
    language: String,
    #[serde(default)]
    group_title: String,
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    posted_date: XmlDateParts,
    #[serde(default)]
    item_number: XmlItemNumber,
    #[serde(rename = "abstract", default)]
    abstract_: Vec<XmlJatsAbstract>,
    #[serde(default)]
    program: Vec<XmlProgram>,
    #[serde(default)]
    doi_data: XmlDOIData,
    #[serde(default)]
    citation_list: XmlCitationList,
}

// ── Dissertation ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlDissertation {
    #[serde(rename = "@language", default)]
    language: String,
    #[serde(default)]
    person_name: Vec<XmlPersonName>,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    approval_date: XmlDateParts,
    #[serde(default)]
    institution: XmlInstitutionBlock,
    #[serde(default)]
    doi_data: XmlDOIData,
    #[serde(default)]
    citation_list: XmlCitationList,
}

// ── Book ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlBook {
    #[serde(default)]
    book_metadata: XmlBookMetadata,
    #[serde(default)]
    content_item: Option<XmlContentItem>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlBookMetadata {
    #[serde(rename = "@language", default)]
    language: String,
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(rename = "abstract", default)]
    abstract_: Vec<XmlJatsAbstract>,
    #[serde(default)]
    publication_date: Vec<XmlPublicationDate>,
    #[serde(default)]
    isbn: Vec<XmlIsbn>,
    #[serde(default)]
    doi_data: XmlDOIData,
}

#[derive(Deserialize, Default, Clone)]
struct XmlIsbn {
    #[serde(rename = "@media_type", default)]
    media_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlContentItem {
    #[allow(dead_code)]
    #[serde(rename = "@component_type", default)]
    component_type: String,
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    publication_date: Vec<XmlPublicationDate>,
    #[serde(default)]
    pages: XmlPages,
    #[serde(default)]
    doi_data: XmlDOIData,
    #[serde(default)]
    citation_list: XmlCitationList,
}

// ── Conference ────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlConference {
    #[serde(default)]
    event_metadata: XmlEventMetadata,
    #[serde(default)]
    proceedings_metadata: XmlProceedingsMetadata,
    #[serde(default)]
    conference_paper: XmlConferencePaper,
}

#[derive(Deserialize, Default, Clone)]
struct XmlEventMetadata {
    #[serde(default)]
    conference_name: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlProceedingsMetadata {
    #[serde(default)]
    isbn: Vec<XmlIsbn>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlConferencePaper {
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    publication_date: Vec<XmlPublicationDate>,
    #[serde(default)]
    pages: XmlPages,
    #[serde(default)]
    doi_data: XmlDOIData,
    #[serde(default)]
    citation_list: XmlCitationList,
}

// ── Database / Dataset ────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlDatabase {
    #[serde(default)]
    database_metadata: XmlDatabaseMetadata,
    #[serde(default)]
    dataset: XmlDataset,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDatabaseMetadata {
    #[serde(default)]
    titles: XmlTitles,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDataset {
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    database_date: XmlDatabaseDate,
    #[serde(default)]
    doi_data: XmlDOIData,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDatabaseDate {
    #[serde(default)]
    creation_date: XmlDateParts,
}

// ── PeerReview ────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlPeerReview {
    #[serde(default)]
    contributors: XmlContributors,
    #[serde(default)]
    titles: XmlTitles,
    #[serde(default)]
    review_date: XmlDateParts,
    #[serde(default)]
    program: Vec<XmlProgram>,
    #[serde(default)]
    doi_data: XmlDOIData,
}

// ── SAComponent ───────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlSAComponent {
    #[serde(default)]
    component_list: XmlComponentList,
}

#[derive(Deserialize, Default, Clone)]
struct XmlComponentList {
    #[serde(default)]
    component: Vec<XmlComponent>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlComponent {
    #[serde(default)]
    doi_data: XmlDOIData,
}

// ── Shared structs ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct XmlTitles {
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlContributors {
    #[serde(default)]
    person_name: Vec<XmlPersonName>,
    #[serde(default)]
    organization: Vec<XmlOrganization>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlPersonName {
    #[serde(rename = "@contributor_role", default)]
    contributor_role: String,
    #[allow(dead_code)]
    #[serde(rename = "@sequence", default)]
    sequence: String,
    #[serde(default)]
    given_name: String,
    #[serde(default)]
    surname: String,
    #[serde(default)]
    affiliation: Vec<String>,
    #[serde(default)]
    affiliations: Option<XmlAffiliations>,
    #[serde(rename = "ORCID", default)]
    orcid: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlOrganization {
    #[serde(rename = "@contributor_role", default)]
    contributor_role: String,
    #[serde(rename = "$text", default)]
    name: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlAffiliations {
    #[serde(default)]
    institution: Vec<XmlInstitution>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlInstitution {
    #[serde(default)]
    institution_name: String,
    #[serde(default)]
    institution_id: Option<XmlInstitutionId>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlInstitutionId {
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlInstitutionBlock {
    #[serde(default)]
    institution_name: String,
    #[allow(dead_code)]
    #[serde(default)]
    institution_id: Option<XmlInstitutionId>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlPublicationDate {
    #[serde(rename = "@media_type", default)]
    media_type: String,
    #[serde(default)]
    year: String,
    #[serde(default)]
    month: String,
    #[serde(default)]
    day: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDateParts {
    #[serde(default)]
    year: String,
    #[serde(default)]
    month: String,
    #[serde(default)]
    day: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlPages {
    #[serde(default)]
    first_page: String,
    #[serde(default)]
    last_page: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlItemNumber {
    #[serde(rename = "@item_number_type", default)]
    item_number_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlPublisherItem {
    #[serde(default)]
    item_number: XmlItemNumber,
}

#[derive(Deserialize, Default, Clone)]
struct XmlDOIData {
    #[allow(dead_code)]
    #[serde(default)]
    doi: String,
    #[serde(default)]
    resource: String,
    #[serde(default)]
    collection: Vec<XmlCollection>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCollection {
    #[serde(default)]
    item: Vec<XmlItem>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlItem {
    #[serde(default)]
    resource: XmlResource,
}

#[derive(Deserialize, Default, Clone)]
struct XmlResource {
    #[serde(rename = "@mime_type", default)]
    mime_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCitationList {
    #[serde(default)]
    citation: Vec<XmlCitation>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCitation {
    #[serde(rename = "@key", default)]
    key: String,
    #[allow(dead_code)]
    #[serde(rename = "@type", default)]
    type_: String,
    #[serde(default)]
    doi: Option<XmlCitationDoi>,
    #[serde(default)]
    article_title: String,
    #[serde(rename = "cYear", default)]
    c_year: String,
    #[serde(default)]
    volume: String,
    #[serde(default)]
    first_page: String,
    #[serde(default)]
    unstructured_citation: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCitationDoi {
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlArchiveLocations {
    #[serde(default)]
    archive: Vec<XmlArchive>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlArchive {
    #[serde(rename = "@name", default)]
    name: String,
}

// ── JATS abstract ─────────────────────────────────────────────────────────────
// quick-xml strips namespace prefixes during deserialization:
// <jats:abstract> → matched as "abstract", <jats:p> → matched as "p"

#[derive(Deserialize, Default, Clone)]
struct XmlJatsAbstract {
    #[serde(rename = "@abstract-type", default)]
    abstract_type: String,
    #[serde(default)]
    p: Vec<XmlJatsP>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlJatsP {
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Unified program struct ─────────────────────────────────────────────────────
// fr:program, ai:program, and plain program all deserialize as "program".
// Distinguished by @name: "fundref" | "AccessIndicators" | "" (relations).

#[derive(Deserialize, Default, Clone)]
struct XmlProgram {
    #[serde(rename = "@name", default)]
    name: String,
    // FundRef: fr:assertion → "assertion" after prefix strip
    #[serde(default)]
    assertion: Vec<XmlFrAssertion>,
    // AccessIndicators: ai:license_ref → "license_ref" after prefix strip
    #[serde(default)]
    license_ref: Vec<XmlAiLicenseRef>,
    // Relations: related_item
    #[serde(default)]
    related_item: Vec<XmlRelatedItem>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlFrAssertion {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "@provider", default)]
    provider: String,
    #[serde(rename = "$text", default)]
    text: String,
    // Recursive: fr:assertion children → "assertion" after prefix strip
    #[serde(default)]
    assertion: Vec<XmlFrAssertion>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlAiLicenseRef {
    #[serde(rename = "@applies_to", default)]
    applies_to: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default, Clone)]
struct XmlRelatedItem {
    #[serde(default)]
    inter_work_relation: Option<XmlWorkRelation>,
    #[serde(default)]
    intra_work_relation: Option<XmlWorkRelation>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlWorkRelation {
    #[serde(rename = "@relationship-type", default)]
    relationship_type: String,
    #[serde(rename = "@identifier-type", default)]
    identifier_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Crossmark (custom_metadata contains submission/acceptance dates) ───────────

#[derive(Deserialize, Default, Clone)]
struct XmlCrossmark {
    #[serde(default)]
    custom_metadata: XmlCustomMetadata,
}

#[derive(Deserialize, Default, Clone)]
struct XmlCustomMetadata {
    #[serde(default)]
    assertion: Vec<XmlAssertion>,
    // fr:program / ai:program also appear here, both strip to "program"
    #[serde(default)]
    program: Vec<XmlProgram>,
}

#[derive(Deserialize, Default, Clone)]
struct XmlAssertion {
    #[serde(rename = "@name", default)]
    name: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Type / container mapping ──────────────────────────────────────────────────

fn cr_xml_type(doi_type: &str) -> &'static str {
    match doi_type {
        "journal_article" => "JournalArticle",
        "journal_issue" => "JournalIssue",
        "journal_volume" => "JournalVolume",
        "journal_title" => "Journal",
        "conference_title" => "Proceedings",
        "conference_series" => "ProceedingsSeries",
        "conference_paper" => "ProceedingsArticle",
        "book_title" => "Book",
        "book_series" => "BookSeries",
        "book_content" => "BookChapter",
        "component" => "Component",
        "dissertation" => "Dissertation",
        "peer_review" => "PeerReview",
        "posted_content" => "Article",
        "report-paper_title" => "Report",
        "report-paper_series" => "ReportSeries",
        "standard_title" => "Standard",
        "standard_series" => "StandardSeries",
        "dataset" => "Dataset",
        _ => "Other",
    }
}

fn cr_xml_container_type(cm_type: &str) -> &'static str {
    match cm_type {
        "JournalArticle" => "Journal",
        "BookChapter" => "Book",
        "ProceedingsArticle" => "Proceedings",
        _ => "",
    }
}

// ── Date helpers ──────────────────────────────────────────────────────────────

fn date_from_parts(year: &str, month: &str, day: &str) -> String {
    let y = year.trim();
    if y.is_empty() {
        return String::new();
    }
    let m = month.trim();
    let d = day.trim();
    match (m.is_empty(), d.is_empty()) {
        (false, false) => format!("{}-{:0>2}-{:0>2}", y, m, d),
        (false, true) => format!("{}-{:0>2}", y, m),
        _ => y.to_string(),
    }
}

fn pick_pub_date(dates: &[XmlPublicationDate]) -> String {
    if dates.is_empty() {
        return String::new();
    }
    let idx = dates
        .iter()
        .position(|d| d.media_type == "online")
        .unwrap_or(0);
    date_from_parts(&dates[idx].year, &dates[idx].month, &dates[idx].day)
}

fn pick_issn(issns: &[XmlIssn]) -> (String, &'static str) {
    if issns.is_empty() {
        return (String::new(), "");
    }
    let idx = issns
        .iter()
        .position(|i| i.media_type == "electronic")
        .unwrap_or(0);
    (issns[idx].text.clone(), "ISSN")
}

fn pick_isbn(isbns: &[XmlIsbn]) -> (String, &'static str) {
    if isbns.is_empty() {
        return (String::new(), "");
    }
    let idx = isbns
        .iter()
        .position(|i| i.media_type == "electronic")
        .unwrap_or(0);
    (isbns[idx].text.clone(), "ISBN")
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn convert_abstract(abstracts: &[XmlJatsAbstract]) -> Vec<Description> {
    let mut out = Vec::new();
    for a in abstracts {
        let text =
            a.p.iter()
                .map(|p| p.text.trim())
                .collect::<Vec<_>>()
                .join(" ");
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let type_ = map_abstract_type(&a.abstract_type);
        out.push(Description {
            description: sanitize(text),
            type_,
            language: String::new(),
        });
    }
    out
}

/// Map Crossref abstract-type attributes to commonmeta v1.0 description type enum values.
fn map_abstract_type(raw: &str) -> String {
    match raw {
        "" => "Abstract",
        "executive-summary" | "summary" => "Summary",
        "methods" | "materials|methods" => "Methods",
        "technical-info" | "technical_info" => "TechnicalInfo",
        _ => "Other",
    }
    .to_string()
}

fn convert_contributors(contrib: &XmlContributors) -> Vec<Contributor> {
    let mut out = Vec::new();
    for p in &contrib.person_name {
        let id = if !p.orcid.is_empty() {
            normalize_orcid(&p.orcid)
        } else {
            String::new()
        };
        let role = match p.contributor_role.as_str() {
            "editor" => "Editor",
            "translator" => "Translator",
            "reviewer" => "Reviewer",
            _ => "Author",
        };
        let affiliations: Vec<Affiliation> = if let Some(affs) = &p.affiliations {
            affs.institution
                .iter()
                .map(|inst| {
                    let aff_id = inst
                        .institution_id
                        .as_ref()
                        .map(|i| {
                            if i.type_ == "ror" {
                                normalize_ror(&i.text)
                            } else {
                                i.text.clone()
                            }
                        })
                        .unwrap_or_default();
                    Affiliation {
                        id: aff_id,
                        name: inst.institution_name.clone(),
                        ..Default::default()
                    }
                })
                .collect()
        } else if !p.affiliation.is_empty() {
            p.affiliation
                .iter()
                .filter(|a| !a.is_empty())
                .map(|a| Affiliation {
                    name: a.clone(),
                    ..Default::default()
                })
                .collect()
        } else {
            Vec::new()
        };
        out.push(Contributor::person(
            DataPerson {
                id,
                given_name: p.given_name.clone(),
                family_name: p.surname.clone(),
                affiliations,
                asserted_by: String::new(),
            },
            normalize_contributor_roles(&[role.to_string()], role),
        ));
    }
    for org in &contrib.organization {
        if org.name.is_empty() {
            continue;
        }
        let role = match org.contributor_role.as_str() {
            "editor" => "Editor",
            "translator" => "Translator",
            "reviewer" => "Reviewer",
            _ => "Author",
        };
        out.push(Contributor::organization(
            DataOrganization { id: String::new(), name: org.name.clone(), asserted_by: String::new() },
            normalize_contributor_roles(&[role.to_string()], role),
        ));
    }
    out
}

fn convert_funding_references(programs: &[XmlProgram]) -> Vec<FundingReference> {
    let fr = match programs.iter().find(|p| p.name == "fundref") {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut refs: Vec<FundingReference> = Vec::new();
    for top in &fr.assertion {
        if top.name != "fundgroup" {
            continue;
        }
        let award_numbers: Vec<&str> = top
            .assertion
            .iter()
            .filter(|a| a.name == "award_number")
            .map(|a| a.text.as_str())
            .collect();

        let mut funder_name = String::new();
        let mut funder_id = String::new();

        for assertion in &top.assertion {
            if assertion.name == "funder_name" {
                funder_name = assertion.text.trim().to_string();
                for child in &assertion.assertion {
                    if child.name == "funder_identifier" {
                        if child.provider == "crossref" {
                            funder_id = normalize_doi(&format!("10.13039/{}", child.text.trim()));
                        } else {
                            let raw = child.text.trim();
                            funder_id = normalize_doi(raw);
                            if funder_id.is_empty() {
                                funder_id = raw.to_string();
                            }
                        }
                    }
                }
            }
        }
        if funder_name.is_empty() {
            continue;
        }
        if award_numbers.is_empty() {
            refs.push(FundingReference {
                funder_id: funder_id.clone(),
                funder_name: funder_name.clone(),
                ..Default::default()
            });
        } else {
            for award in &award_numbers {
                refs.push(FundingReference {
                    funder_id: funder_id.clone(),
                    funder_name: funder_name.clone(),
                    award_number: award.to_string(),
                    ..Default::default()
                });
            }
        }
    }
    dedupe_slice(refs)
}

fn convert_relations(programs: &[XmlProgram]) -> Vec<Relation> {
    let mut out = Vec::new();
    for prog in programs.iter().filter(|p| p.name.is_empty()) {
        for item in &prog.related_item {
            if let Some(iw) = &item.inter_work_relation {
                if iw.text.is_empty() {
                    continue;
                }
                let id = resolve_relation_id(&iw.text, &iw.identifier_type);
                let t = title_case(&iw.relationship_type);
                out.push(Relation { id, type_: t });
            }
            if let Some(iw) = &item.intra_work_relation {
                if iw.text.is_empty() {
                    continue;
                }
                let id = resolve_relation_id(&iw.text, &iw.identifier_type);
                let t = title_case(&iw.relationship_type);
                out.push(Relation { id, type_: t });
            }
        }
    }
    out
}

fn resolve_relation_id(text: &str, id_type: &str) -> String {
    match id_type {
        "doi" => normalize_doi(text),
        "issn" => issn_as_url(text),
        _ => {
            let (pid, _) = validate_id(text);
            if pid.is_empty() {
                text.to_string()
            } else {
                pid
            }
        }
    }
}

fn convert_citations(list: &XmlCitationList) -> Vec<Reference> {
    let mut out = Vec::new();
    let mut seen_keys = std::collections::HashSet::new();
    for c in &list.citation {
        let doi_text = c.doi.as_ref().map(|d| d.text.trim()).unwrap_or("");
        let id = if !doi_text.is_empty() {
            normalize_doi(doi_text)
        } else {
            String::new()
        };
        if id.is_empty() && c.unstructured_citation.is_empty() {
            continue;
        }
        if !c.key.is_empty() && !seen_keys.insert(c.key.clone()) {
            continue;
        }
        out.push(Reference {
            key: c.key.clone(),
            id,
            type_: String::new(),
            reference: c.article_title.clone(),
            publication_year: c.c_year.clone(),
            volume: c.volume.clone(),
            first_page: c.first_page.clone(),
            unstructured: c.unstructured_citation.clone(),
            ..Default::default()
        });
    }
    out
}

fn convert_files(collections: &[XmlCollection]) -> Vec<crate::data::File> {
    collections
        .iter()
        .flat_map(|c| c.item.iter())
        .filter(|i| !i.resource.text.is_empty() && !i.resource.mime_type.is_empty())
        .map(|i| crate::data::File {
            url: i.resource.text.clone(),
            mime_type: i.resource.mime_type.clone(),
            ..Default::default()
        })
        .collect()
}

fn pick_license(programs: &[XmlProgram]) -> License {
    let prog = match programs.iter().find(|p| p.name == "AccessIndicators") {
        Some(p) => p,
        None => return License::default(),
    };
    if prog.license_ref.is_empty() {
        return License::default();
    }
    let idx = prog
        .license_ref
        .iter()
        .position(|l| l.applies_to == "vor")
        .unwrap_or(0);
    let raw = &prog.license_ref[idx].text;
    let (url, _) = normalize_cc_url(raw);
    let url = if url.is_empty() { raw.clone() } else { url };
    crate::spdx::from_url(&url)
}

fn convert_item_number(item: &XmlItemNumber) -> Vec<Identifier> {
    if item.text.is_empty() {
        return Vec::new();
    }
    let raw_type = item.item_number_type.to_uppercase();
    let (id_text, id_type) = if raw_type == "UUID" {
        // Crossref strips dashes; reinsert them for a standard UUID
        let s = &item.text;
        let uuid = if s.len() == 32 {
            format!(
                "{}-{}-{}-{}-{}",
                &s[..8],
                &s[8..12],
                &s[12..16],
                &s[16..20],
                &s[20..]
            )
        } else {
            s.clone()
        };
        (uuid, "UUID".to_string())
    } else {
        // Map to the commonmeta identifier_type enum; unknown types fall back to "Other"
        let id_type = match raw_type.as_str() {
            "ARK" | "ARXIV" | "BIBCODE" | "DOI" | "HANDLE" | "ISBN" | "ISSN"
            | "OPENALEX" | "PMID" | "PMCID" | "PURL" | "RAID" | "SWHID"
            | "URL" | "URN" | "GUID" => raw_type,
            _ => "Other".to_string(),
        };
        (item.text.clone(), id_type)
    };
    vec![Identifier {
        identifier: id_text,
        identifier_type: id_type,
    }]
}

// ── Read (main conversion) ────────────────────────────────────────────────────

fn from_query(query: XmlQuery) -> Data {
    let mut data = Data::default();

    let doi_type = query.doi.type_.as_str();
    let type_str = cr_xml_type(doi_type);
    data.type_ = type_str.to_string();
    data.id = normalize_doi(&query.doi.text);
    data.provider = "Crossref".to_string();

    // Publisher info from crm-items
    let publisher_id = String::new();
    let mut publisher_name = String::new();
    for item in &query.crm_items {
        match item.name.as_str() {
            // Crossref member URLs are not ROR IDs; omit from publisher.id
            "member-id" => { let _ = &item.text; }
            "publisher-name" => publisher_name = item.text.clone(),
            "last-update" => data.date_updated = item.text.clone(),
            _ => {}
        }
    }
    data.publisher = Publisher {
        id: publisher_id,
        name: publisher_name.clone(),
        asserted_by: String::new(),
    };

    // Workaround: Front Matter posts registered as posted-content are blog posts
    if data.type_ == "Article" && publisher_name == "Front Matter" {
        data.type_ = "BlogPost".to_string();
    }

    let meta = &query.doi_record.crossref;

    // Extract type-specific fields
    let mut container_title = String::new();
    let mut issue = String::new();
    let mut volume = String::new();
    let mut language = String::new();
    let mut pages = XmlPages::default();
    let mut pub_dates: Vec<XmlPublicationDate> = Vec::new();
    let mut contributors = XmlContributors::default();
    let mut abstracts: Vec<XmlJatsAbstract> = Vec::new();
    let mut programs: Vec<XmlProgram> = Vec::new();
    let mut doi_data = XmlDOIData::default();
    let mut citation_list = XmlCitationList::default();
    let mut archive_names: Vec<String> = Vec::new();
    let mut issn_pair: (String, &'static str) = (String::new(), "");
    let mut isbn_pair: (String, &'static str) = (String::new(), "");
    let mut item_number = XmlItemNumber::default();
    let mut subjects: Vec<String> = Vec::new();

    match type_str {
        "Article" | "BlogPost" => {
            if let Some(pc) = &meta.posted_content {
                abstracts = pc.abstract_.clone();
                contributors = pc.contributors.clone();
                doi_data = pc.doi_data.clone();
                item_number = pc.item_number.clone();
                language = pc.language.clone();
                programs.extend(pc.program.clone());
                pub_dates.push(XmlPublicationDate {
                    media_type: "online".to_string(),
                    year: pc.posted_date.year.clone(),
                    month: pc.posted_date.month.clone(),
                    day: pc.posted_date.day.clone(),
                });
                if !pc.group_title.is_empty() {
                    subjects.push(pc.group_title.clone());
                    data.relations.push(Relation {
                        id: community_slug_as_url(&pc.group_title, "rogue-scholar.org"),
                        type_: "IsPartOf".to_string(),
                    });
                }
                citation_list = pc.citation_list.clone();
            }
        }
        "JournalArticle" | "JournalIssue" | "JournalVolume" | "Journal" => {
            if let Some(j) = &meta.journal {
                container_title = j.journal_metadata.full_title.clone();
                language = j.journal_metadata.language.clone();
                issn_pair = pick_issn(&j.journal_metadata.issn);
                volume = j.journal_issue.journal_volume.volume.clone();
                issue = j.journal_issue.issue.clone();
                if type_str == "JournalArticle" {
                    abstracts = j.journal_article.abstract_.clone();
                    contributors = j.journal_article.contributors.clone();
                    pub_dates = j.journal_article.publication_date.clone();
                    programs.extend(j.journal_article.program.clone());
                    if let Some(cm) = &j.journal_article.crossmark {
                        programs.extend(cm.custom_metadata.program.clone());
                        for a in &cm.custom_metadata.assertion {
                            match a.name.as_str() {
                                "received" => data.dates.submitted = a.text.clone(),
                                "accepted" => data.dates.accepted = a.text.clone(),
                                _ => {}
                            }
                        }
                    }
                    for n in &j.journal_article.archive_locations.archive {
                        archive_names.push(n.name.clone());
                    }
                    doi_data = j.journal_article.doi_data.clone();
                    item_number = j.journal_article.publisher_item.item_number.clone();
                    citation_list = j.journal_article.citation_list.clone();
                }
            }
        }
        "Dissertation" => {
            if let Some(d) = &meta.dissertation {
                language = d.language.clone();
                contributors = XmlContributors {
                    person_name: d.person_name.clone(),
                    organization: Vec::new(),
                };
                pub_dates.push(XmlPublicationDate {
                    media_type: String::new(),
                    year: d.approval_date.year.clone(),
                    month: d.approval_date.month.clone(),
                    day: d.approval_date.day.clone(),
                });
                doi_data = d.doi_data.clone();
                citation_list = d.citation_list.clone();
                if !d.institution.institution_name.is_empty() {
                    data.publisher = Publisher {
                        id: data.publisher.id.clone(),
                        name: d.institution.institution_name.clone(),
                        asserted_by: String::new(),
                    };
                }
            }
        }
        "Book" => {
            if let Some(b) = &meta.book {
                language = b.book_metadata.language.clone();
                contributors = b.book_metadata.contributors.clone();
                abstracts = b.book_metadata.abstract_.clone();
                pub_dates = b.book_metadata.publication_date.clone();
                isbn_pair = pick_isbn(&b.book_metadata.isbn);
                doi_data = b.book_metadata.doi_data.clone();
            }
        }
        "BookChapter" => {
            if let Some(b) = &meta.book {
                language = b.book_metadata.language.clone();
                isbn_pair = pick_isbn(&b.book_metadata.isbn);
                if let Some(ci) = &b.content_item {
                    contributors = ci.contributors.clone();
                    pub_dates = ci.publication_date.clone();
                    pages = ci.pages.clone();
                    doi_data = ci.doi_data.clone();
                    citation_list = ci.citation_list.clone();
                }
            }
        }
        "ProceedingsArticle" => {
            if let Some(c) = &meta.conference {
                container_title = c.event_metadata.conference_name.clone();
                isbn_pair = pick_isbn(&c.proceedings_metadata.isbn);
                contributors = c.conference_paper.contributors.clone();
                pub_dates = c.conference_paper.publication_date.clone();
                pages = c.conference_paper.pages.clone();
                doi_data = c.conference_paper.doi_data.clone();
                citation_list = c.conference_paper.citation_list.clone();
            }
        }
        "Dataset" => {
            if let Some(db) = &meta.database {
                container_title = db.database_metadata.titles.title.clone();
                contributors = db.dataset.contributors.clone();
                pub_dates.push(XmlPublicationDate {
                    media_type: String::new(),
                    year: db.dataset.database_date.creation_date.year.clone(),
                    month: db.dataset.database_date.creation_date.month.clone(),
                    day: db.dataset.database_date.creation_date.day.clone(),
                });
                doi_data = db.dataset.doi_data.clone();
            }
        }
        "PeerReview" => {
            if let Some(pr) = &meta.peer_review {
                contributors = pr.contributors.clone();
                programs.extend(pr.program.clone());
                pub_dates.push(XmlPublicationDate {
                    media_type: String::new(),
                    year: pr.review_date.year.clone(),
                    month: pr.review_date.month.clone(),
                    day: pr.review_date.day.clone(),
                });
                doi_data = pr.doi_data.clone();
            }
        }
        "Component" => {
            if let Some(sa) = &meta.sa_component
                && let Some(comp) = sa.component_list.component.first()
            {
                doi_data = comp.doi_data.clone();
            }
        }
        _ => {}
    }

    // Populate data fields
    data.language = language;
    data.url = doi_data.resource.clone();
    data.archive_locations = archive_names;

    data.date_published = pick_pub_date(&pub_dates);

    let (container_id, container_id_type) = if !issn_pair.0.is_empty() {
        issn_pair
    } else {
        isbn_pair
    };
    let container_type = cr_xml_container_type(&data.type_).to_string();
    data.container = Container {
        identifier: container_id.clone(),
        identifier_type: container_id_type.to_string(),
        type_: container_type,
        title: container_title,
        volume,
        issue,
        first_page: pages.first_page,
        last_page: pages.last_page,
        ..Default::default()
    };

    // ISSN → IsPartOf relation
    if container_id_type == "ISSN" && !container_id.is_empty() {
        data.relations.push(Relation {
            id: issn_as_url(&container_id),
            type_: "IsPartOf".to_string(),
        });
    }

    let extra_relations = convert_relations(&programs);
    data.relations.extend(extra_relations);

    data.contributors = convert_contributors(&contributors);
    let descs = convert_abstract(&abstracts);
    let mut desc_iter = descs.into_iter();
    if let Some(first) = desc_iter.next() {
        data.description = first.description;
        data.additional_descriptions.extend(desc_iter);
    }
    data.subjects = subjects
        .into_iter()
        .map(|s| Subject { subject: s, ..Default::default() })
        .collect();
    data.license = pick_license(&programs);
    data.funding_references = convert_funding_references(&programs);

    data.references = convert_citations(&citation_list);

    let mut files = convert_files(&doi_data.collection[..]);
    files = dedupe_slice(files);
    data.files = files;

    // Identifiers: DOI first, then item number
    data.identifiers.push(Identifier {
        identifier: data.id.clone(),
        identifier_type: "DOI".to_string(),
    });
    data.identifiers.extend(convert_item_number(&item_number));

    let (title_str, subtitle_str) = extract_titles_from_meta(meta, type_str);
    if !title_str.is_empty() {
        data.title = title_str;
    }
    if !subtitle_str.is_empty() {
        data.additional_titles.push(Title {
            title: subtitle_str,
            type_: "Subtitle".to_string(),
            ..Default::default()
        });
    }

    data
}

fn extract_titles_from_meta(meta: &XmlContent, type_str: &str) -> (String, String) {
    let t = match type_str {
        "JournalArticle" => meta.journal.as_ref().map(|j| &j.journal_article.titles),
        "Article" | "BlogPost" => meta.posted_content.as_ref().map(|pc| &pc.titles),
        "Dissertation" => meta.dissertation.as_ref().map(|d| &d.titles),
        "Book" => meta.book.as_ref().map(|b| &b.book_metadata.titles),
        "BookChapter" => meta
            .book
            .as_ref()
            .and_then(|b| b.content_item.as_ref().map(|ci| &ci.titles)),
        "ProceedingsArticle" => meta.conference.as_ref().map(|c| &c.conference_paper.titles),
        "Dataset" => meta.database.as_ref().map(|db| &db.dataset.titles),
        "PeerReview" => meta.peer_review.as_ref().map(|pr| &pr.titles),
        _ => None,
    };
    match t {
        Some(titles) => (titles.title.clone(), titles.subtitle.clone()),
        None => (String::new(), String::new()),
    }
}

// ── Public read / fetch ───────────────────────────────────────────────────────

/// Parse a Crossref XML API response (the full `crossref_result` envelope).
/// Normalize namespace-prefixed elements that all map to the same serde field.
/// quick-xml 0.37 strips namespace prefixes during deserialization, but its serde
/// struct deserializer raises "duplicate field" when different prefixed tags (`fr:program`,
/// `ai:program`) all resolve to the same field name. Normalizing them to a single
/// tag name before parsing avoids this limitation.
fn normalize_program_namespaces(xml: &str) -> String {
    xml.replace("<fr:program", "<program")
        .replace("</fr:program>", "</program>")
        .replace("<ai:program", "<program")
        .replace("</ai:program>", "</program>")
        .replace("<rel:program", "<program")
        .replace("</rel:program>", "</program>")
}

pub fn read_xml(input: &str) -> Result<Data> {
    let normalized = normalize_program_namespaces(input);
    let result: XmlCrossrefResult =
        xml_from_str(&normalized).map_err(|e| Error::Parse(e.to_string()))?;
    if result.query_result.body.query.status != "resolved" {
        return Err(Error::Parse("Crossref query not resolved".to_string()));
    }
    Ok(from_query(result.query_result.body.query))
}

/// Fetch a work from the Crossref XML API by DOI.
pub fn fetch(doi: &str) -> Result<Data> {
    let bare = doi
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/");
    let url = format!(
        "https://api.crossref.org/works/{}/transform/application/vnd.crossref.unixsd+xml",
        bare
    );
    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| Error::Http(e.to_string()))?;
    let xml = client
        .get(&url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?
        .text()
        .map_err(|e| Error::Http(e.to_string()))?;
    read_xml(&xml)
}

#[cfg(test)]
mod tests {
    use super::{build_doi_data, build_titles};
    use crate::data::{Data, Identifier, Title};

    #[test]
    fn build_titles_prefers_primary_and_subtitle() {
        let mut data = Data::default();
        data.title = "Main Title".to_string();
        data.additional_titles.push(Title {
            title: "Translated Title".to_string(),
            type_: "TranslatedTitle".to_string(),
            ..Default::default()
        });
        data.additional_titles.push(Title {
            title: "A Subtitle".to_string(),
            type_: "Subtitle".to_string(),
            ..Default::default()
        });

        let titles = build_titles(&data);
        assert_eq!(titles.title, "Main Title");
        assert_eq!(titles.subtitle, "A Subtitle");
    }

    #[test]
    fn build_doi_data_prefers_identifier_doi_over_id() {
        let mut data = Data {
            id: "https://example.org/not-a-doi".to_string(),
            url: "https://example.org/resource".to_string(),
            ..Default::default()
        };
        data.identifiers.push(Identifier {
            identifier: "https://doi.org/10.5555/12345678".to_string(),
            identifier_type: "DOI".to_string(),
        });

        let doi_data = build_doi_data(&data);
        assert_eq!(doi_data.doi, "10.5555/12345678");
    }

    #[test]
    fn build_doi_data_falls_back_to_id_doi() {
        let data = Data {
            id: "https://doi.org/10.9999/abc".to_string(),
            url: "https://example.org/resource".to_string(),
            ..Default::default()
        };

        let doi_data = build_doi_data(&data);
        assert_eq!(doi_data.doi, "10.9999/abc");
    }
}
