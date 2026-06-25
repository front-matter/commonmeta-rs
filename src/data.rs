//! Core Commonmeta data model.
//!
//! `Data` mirrors the commonmeta v1.0 JSON schema
//! (`resources/commonmeta_v1.0.json`) directly — field names and nesting
//! match the schema 1:1, the same way `commonmeta-py`'s `Metadata` class
//! *is* the v1.0 shape rather than an internal model translated to/from it.
//!
//! A few sub-structs carry fields beyond what the schema defines, because
//! other formats' writers genuinely need them for round-tripping (e.g.
//! `Reference.unstructured`/`.asserted_by`, `Dates.collected`/`.valid`/
//! `.other`/`.copyrighted`). These ride along unserialized-by-default
//! wherever empty and don't affect schema validation, since the schema's
//! nested item definitions don't set `additionalProperties: false`.

use serde::{Deserialize, Deserializer, Serialize};

fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}

/// The native Commonmeta record, shaped like the v1.0 JSON schema.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Data {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_descriptions: Vec<Description>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_titles: Vec<Title>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub additional_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive_locations: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<Citation>,
    #[serde(default, skip_serializing_if = "Container::is_empty")]
    pub container: Container,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<Contributor>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date_published: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub date_updated: String,
    #[serde(default, skip_serializing_if = "Dates::is_empty")]
    pub dates: Dates,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<File>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub funding_references: Vec<FundingReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geo_locations: Vec<GeoLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifiers: Vec<Identifier>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default, skip_serializing_if = "License::is_empty")]
    pub license: License,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default, skip_serializing_if = "Publisher::is_empty")]
    pub publisher: Publisher,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schema_version: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<Subject>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Affiliation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub citation: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Container {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identifier: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub identifier_type: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub platform: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub volume: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue: String,
}

impl Container {
    pub fn is_empty(&self) -> bool {
        self.identifier.is_empty()
            && self.identifier_type.is_empty()
            && self.type_.is_empty()
            && self.title.is_empty()
            && self.description.is_empty()
            && self.language.is_empty()
            && self.license.is_none()
            && self.platform.is_empty()
            && self.image.is_empty()
            && self.first_page.is_empty()
            && self.last_page.is_empty()
            && self.volume.is_empty()
            && self.issue.is_empty()
    }
}

/// `type_` is "Person" or "Organization"; exactly one of `person`/
/// `organization` is set accordingly.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Contributor {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<Person>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization: Option<Organization>,
    #[serde(default, skip_serializing_if = "Vec::is_empty", alias = "contributor_roles")]
    pub roles: Vec<String>,
}

impl Contributor {
    pub fn person(person: Person, roles: Vec<String>) -> Self {
        Contributor {
            type_: "Person".to_string(),
            person: Some(person),
            organization: None,
            roles,
        }
    }

    pub fn organization(organization: Organization, roles: Vec<String>) -> Self {
        Contributor {
            type_: "Organization".to_string(),
            person: None,
            organization: Some(organization),
            roles,
        }
    }

    pub fn given_name(&self) -> &str {
        self.person.as_ref().map_or("", |p| p.given_name.as_str())
    }

    pub fn family_name(&self) -> &str {
        self.person.as_ref().map_or("", |p| p.family_name.as_str())
    }

    /// The person's ORCID, or the organization's name when this contributor
    /// has no person (i.e. a display name regardless of contributor type).
    pub fn name(&self) -> String {
        if let Some(p) = &self.person {
            format!("{} {}", p.given_name, p.family_name)
                .trim()
                .to_string()
        } else {
            self.organization
                .as_ref()
                .map(|o| o.name.clone())
                .unwrap_or_default()
        }
    }

    pub fn id(&self) -> &str {
        self.person
            .as_ref()
            .map(|p| p.id.as_str())
            .or_else(|| self.organization.as_ref().map(|o| o.id.as_str()))
            .unwrap_or("")
    }

