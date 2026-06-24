use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::author_utils::{
    cleanup_author, infer_contributor_type, normalize_contributor_roles, parse_affiliations,
    split_person_name,
};
/// Deserialize a JSON string-or-null as String, treating null as "".
fn null_to_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

/// Like null_to_string but also coerces numeric scalars to their string
/// representation (e.g. a bare integer year in a `date` field).
fn value_to_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

use crate::data::{
    Affiliation, Citation, Container, Contributor, Data, Description, FundingReference,
    GeoLocation, Identifier, Organization, Person, Publisher, Reference, Relation, Subject, Title,
};
use crate::constants as C;
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::{Error, Result};
use crate::utils::{
    normalize_cc_url, normalize_id, normalize_orcid, normalize_ror, normalize_url, sanitize,
    validate_id,
};

// ── API response structs ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct DcResponse {
    data: DcData,
}

#[derive(Deserialize, Default)]
struct DcData {
    attributes: DcAttributes,
}

#[derive(Deserialize, Default)]
struct DcAttributes {
    #[serde(default, deserialize_with = "null_to_string")]
    doi: String,
    #[serde(rename = "alternateIdentifiers", default)]
    alternate_identifiers: Vec<DcAlternateIdentifier>,
    #[serde(default)]
    creators: Vec<DcContributor>,
    #[serde(default)]
    contributors: Vec<DcContributor>,
    // String (schema ≤4.4) or struct (schema 4.5+)
    publisher: Option<Value>,
    container: Option<DcContainer>,
    // int or string depending on DataCite schema version
    #[serde(rename = "publicationYear")]
    publication_year: Option<Value>,
    #[serde(default)]
    titles: Vec<DcTitle>,
    url: Option<String>,
    #[serde(default)]
    subjects: Vec<DcSubject>,
    #[serde(default)]
    dates: Vec<DcDate>,
    language: Option<String>,
    types: Option<DcTypes>,
    #[serde(rename = "relatedIdentifiers", default)]
    related_identifiers: Vec<DcRelatedIdentifier>,
    version: Option<String>,
    #[serde(rename = "rightsList", default)]
    rights_list: Vec<DcRights>,
    #[serde(default)]
    descriptions: Vec<DcDescription>,
    #[serde(rename = "geoLocations", default)]
    geo_locations: Vec<DcGeoLocation>,
    #[serde(rename = "fundingReferences", default)]
    funding_references: Vec<DcFundingReference>,
}

#[derive(Deserialize, Default)]
struct DcAlternateIdentifier {
    #[serde(
        rename = "alternateIdentifier",
        default,
        deserialize_with = "null_to_string"
    )]
    alternate_identifier: String,
    #[serde(
        rename = "alternateIdentifierType",
        default,
        deserialize_with = "null_to_string"
    )]
    alternate_identifier_type: String,
}

// Affiliation is either Vec<String> or Vec<struct>; use Value
#[derive(Deserialize, Default)]
struct DcContributor {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(rename = "givenName", default, deserialize_with = "null_to_string")]
    given_name: String,
    #[serde(rename = "familyName", default, deserialize_with = "null_to_string")]
    family_name: String,
    #[serde(rename = "nameType", default, deserialize_with = "null_to_string")]
    name_type: String,
    affiliation: Option<Value>,
    #[serde(rename = "nameIdentifiers", default)]
    name_identifiers: Vec<DcNameIdentifier>,
    #[serde(
        rename = "contributorType",
        default,
        deserialize_with = "null_to_string"
    )]
    contributor_type: String,
}

#[derive(Deserialize, Default)]
struct DcNameIdentifier {
    #[serde(
        rename = "nameIdentifier",
        default,
        deserialize_with = "null_to_string"
    )]
    name_identifier: String,
    #[serde(
        rename = "nameIdentifierScheme",
        default,
        deserialize_with = "null_to_string"
    )]
    name_identifier_scheme: String,
}

#[derive(Deserialize, Default)]
struct DcPublisherStruct {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(
        rename = "publisherIdentifier",
        default,
        deserialize_with = "null_to_string"
    )]
    publisher_identifier: String,
}

#[derive(Deserialize, Default)]
struct DcContainer {
    #[serde(rename = "type", default, deserialize_with = "null_to_string")]
    type_: String,
    #[serde(default, deserialize_with = "null_to_string")]
    identifier: String,
    #[serde(
        rename = "identifierType",
        default,
        deserialize_with = "null_to_string"
    )]
    identifier_type: String,
    #[serde(default, deserialize_with = "null_to_string")]
    title: String,
    #[serde(default, deserialize_with = "null_to_string")]
    volume: String,
    #[serde(default, deserialize_with = "null_to_string")]
    issue: String,
    #[serde(rename = "firstPage", default, deserialize_with = "null_to_string")]
    first_page: String,
    #[serde(rename = "lastPage", default, deserialize_with = "null_to_string")]
    last_page: String,
}

#[derive(Deserialize, Default)]
struct DcTitle {
    #[serde(default, deserialize_with = "null_to_string")]
    title: String,
    #[serde(rename = "titleType", default, deserialize_with = "null_to_string")]
    title_type: String,
    #[serde(default, deserialize_with = "null_to_string")]
    lang: String,
}

#[derive(Deserialize, Default)]
struct DcSubject {
    #[serde(default, deserialize_with = "null_to_string")]
    subject: String,
}

