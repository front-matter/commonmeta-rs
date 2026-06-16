//! Core Commonmeta data model.
//!
//! Translated field-for-field from the Go `commonmeta` package (`reader.go`).
//! Each struct uses `rename_all = "camelCase"` to match the Go JSON tags, with
//! explicit `rename` overrides for the handful of irregular tags in the source
//! (`contentHTML`, `first_page`, `last_page`, and the reserved word `type`).
//! Go `omitempty` is reproduced with `skip_serializing_if`; Go pointer fields
//! become `Option<T>`.

use serde::{Deserialize, Serialize};

fn is_zero_i64(n: &i64) -> bool {
    *n == 0
}
fn is_zero_f64(n: &f64) -> bool {
    *n == 0.0
}

/// The native Commonmeta record. Mirrors `commonmeta.Data`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub additional_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub archive_locations: Vec<String>,
    #[serde(default)]
    pub container: Container,
    #[serde(rename = "contentHTML", default, skip_serializing_if = "String::is_empty")]
    pub content_html: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<Contributor>,
    #[serde(default)]
    pub date: Date,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub descriptions: Vec<Description>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub feature_image: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<File>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub funding_references: Vec<FundingReference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub geo_locations: Vec<GeoLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub identifiers: Vec<Identifier>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
    #[serde(default)]
    pub license: License,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provider: String,
    #[serde(default)]
    pub publisher: Publisher,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<Reference>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<Relation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<Subject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub titles: Vec<Title>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Affiliation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asserted_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub favicon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_page: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub volume: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contributor {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affiliations: Vec<Affiliation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributor_roles: Vec<String>,
}

/// All fields are ISO 8601 date strings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Date {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub submitted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accepted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub published: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub accessed: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub available: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub copyrighted: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub collected: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub valid: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub withdrawn: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub other: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    pub description: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct FundingReference {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub funder_identifier: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub funder_identifier_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub funder_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub award_number: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub award_title: String,
    #[serde(rename = "awardUri", default, skip_serializing_if = "String::is_empty")]
    pub award_uri: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoLocation {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub geo_location_place: String,
    #[serde(default, skip_serializing_if = "GeoLocationPoint::is_empty")]
    pub geo_location_point: GeoLocationPoint,
    #[serde(default, skip_serializing_if = "GeoLocationBox::is_empty")]
    pub geo_location_box: GeoLocationBox,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoLocationPoint {
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub point_longitude: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub point_latitude: f64,
}

impl GeoLocationPoint {
    pub fn is_empty(&self) -> bool {
        self.point_longitude == 0.0 && self.point_latitude == 0.0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeoLocationBox {
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub east_bound_longitude: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub west_bound_longitude: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub south_bound_latitude: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub north_bound_latitude: f64,
}

impl GeoLocationBox {
    pub fn is_empty(&self) -> bool {
        self.east_bound_longitude == 0.0
            && self.west_bound_longitude == 0.0
            && self.south_bound_latitude == 0.0
            && self.north_bound_latitude == 0.0
    }
}

/// The Go source uses snake_case JSON tags for the polygon fields.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoLocationPolygon {
    #[serde(rename = "polygon_points", default, skip_serializing_if = "Vec::is_empty")]
    pub polygon_points: Vec<GeoLocationPoint>,
    #[serde(
        rename = "in_polygon_point",
        default,
        skip_serializing_if = "GeoLocationPoint::is_empty"
    )]
    pub in_polygon_point: GeoLocationPoint,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identifier {
    pub identifier: String,
    pub identifier_type: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct License {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publisher {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// `first_page`/`last_page` keep the Go source's snake_case JSON tags.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
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
    #[serde(rename = "first_page", default, skip_serializing_if = "String::is_empty")]
    pub first_page: String,
    #[serde(rename = "last_page", default, skip_serializing_if = "String::is_empty")]
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
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Subject {
    pub subject: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Title {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub language: String,
}