    pub fn affiliations(&self) -> &[Affiliation] {
        self.person.as_ref().map_or(&[], |p| p.affiliations.as_slice())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ContributorInput {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(default)]
    name: String,
    #[serde(default, alias = "givenName", alias = "given_name")]
    given_name: String,
    #[serde(default, alias = "familyName", alias = "family_name")]
    family_name: String,
    #[serde(default)]
    affiliations: Vec<Affiliation>,
    #[serde(default, alias = "roles", alias = "contributor_roles")]
    roles: Vec<String>,
    #[serde(default)]
    person: Option<Person>,
    #[serde(default)]
    organization: Option<Organization>,
}

/// Accepts both the v1.0 `{type, person: {...}, organization: {...}, roles}`
/// shape and a flat legacy shape (`id`/`name`/`given_name`/`family_name`/
/// `affiliations`/`contributor_roles` directly on the contributor object).
impl<'de> Deserialize<'de> for Contributor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = ContributorInput::deserialize(deserializer)?;

        let person = input.person.or_else(|| {
            if input.given_name.is_empty() && input.family_name.is_empty() && input.id.is_empty()
            {
                None
            } else {
                Some(Person {
                    id: input.id.clone(),
                    given_name: input.given_name,
                    family_name: input.family_name,
                    affiliations: input.affiliations.clone(),
                    asserted_by: String::new(),
                })
            }
        });

        let organization = input.organization.or_else(|| {
            if person.is_some() || input.name.is_empty() {
                None
            } else {
                Some(Organization {
                    id: input.id.clone(),
                    name: input.name,
                    asserted_by: String::new(),
                })
            }
        });

        let type_ = if !input.type_.is_empty() {
            input.type_
        } else if person.is_some() {
            "Person".to_string()
        } else if organization.is_some() {
            "Organization".to_string()
        } else {
            String::new()
        };

        Ok(Contributor {
            type_,
            person,
            organization,
            roles: input.roles,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Person {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affiliations: Vec<Affiliation>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Organization {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

/// "Other dates" beyond `date_published`/`date_updated`. All fields are
/// ISO 8601 date strings. `collected`/`valid`/`other`/`copyrighted` are
/// carried for DataCite/InvenioRDM round-tripping and aren't part of the
/// v1.0 schema's `dates` definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Dates {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub submitted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accepted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accessed: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub available: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub withdrawn: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collected: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub valid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub copyrighted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub other: String,
}

impl Dates {
    pub fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.submitted.is_empty()
            && self.accepted.is_empty()
            && self.accessed.is_empty()
            && self.available.is_empty()
            && self.withdrawn.is_empty()
            && self.collected.is_empty()
            && self.valid.is_empty()
            && self.copyrighted.is_empty()
            && self.other.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Description {
    pub description: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct File {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub bucket: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub checksum: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "is_zero_i64")]
    pub size: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mime_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FundingReference {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub funder_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub funder_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub award_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub award_title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub award_number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

/// Flattened to match the v1.0 schema's `geo_locations` shape directly
/// (no nested point/box objects).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub geo_location_place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_point_longitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_point_latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_box_west_longitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_box_east_longitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_box_south_latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geo_location_box_north_latitude: Option<f64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub geo_location_polygon: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Identifier {
    pub identifier: String,
    pub identifier_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct License {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

impl License {
    pub fn is_empty(&self) -> bool {
        self.id.is_empty()
            && self.title.is_empty()
            && self.url.is_empty()
            && self.asserted_by.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Publisher {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

impl Publisher {
    pub fn is_empty(&self) -> bool {
        self.id.is_empty() && self.name.is_empty() && self.asserted_by.is_empty()
    }
}

/// `publisher`/`publication_year`/`volume`/`issue`/`first_page`/`last_page`/
/// `unstructured`/`asserted_by` ride along for internal use (e.g. the
/// crossref_xml and InvenioRDM writers); only `key`/`id`/`type_`/
/// `reference` are part of the v1.0 schema's `references` definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publication_year: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub volume: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub unstructured: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Relation {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subject {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Title {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
}