#[derive(Deserialize, Default)]
struct DcDate {
    #[serde(default, deserialize_with = "value_to_string")]
    date: String,
    #[serde(rename = "dateType", default, deserialize_with = "null_to_string")]
    date_type: String,
}

#[derive(Deserialize, Default)]
struct DcTypes {
    #[serde(
        rename = "resourceTypeGeneral",
        default,
        deserialize_with = "null_to_string"
    )]
    resource_type_general: String,
    #[serde(rename = "resourceType", default, deserialize_with = "null_to_string")]
    resource_type: String,
}

#[derive(Deserialize, Default)]
struct DcRelatedIdentifier {
    #[serde(
        rename = "relatedIdentifier",
        default,
        deserialize_with = "null_to_string"
    )]
    related_identifier: String,
    #[serde(rename = "relationType", default, deserialize_with = "null_to_string")]
    relation_type: String,
    #[serde(
        rename = "resourceTypeGeneral",
        default,
        deserialize_with = "null_to_string"
    )]
    resource_type_general: String,
}

#[derive(Deserialize, Default)]
struct DcRights {
    #[serde(rename = "rightsUri", default, deserialize_with = "null_to_string")]
    rights_uri: String,
}

#[derive(Deserialize, Default)]
struct DcDescription {
    #[serde(default, deserialize_with = "null_to_string")]
    description: String,
    #[serde(
        rename = "descriptionType",
        default,
        deserialize_with = "null_to_string"
    )]
    description_type: String,
    #[serde(default, deserialize_with = "null_to_string")]
    lang: String,
}

#[derive(Deserialize, Default)]
struct DcGeoLocation {
    #[serde(
        rename = "geoLocationPlace",
        default,
        deserialize_with = "null_to_string"
    )]
    geo_location_place: String,
    #[serde(rename = "geoLocationPoint")]
    geo_location_point: Option<DcGeoPoint>,
    #[serde(rename = "geoLocationBox")]
    geo_location_box: Option<DcGeoBox>,
}

#[derive(Deserialize, Default)]
struct DcGeoPoint {
    #[serde(rename = "pointLongitude")]
    point_longitude: Option<Value>,
    #[serde(rename = "pointLatitude")]
    point_latitude: Option<Value>,
}

#[derive(Deserialize, Default)]
struct DcGeoBox {
    #[serde(rename = "westBoundLongitude")]
    west_bound_longitude: Option<Value>,
    #[serde(rename = "eastBoundLongitude")]
    east_bound_longitude: Option<Value>,
    #[serde(rename = "southBoundLatitude")]
    south_bound_latitude: Option<Value>,
    #[serde(rename = "northBoundLatitude")]
    north_bound_latitude: Option<Value>,
}

#[derive(Deserialize, Default)]
struct DcFundingReference {
    #[serde(rename = "funderName", default, deserialize_with = "null_to_string")]
    funder_name: String,
    #[serde(
        rename = "funderIdentifier",
        default,
        deserialize_with = "null_to_string"
    )]
    funder_identifier: String,
    #[serde(
        rename = "funderIdentifierType",
        default,
        deserialize_with = "null_to_string"
    )]
    funder_identifier_type: String,
    #[serde(rename = "awardNumber", default, deserialize_with = "null_to_string")]
    award_number: String,
    #[serde(rename = "awardTitle", default, deserialize_with = "null_to_string")]
    award_title: String,
    #[serde(rename = "awardUri", default, deserialize_with = "null_to_string")]
    award_uri: String,
}

// ── Type mapping ───────────────────────────────────────────────────────────────

pub(crate) fn dc_to_cm_relation(rt: &str) -> &'static str {
    match rt {
        "Reviews" => "IsReviewOf",
        "IsReviewedBy" => "HasReview",
        _ => "",
    }
}

fn parse_geo_coord(v: &Option<Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

pub(crate) fn is_reference_relation(rt: &str) -> bool {
    matches!(rt, "Cites" | "References")
}

pub(crate) fn is_citation_relation(rt: &str) -> bool {
    matches!(rt, "IsCitedBy" | "IsReferencedBy")
}

pub(crate) fn is_supported_relation(rt: &str) -> bool {
    matches!(
        rt,
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
            | "IsReviewedBy"
            | "Reviews"
            | "IsPreprintOf"
            | "HasPreprint"
            | "IsSupplementTo"
    )
}

pub(crate) fn is_recognized_role(role: &str) -> bool {
    matches!(
        role,
        "Author"
            | "Editor"
            | "Chair"
            | "Reviewer"
            | "Translator"
            | "ContactPerson"
            | "DataCollector"
            | "DataCurator"
            | "DataCuration"
            | "DataManager"
            | "Distributor"
            | "HostingInstitution"
            | "Producer"
            | "ProjectLeader"
            | "ProjectManager"
            | "ProjectMember"
            | "RegistrationAgency"
            | "RegistrationAuthority"
            | "RelatedPerson"
            | "Researcher"
            | "ResearchGroup"
            | "RightsHolder"
            | "Sponsor"
            | "Supervisor"
            | "Supervision"
            | "WorkPackageLeader"
            | "Other"
    )
}

pub(crate) fn normalize_commonmeta_role(role: &str) -> String {
    match role {
        "DataCurator" => "DataCuration".to_string(),
        "Supervisor" => "Supervision".to_string(),
        _ => role.to_string(),
    }
}

// ── Contributor conversion ─────────────────────────────────────────────────────

fn get_contributor(v: DcContributor, default_role: &str) -> Contributor {
    let DcContributor {
        name,
        given_name,
        family_name,
        name_type,
        affiliation,
        name_identifiers,
        contributor_type: _,
    } = v;

    // DataCite uses "Personal"/"Organizational"; strip last 2 chars → "Person"/"Organization"
    let name_type = match name_type.as_str() {
        "Personal" | "Organizational" => name_type[..name_type.len() - 2].to_string(),
        "Person" | "Organization" => name_type,
        _ => String::new(),
    };

    let mut id = String::new();
    for ni in &name_identifiers {
        if ni.name_identifier_scheme == "ORCID" || ni.name_identifier_scheme.contains("orcid.org")
        {
            id = normalize_orcid(&ni.name_identifier);
            break;
        } else if ni.name_identifier_scheme == "ROR" || ni.name_identifier_scheme.contains("ror.org") {
            id = normalize_ror(&ni.name_identifier);
            break;
        }
    }

    let mut name = cleanup_author(Some(&name)).unwrap_or(name);
    let mut given_name = given_name;
    let mut family_name = family_name;

    let mut inferred_type = infer_contributor_type(
        &name_type,
        &id,
        &given_name,
        &family_name,
        &name,
        None,
    );
    if inferred_type.is_empty() {
        inferred_type = "Organization".to_string();
    }

    if inferred_type == "Person" && !name.is_empty() && given_name.is_empty() && family_name.is_empty() {
        let (given, family, remainder) = split_person_name(&name);
        if !given.is_empty() || !family.is_empty() {
            given_name = given;
            family_name = family;
            name = String::new();
        } else {
            name = remainder;
        }
    }

    let affiliations: Vec<Affiliation> = if let Some(aff_val) = affiliation {
        match aff_val {
            Value::Array(values) => parse_affiliations(&values),
            other => {
                let values = vec![other];
                parse_affiliations(&values)
            }
        }
    } else {
        vec![]
    };

    let normalized_default_role = normalize_commonmeta_role(default_role);
    let roles = normalize_contributor_roles(
        &[normalized_default_role.clone()],
        &normalized_default_role,
    );

    if inferred_type == "Person" {
        Contributor::person(
            Person { id, given_name, family_name, affiliations, asserted_by: String::new() },
            roles,
        )
    } else {
        Contributor::organization(
            Organization { id, name, asserted_by: String::new() },
            roles,
        )
    }
}

// ── Core conversion ────────────────────────────────────────────────────────────

fn from_attributes(attr: DcAttributes) -> Data {
    let mut data = Data::default();

    let doi_id = normalize_doi(&attr.doi);
    data.id = doi_id.clone();

    // Type
    let types = attr.types.unwrap_or_default();
    data.type_ = C::dc_to_cm(&types.resource_type_general).to_string();

    // resourceType overrides resourceTypeGeneral when it maps to a CM type (schema 4.4+)
    let additional = C::dc_to_cm(&types.resource_type);
    if !additional.is_empty() {
        data.type_ = additional.to_string();
    } else if !types.resource_type.is_empty()
        && !types.resource_type.eq_ignore_ascii_case(&data.type_)
    {
        data.additional_type = types.resource_type.clone();
    }

    // Container
    if let Some(c) = attr.container {
        data.container = Container {
            type_: c.type_,
            identifier: c.identifier,
            identifier_type: c.identifier_type,
            title: c.title,
            volume: c.volume,
            issue: c.issue,
            first_page: c.first_page,
            last_page: c.last_page,
            ..Default::default()
        };
    }

    // Creators → all get role "Author"
    for v in attr.creators {
        if v.name.is_empty() && v.given_name.is_empty() && v.family_name.is_empty() {
            continue;
        }
        let contrib = get_contributor(v, "Author");
        let id = contrib.id();
        if id.is_empty()
            || !data
                .contributors
                .iter()
                .any(|c| !c.id().is_empty() && c.id() == id)
        {
            data.contributors.push(contrib);
        }
    }

    // Contributors → merge in, honoring contributorType
    for v in attr.contributors {
        if v.name.is_empty() && v.given_name.is_empty() && v.family_name.is_empty() {
            continue;
        }
        let role = if is_recognized_role(&v.contributor_type) {
            v.contributor_type.clone()
        } else {
            "Author".to_string()
        };
        let contrib = get_contributor(v, &role);
        let id = contrib.id();
        if id.is_empty()
            || !data
                .contributors
                .iter()
                .any(|c| !c.id().is_empty() && c.id() == id)
        {
            data.contributors.push(contrib);
        }
    }

    // Dates
    for d in attr.dates {
        match d.date_type.as_str() {
            "Accepted" => data.dates.accepted = d.date,
            "Available" => data.dates.available = d.date,
            "Collected" => data.dates.collected = d.date,
            "Created" => data.dates.created = d.date,
            "Issued" | "Published" => data.date_published = d.date,
            "Submitted" => data.dates.submitted = d.date,
            "Updated" => data.date_updated = d.date,
            "Valid" => data.dates.valid = d.date,
            "Withdrawn" => data.dates.withdrawn = d.date,
            "Other" => data.dates.other = d.date,
            _ => {}
        }
    }
    // Fall back to publicationYear
    if data.date_published.is_empty()
        && let Some(py) = attr.publication_year
    {
        data.date_published = match py {
            Value::Number(n) => n.as_i64().map(|y| y.to_string()).unwrap_or_default(),
            Value::String(s) => s,
            _ => String::new(),
        };
    }

    // Descriptions: first Abstract/Summary → data.description; rest → additional_descriptions
    for d in attr.descriptions {
        let type_ = match d.description_type.as_str() {
            "Abstract" | "Summary" | "Methods" | "TechnicalInfo" | "Other" => {
                d.description_type.clone()
            }
            _ => "Other".to_string(),
        };
        let text = sanitize(&d.description);
        if data.description.is_empty() && matches!(type_.as_str(), "Abstract" | "Summary") {
            data.description = text;
        } else {
            data.additional_descriptions.push(Description {
                description: text,
                type_,
                language: d.lang,
            });
        }
    }

    // Funding references
    for f in attr.funding_references {
        let funder_id = match f.funder_identifier_type.as_str() {
            "ROR" => normalize_ror(&f.funder_identifier),
            "Crossref Funder ID" => normalize_doi(&f.funder_identifier),
            _ if f.funder_identifier.starts_with("https://")
                || f.funder_identifier.starts_with("http://") =>
            {
                f.funder_identifier.clone()
            }
            _ => String::new(),
        };
        data.funding_references.push(FundingReference {
            funder_id,
            funder_name: f.funder_name,
            award_number: f.award_number,
            award_title: f.award_title,
            award_id: f.award_uri,
        });
    }

    // GeoLocations
    for g in attr.geo_locations {
        data.geo_locations.push(GeoLocation {
            geo_location_place: g.geo_location_place,
            geo_location_point_longitude: g.geo_location_point.as_ref().and_then(|p| parse_geo_coord(&p.point_longitude)),
            geo_location_point_latitude: g.geo_location_point.as_ref().and_then(|p| parse_geo_coord(&p.point_latitude)),
            geo_location_box_west_longitude: g.geo_location_box.as_ref().and_then(|b| parse_geo_coord(&b.west_bound_longitude)),
            geo_location_box_east_longitude: g.geo_location_box.as_ref().and_then(|b| parse_geo_coord(&b.east_bound_longitude)),
            geo_location_box_south_latitude: g.geo_location_box.as_ref().and_then(|b| parse_geo_coord(&b.south_bound_latitude)),
            geo_location_box_north_latitude: g.geo_location_box.as_ref().and_then(|b| parse_geo_coord(&b.north_bound_latitude)),
        });
    }

    // Identifiers: alternateIdentifiers first, then the DOI
    for id in attr.alternate_identifiers {
        if id.alternate_identifier.is_empty() {
            continue;
        }
        data.identifiers.push(Identifier {
            identifier: id.alternate_identifier,
            identifier_type: id.alternate_identifier_type,
        });
    }
    if !data.identifiers.iter().any(|i| i.identifier == doi_id) {
        data.identifiers.push(Identifier {
            identifier: doi_id.clone(),
            identifier_type: "DOI".to_string(),
        });
    }

    // Publisher: String (old) or struct (new)
    if let Some(pub_val) = attr.publisher {
        if let Some(name) = pub_val.as_str() {
            data.publisher = Publisher {
                name: name.to_string(),
                ..Default::default()
            };
        } else if let Ok(p) = serde_json::from_value::<DcPublisherStruct>(pub_val)
            && !p.name.is_empty()
        {
            data.publisher = Publisher {
                id: normalize_ror(&p.publisher_identifier),
                name: p.name,
                asserted_by: String::new(),
            };
        }
    }

    // Subjects (deduplicated)
    for s in attr.subjects {
        let subject = Subject { subject: s.subject, ..Default::default() };
        if !data.subjects.contains(&subject) {
            data.subjects.push(subject);
        }
    }

    data.language = attr.language.unwrap_or_default();

    // License: use first entry only
    if let Some(r) = attr.rights_list.into_iter().next() {
        let (url, ok) = normalize_cc_url(&r.rights_uri);
        let url = if ok { url } else { r.rights_uri.clone() };
        data.license = crate::spdx::from_url(&url);
    }

    data.provider = "DataCite".to_string();

    // References (Cites / References relation types)
    for r in &attr.related_identifiers {
        let id = normalize_id(&r.related_identifier);
        if !id.is_empty() && is_reference_relation(&r.relation_type) {
            let type_ = C::dc_to_cm(&r.resource_type_general).to_string();
            data.references.push(Reference {
                id,
                type_,
                ..Default::default()
            });
        }
    }

    // Citations (works that cite this resource: IsCitedBy / IsReferencedBy)
    for r in &attr.related_identifiers {
        let id = normalize_id(&r.related_identifier);
        if !id.is_empty() && is_citation_relation(&r.relation_type) {
            data.citations.push(Citation {
                id,
                ..Default::default()
            });
        }
    }

    // Relations
    for r in &attr.related_identifiers {
        let id = normalize_id(&r.related_identifier);
        if !id.is_empty() && is_supported_relation(&r.relation_type) {
            let mapped = dc_to_cm_relation(&r.relation_type);
            let type_ = if mapped.is_empty() {
                r.relation_type.clone()
            } else {
                mapped.to_string()
            };
            let relation = Relation { id, type_ };
            if !data.relations.contains(&relation) {
                data.relations.push(relation);
            }
        }
    }

    // Titles: first main title → data.title; rest → additional_titles
    for t in attr.titles {
        let type_ = match t.title_type.as_str() {
            "MainTitle" | "Subtitle" | "TranslatedTitle" => t.title_type,
            _ => String::new(),
        };
        if data.title.is_empty() && (type_.is_empty() || type_ == "MainTitle") {
            data.title = t.title;
        } else {
            data.additional_titles.push(Title {
                title: t.title,
                type_,
                language: t.lang,
            });
        }
    }

    // URL
    if let Some(url) = attr.url {
        data.url = normalize_url(&url, true, false).unwrap_or(url);
    }

    data.version = attr.version.unwrap_or_default();

    data
}

// ── Public API ─────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let response: DcResponse =
        serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_attributes(response.data.attributes))
}

pub fn fetch(doi: &str) -> Result<Data> {
    let bare = validate_doi(doi).ok_or_else(|| Error::Parse("invalid DOI".to_string()))?;
    let url = format!("https://api.datacite.org/dois/{}?affiliation=true", bare);
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

// ── Writer ─────────────────────────────────────────────────────────────────────

// ── Output structs ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct OutPayload {
    doi: String,
    types: OutTypes,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    creators: Vec<OutContributor>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    contributors: Vec<OutContributor>,
    publisher: OutPublisher,
    #[serde(rename = "publicationYear", skip_serializing_if = "Option::is_none")]
    publication_year: Option<i32>,
    titles: Vec<OutTitle>,
    url: String,
    #[serde(skip_serializing_if = "OutContainer::is_empty")]
    container: OutContainer,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    identifiers: Vec<OutIdentifier>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    dates: Vec<OutDate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    descriptions: Vec<OutDescription>,
    #[serde(rename = "fundingReferences", skip_serializing_if = "Vec::is_empty")]
    funding_references: Vec<OutFundingReference>,
    #[serde(rename = "geoLocations", skip_serializing_if = "Vec::is_empty")]
    geo_locations: Vec<OutGeoLocation>,
    #[serde(skip_serializing_if = "String::is_empty")]
    language: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    subjects: Vec<OutSubject>,
    #[serde(rename = "rightsList", skip_serializing_if = "Vec::is_empty")]
    rights_list: Vec<OutRights>,
    #[serde(rename = "relatedIdentifiers", skip_serializing_if = "Vec::is_empty")]
    related_identifiers: Vec<OutRelatedIdentifier>,
    #[serde(skip_serializing_if = "String::is_empty")]
    version: String,
    // DataCite event to trigger DOI state transition
    event: &'static str,
}

#[derive(Serialize)]
struct OutTypes {
    #[serde(rename = "resourceTypeGeneral")]
    resource_type_general: String,
    #[serde(rename = "resourceType", skip_serializing_if = "String::is_empty")]
    resource_type: String,
    #[serde(rename = "schemaOrg", skip_serializing_if = "String::is_empty")]
    schema_org: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    citeproc: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    bibtex: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    ris: String,
}

#[derive(Serialize)]
struct OutContributor {
    #[serde(skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(rename = "givenName", skip_serializing_if = "String::is_empty")]
    given_name: String,
    #[serde(rename = "familyName", skip_serializing_if = "String::is_empty")]
    family_name: String,
    #[serde(rename = "nameType")]
    name_type: String,
    #[serde(rename = "nameIdentifiers", skip_serializing_if = "Vec::is_empty")]
    name_identifiers: Vec<OutNameIdentifier>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    affiliation: Vec<String>,
    #[serde(rename = "contributorType", skip_serializing_if = "String::is_empty")]
    contributor_type: String,
}

#[derive(Serialize)]
struct OutNameIdentifier {
    #[serde(rename = "nameIdentifier")]
    name_identifier: String,
    #[serde(rename = "nameIdentifierScheme")]
    name_identifier_scheme: &'static str,
    #[serde(rename = "schemeUri")]
    scheme_uri: &'static str,
}

#[derive(Serialize)]
struct OutPublisher {
    name: String,
}

#[derive(Serialize, Default)]
struct OutContainer {
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    type_: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    identifier: String,
    #[serde(rename = "identifierType", skip_serializing_if = "String::is_empty")]
    identifier_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    volume: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    issue: String,
    #[serde(rename = "firstPage", skip_serializing_if = "String::is_empty")]
    first_page: String,
    #[serde(rename = "lastPage", skip_serializing_if = "String::is_empty")]
    last_page: String,
}

impl OutContainer {
    fn is_empty(&self) -> bool {
        self.type_.is_empty()
            && self.identifier.is_empty()
            && self.identifier_type.is_empty()
            && self.title.is_empty()
            && self.volume.is_empty()
            && self.issue.is_empty()
            && self.first_page.is_empty()
            && self.last_page.is_empty()
    }
}

#[derive(Serialize)]
struct OutTitle {
    title: String,
    #[serde(rename = "titleType", skip_serializing_if = "String::is_empty")]
    title_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    lang: String,
}

#[derive(Serialize)]
struct OutIdentifier {
    identifier: String,
    #[serde(rename = "identifierType")]
    identifier_type: String,
}

#[derive(Serialize)]
struct OutDate {
    date: String,
    #[serde(rename = "dateType")]
    date_type: &'static str,
}

#[derive(Serialize)]
struct OutDescription {
    description: String,
    #[serde(rename = "descriptionType")]
    description_type: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    lang: String,
}

#[derive(Serialize)]
struct OutFundingReference {
    #[serde(rename = "funderName")]
    funder_name: String,
    #[serde(rename = "funderIdentifier", skip_serializing_if = "String::is_empty")]
    funder_identifier: String,
    #[serde(
        rename = "funderIdentifierType",
        skip_serializing_if = "String::is_empty"
    )]
    funder_identifier_type: String,
    #[serde(rename = "awardNumber", skip_serializing_if = "String::is_empty")]
    award_number: String,
    #[serde(rename = "awardUri", skip_serializing_if = "String::is_empty")]
    award_uri: String,
}

#[derive(Serialize)]
struct OutGeoLocation {
    #[serde(rename = "geoLocationPlace", skip_serializing_if = "String::is_empty")]
    geo_location_place: String,
    #[serde(
        rename = "geoLocationPoint",
        skip_serializing_if = "OutGeoPoint::is_empty"
    )]
    geo_location_point: OutGeoPoint,
    #[serde(rename = "geoLocationBox", skip_serializing_if = "OutGeoBox::is_empty")]
    geo_location_box: OutGeoBox,
}

#[derive(Serialize, Default)]
struct OutGeoPoint {
    #[serde(rename = "pointLongitude", skip_serializing_if = "Option::is_none")]
    point_longitude: Option<f64>,
    #[serde(rename = "pointLatitude", skip_serializing_if = "Option::is_none")]
    point_latitude: Option<f64>,
}

impl OutGeoPoint {
    fn is_empty(&self) -> bool {
        self.point_longitude.is_none() && self.point_latitude.is_none()
    }
}

#[derive(Serialize, Default)]
struct OutGeoBox {
    #[serde(rename = "westBoundLongitude", skip_serializing_if = "Option::is_none")]
    west_bound_longitude: Option<f64>,
    #[serde(rename = "eastBoundLongitude", skip_serializing_if = "Option::is_none")]
    east_bound_longitude: Option<f64>,
    #[serde(rename = "southBoundLatitude", skip_serializing_if = "Option::is_none")]
    south_bound_latitude: Option<f64>,
    #[serde(rename = "northBoundLatitude", skip_serializing_if = "Option::is_none")]
    north_bound_latitude: Option<f64>,
}

impl OutGeoBox {
    fn is_empty(&self) -> bool {
        self.west_bound_longitude.is_none()
            && self.east_bound_longitude.is_none()
            && self.south_bound_latitude.is_none()
            && self.north_bound_latitude.is_none()
    }
}

#[derive(Serialize)]
struct OutSubject {
    subject: String,
}

#[derive(Serialize)]
struct OutRights {
    #[serde(rename = "rightsUri")]
    rights_uri: String,
    #[serde(rename = "rightsIdentifier", skip_serializing_if = "String::is_empty")]
    rights_identifier: String,
    #[serde(rename = "rightsIdentifierScheme")]
    rights_identifier_scheme: &'static str,
    #[serde(rename = "schemeUri")]
    scheme_uri: &'static str,
}

#[derive(Serialize)]
struct OutRelatedIdentifier {
    #[serde(rename = "relatedIdentifier")]
    related_identifier: String,
    #[serde(rename = "relatedIdentifierType")]
    related_identifier_type: String,
    #[serde(rename = "relationType")]
    relation_type: String,
    #[serde(
        rename = "resourceTypeGeneral",
        skip_serializing_if = "String::is_empty"
    )]
    resource_type_general: String,
}

// ── Type mappings (CM → DC) ───────────────────────────────────────────────────

fn cm_to_dc_relation(cm: &str) -> &'static str {
    match cm {
        "IsReviewOf" => "Reviews",
        "HasReview" => "IsReviewedBy",
        _ => "",
    }
}

fn doi_from_identifiers(data: &Data) -> Option<String> {
    data.identifiers
        .iter()
        .find(|i| i.identifier_type == "DOI" && !i.identifier.is_empty())
        .map(|i| i.identifier.clone())
}

// ── Conversion ────────────────────────────────────────────────────────────────

fn convert(data: &Data) -> OutPayload {
    // DOI
    let doi_source = doi_from_identifiers(data).unwrap_or_else(|| data.id.clone());
    let doi = doi_source
        .trim_start_matches("https://doi.org/")
        .to_string();

    // Types
    let resource_type_general = {
        let mapped = C::cm_to_dc(&data.type_);
        if mapped.is_empty() {
            "Other".to_string()
        } else {
            mapped.to_string()
        }
    };
    let resource_type = if data.type_ == "BlogPost" {
        "BlogPost".to_string()
    } else if !data.additional_type.is_empty() {
        data.additional_type.clone()
    } else {
        String::new()
    };
    let types = OutTypes {
        resource_type_general,
        resource_type,
        schema_org: C::cm_to_schema_org(&data.type_).to_string(),
        citeproc: C::cm_to_citeproc(&data.type_).to_string(),
        bibtex: C::cm_to_bib(&data.type_).to_string(),
        ris: C::cm_to_ris(&data.type_).to_string(),
    };

    // Publication year
    let publication_year: Option<i32> = if data.date_published.len() >= 4 {
        data.date_published[..4].parse().ok()
    } else {
        None
    };

    // Titles
    let mut titles: Vec<OutTitle> = Vec::new();
    if !data.title.is_empty() {
        titles.push(OutTitle { title: data.title.clone(), title_type: String::new(), lang: String::new() });
    }
    for t in &data.additional_titles {
        titles.push(OutTitle { title: t.title.clone(), title_type: t.type_.clone(), lang: t.language.clone() });
    }

    // Contributors → split into creators (Author role) and contributors (other roles)
    let mut creators: Vec<OutContributor> = Vec::new();
    let mut contribs: Vec<OutContributor> = Vec::new();

    for v in &data.contributors {
        let name = if !v.given_name().is_empty() || !v.family_name().is_empty() {
            format!("{}, {}", v.given_name(), v.family_name())
        } else {
            v.name()
        };

        let v_id = v.id();
        let name_identifiers = if !v_id.is_empty() {
            let (scheme, scheme_uri): (&'static str, &'static str) =
                if v_id.starts_with("https://orcid.org/") {
                    ("ORCID", "https://orcid.org")
                } else if v_id.starts_with("https://ror.org/") {
                    ("ROR", "https://ror.org")
                } else {
                    ("URL", "")
                };
            vec![OutNameIdentifier {
                name_identifier: v_id.to_string(),
                name_identifier_scheme: scheme,
                scheme_uri,
            }]
        } else {
            vec![]
        };

        let affiliation: Vec<String> = v
            .affiliations()
            .iter()
            .filter(|a| !a.name.is_empty())
            .map(|a| a.name.clone())
            .collect();

        // DataCite nameType is "Personal" / "Organizational" (add "al" suffix)
        let name_type = match v.type_.as_str() {
            "Person" => "Personal".to_string(),
            "Organization" => "Organizational".to_string(),
            other => other.to_string(),
        };

        let is_author = v.roles.contains(&"Author".to_string());
        if is_author {
            creators.push(OutContributor {
                name,
                given_name: v.given_name().to_string(),
                family_name: v.family_name().to_string(),
                name_type,
                name_identifiers,
                affiliation,
                contributor_type: String::new(),
            });
        } else {
            let contributor_type = v.roles.first().cloned().unwrap_or_default();
            contribs.push(OutContributor {
                name,
                given_name: v.given_name().to_string(),
                family_name: v.family_name().to_string(),
                name_type,
                name_identifiers,
                affiliation,
                contributor_type,
            });
        }
    }

    // Publisher
    let publisher = OutPublisher {
        name: data.publisher.name.clone(),
    };

    // URL
    let url = data.url.clone();

    // Container
    let container = OutContainer {
        type_: data.container.type_.clone(),
        identifier: data.container.identifier.clone(),
        identifier_type: data.container.identifier_type.clone(),
        title: data.container.title.clone(),
        volume: data.container.volume.clone(),
        issue: data.container.issue.clone(),
        first_page: data.container.first_page.clone(),
        last_page: data.container.last_page.clone(),
    };

    // Alternate identifiers (exclude the DOI itself)
    let doi_id = normalize_doi(&data.id);
    let identifiers: Vec<OutIdentifier> = data
        .identifiers
        .iter()
        .filter(|i| i.identifier != doi_id)
        .map(|i| OutIdentifier {
            identifier: i.identifier.clone(),
            identifier_type: i.identifier_type.clone(),
        })
        .collect();

    // Emit all relevant date fields present in the record.
    let mut dates: Vec<OutDate> = Vec::new();
    if !data.dates.created.is_empty() {
        dates.push(OutDate { date: data.dates.created.clone(), date_type: "Created" });
    }
    if !data.dates.submitted.is_empty() {
        dates.push(OutDate { date: data.dates.submitted.clone(), date_type: "Submitted" });
    }
    if !data.dates.accepted.is_empty() {
        dates.push(OutDate { date: data.dates.accepted.clone(), date_type: "Accepted" });
    }
    if !data.date_published.is_empty() {
        dates.push(OutDate { date: data.date_published.clone(), date_type: "Issued" });
    }
    if !data.date_updated.is_empty() {
        dates.push(OutDate { date: data.date_updated.clone(), date_type: "Updated" });
    }
    if !data.dates.accessed.is_empty() {
        dates.push(OutDate { date: data.dates.accessed.clone(), date_type: "Accessed" });
    }
    if !data.dates.available.is_empty() {
        dates.push(OutDate { date: data.dates.available.clone(), date_type: "Available" });
    }
    if !data.dates.collected.is_empty() {
        dates.push(OutDate { date: data.dates.collected.clone(), date_type: "Collected" });
    }
    if !data.dates.valid.is_empty() {
        dates.push(OutDate { date: data.dates.valid.clone(), date_type: "Valid" });
    }
    if !data.dates.withdrawn.is_empty() {
        dates.push(OutDate { date: data.dates.withdrawn.clone(), date_type: "Withdrawn" });
    }
    if !data.dates.other.is_empty() {
        dates.push(OutDate { date: data.dates.other.clone(), date_type: "Other" });
    }

    // Descriptions
    let mut descriptions: Vec<OutDescription> = Vec::new();
    if !data.description.is_empty() {
        descriptions.push(OutDescription {
            description: data.description.clone(),
            description_type: String::new(),
            lang: String::new(),
        });
    }
    for d in &data.additional_descriptions {
        descriptions.push(OutDescription {
            description: d.description.clone(),
            description_type: d.type_.clone(),
            lang: d.language.clone(),
        });
    }

    // Funding references
    let funding_references: Vec<OutFundingReference> = data
        .funding_references
        .iter()
        .map(|f| {
            let funder_identifier_type = if f.funder_id.starts_with("https://ror.org/") {
                "ROR".to_string()
            } else if f.funder_id.starts_with("https://doi.org/10.13039/") {
                "Crossref Funder ID".to_string()
            } else if !f.funder_id.is_empty() {
                "Other".to_string()
            } else {
                String::new()
            };
            OutFundingReference {
                funder_name: f.funder_name.clone(),
                funder_identifier: f.funder_id.clone(),
                funder_identifier_type,
                award_number: f.award_number.clone(),
                award_uri: f.award_id.clone(),
            }
        })
        .collect();

    // GeoLocations
    let geo_locations: Vec<OutGeoLocation> = data
        .geo_locations
        .iter()
        .map(|g| OutGeoLocation {
            geo_location_place: g.geo_location_place.clone(),
            geo_location_point: OutGeoPoint {
                point_longitude: g.geo_location_point_longitude,
                point_latitude: g.geo_location_point_latitude,
            },
            geo_location_box: OutGeoBox {
                west_bound_longitude: g.geo_location_box_west_longitude,
                east_bound_longitude: g.geo_location_box_east_longitude,
                south_bound_latitude: g.geo_location_box_south_latitude,
                north_bound_latitude: g.geo_location_box_north_latitude,
            },
        })
        .collect();

    // Subjects
    let subjects: Vec<OutSubject> = data
        .subjects
        .iter()
        .map(|s| OutSubject {
            subject: s.subject.clone(),
        })
        .collect();

    // License
    let rights_list: Vec<OutRights> = if !data.license.url.is_empty() {
        let rights_identifier = data.license.id.to_lowercase();
        vec![OutRights {
            rights_uri: data.license.url.clone(),
            rights_identifier,
            rights_identifier_scheme: "SPDX",
            scheme_uri: "https://spdx.org/licenses/",
        }]
    } else {
        vec![]
    };

    // Related identifiers: relations first, then references
    let mut related_identifiers: Vec<OutRelatedIdentifier> = Vec::new();

    for r in &data.relations {
        let (identifier, identifier_type) = validate_id(&r.id);
        if identifier.is_empty() {
            continue;
        }
        let mapped = cm_to_dc_relation(&r.type_);
        let relation_type = if mapped.is_empty() {
            r.type_.clone()
        } else {
            mapped.to_string()
        };
        related_identifiers.push(OutRelatedIdentifier {
            related_identifier: identifier,
            related_identifier_type: identifier_type.to_string(),
            relation_type,
            resource_type_general: String::new(),
        });
    }

    for r in &data.references {
        let (identifier, identifier_type) = validate_id(&r.id);
        if identifier.is_empty() {
            continue;
        }
        let resource_type_general = C::cm_to_dc(&r.type_).to_string();
        related_identifiers.push(OutRelatedIdentifier {
            related_identifier: identifier,
            related_identifier_type: identifier_type.to_string(),
            relation_type: "References".to_string(),
            resource_type_general,
        });
    }

    OutPayload {
        doi,
        types,
        creators,
        contributors: contribs,
        publisher,
        publication_year,
        titles,
        url,
        container,
        identifiers,
        dates,
        descriptions,
        funding_references,
        geo_locations,
        language: data.language.clone(),
        subjects,
        rights_list,
        related_identifiers,
        version: data.version.clone(),
        event: "publish",
    }
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let payload = convert(data);
    serde_json::to_vec(&payload).map_err(|e| Error::Parse(e.to_string()))
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let payloads: Vec<OutPayload> = list.iter().map(convert).collect();
    serde_json::to_vec_pretty(&payloads).map_err(|e| Error::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real-world VRAIX DataCite dumps use explicit JSON `null` (not a
    /// missing key) for `givenName`/`familyName` on organizational creators,
    /// which `#[serde(default)]` alone does not catch since default only
    /// fires when the key is absent. See e.g. pid 10.25828/jnkz-s804.
    #[test]
    fn test_read_json_tolerates_null_creator_name_fields() {
        let json = r#"{"data":{"id":"10.25828/jnkz-s804","attributes":{
            "doi":"10.25828/jnkz-s804",
            "creators":[{"name":"Some Org","nameType":"Organizational","givenName":null,"familyName":null,"affiliation":[],"nameIdentifiers":[]}],
            "titles":[{"title":"A Title"}]
        }}}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.id, "https://doi.org/10.25828/jnkz-s804");
        assert_eq!(data.contributors[0].name(), "Some Org");
    }

    /// `descriptions[]` entries sometimes carry only a `descriptionType`
    /// with no `description` text at all (missing key, not null).
    #[test]
    fn test_read_json_tolerates_description_without_text() {
        let json = r#"{"data":{"id":"10.1/a","attributes":{
            "doi":"10.1/a",
            "titles":[{"title":"A Title"}],
            "descriptions":[{"descriptionType":"Other"}]
        }}}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.additional_descriptions[0].description, "");
        assert_eq!(data.additional_descriptions[0].type_, "Other");
    }

    #[test]
    fn test_read_json_normalizes_legacy_contributor_role_names() {
        let json = r#"{"data":{"id":"10.1/a","attributes":{
            "doi":"10.1/a",
            "titles":[{"title":"A Title"}],
            "creators":[{"name":"Jane Doe","nameType":"Personal"}],
            "contributors":[{"name":"John Doe","nameType":"Personal","contributorType":"DataCurator"}]
        }}}"#;
        let data = read_json(json).unwrap();
        assert_eq!(data.contributors[1].roles[0], "DataCuration");
    }

    #[test]
    fn test_write_prefers_doi_identifier_over_id() {
        let mut data = Data {
            id: "https://example.org/not-a-doi".to_string(),
            type_: "JournalArticle".to_string(),
            ..Default::default()
        };
        data.identifiers.push(Identifier {
            identifier: "https://doi.org/10.1234/identifier".to_string(),
            identifier_type: "DOI".to_string(),
        });

        let out = write(&data).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["doi"], "10.1234/identifier");
    }
}
