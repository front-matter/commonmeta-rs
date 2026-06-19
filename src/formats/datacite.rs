use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Deserialize a JSON string-or-null as String, treating null as "".
fn null_to_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

use crate::data::{
    Affiliation, Container, Contributor, Data, Description, FundingReference, GeoLocation,
    GeoLocationBox, GeoLocationPoint, Identifier, License, Publisher, Reference, Relation,
    Subject, Title,
};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::{Error, Result};
use crate::utils::{
    normalize_cc_url, normalize_id, normalize_orcid, normalize_ror, normalize_url, sanitize,
    url_to_spdx, validate_id,
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
    #[serde(rename = "alternateIdentifier", default, deserialize_with = "null_to_string")]
    alternate_identifier: String,
    #[serde(rename = "alternateIdentifierType", default, deserialize_with = "null_to_string")]
    alternate_identifier_type: String,
}

// Affiliation is either Vec<String> or Vec<DcAffiliationStruct>; use Value
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
    #[serde(rename = "contributorType", default, deserialize_with = "null_to_string")]
    contributor_type: String,
}

#[derive(Deserialize, Default)]
struct DcNameIdentifier {
    #[serde(rename = "nameIdentifier", default, deserialize_with = "null_to_string")]
    name_identifier: String,
    #[serde(rename = "nameIdentifierScheme", default, deserialize_with = "null_to_string")]
    name_identifier_scheme: String,
}

#[derive(Deserialize, Default)]
struct DcAffiliationStruct {
    #[serde(rename = "affiliationIdentifier", default, deserialize_with = "null_to_string")]
    affiliation_identifier: String,
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
}

#[derive(Deserialize, Default)]
struct DcPublisherStruct {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(rename = "publisherIdentifier", default, deserialize_with = "null_to_string")]
    publisher_identifier: String,
}

#[derive(Deserialize, Default)]
struct DcContainer {
    #[serde(rename = "type", default, deserialize_with = "null_to_string")]
    type_: String,
    #[serde(default, deserialize_with = "null_to_string")]
    identifier: String,
    #[serde(rename = "identifierType", default, deserialize_with = "null_to_string")]
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
    #[serde(default, deserialize_with = "null_to_string")]
    date: String,
    #[serde(rename = "dateType", default, deserialize_with = "null_to_string")]
    date_type: String,
}

#[derive(Deserialize, Default)]
struct DcTypes {
    #[serde(rename = "resourceTypeGeneral", default, deserialize_with = "null_to_string")]
    resource_type_general: String,
    #[serde(rename = "resourceType", default, deserialize_with = "null_to_string")]
    resource_type: String,
}

#[derive(Deserialize, Default)]
struct DcRelatedIdentifier {
    #[serde(rename = "relatedIdentifier", default, deserialize_with = "null_to_string")]
    related_identifier: String,
    #[serde(rename = "relationType", default, deserialize_with = "null_to_string")]
    relation_type: String,
    #[serde(rename = "resourceTypeGeneral", default, deserialize_with = "null_to_string")]
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
    #[serde(rename = "descriptionType", default, deserialize_with = "null_to_string")]
    description_type: String,
    #[serde(default, deserialize_with = "null_to_string")]
    lang: String,
}

#[derive(Deserialize, Default)]
struct DcGeoLocation {
    #[serde(rename = "geoLocationPlace", default, deserialize_with = "null_to_string")]
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
    #[serde(rename = "funderIdentifier", default, deserialize_with = "null_to_string")]
    funder_identifier: String,
    #[serde(rename = "funderIdentifierType", default, deserialize_with = "null_to_string")]
    funder_identifier_type: String,
    #[serde(rename = "awardNumber", default, deserialize_with = "null_to_string")]
    award_number: String,
    #[serde(rename = "awardTitle", default, deserialize_with = "null_to_string")]
    award_title: String,
    #[serde(rename = "awardUri", default, deserialize_with = "null_to_string")]
    award_uri: String,
}

// ── Type mapping ───────────────────────────────────────────────────────────────

fn dc_to_cm_type(dc: &str) -> &'static str {
    match dc {
        "Audiovisual"           => "Audiovisual",
        "BlogPosting"           => "BlogPost",
        "Book"                  => "Book",
        "BookChapter"           => "BookChapter",
        "Collection"            => "Collection",
        "ComputationalNotebook" => "ComputationalNotebook",
        "ConferencePaper"       => "ProceedingsArticle",
        "ConferenceProceeding"  => "Proceedings",
        "DataPaper"             => "JournalArticle",
        "Dataset"               => "Dataset",
        "Dissertation"          => "Dissertation",
        "Event"                 => "Event",
        "Image"                 => "Image",
        "Instrument"            => "Instrument",
        "InteractiveResource"   => "InteractiveResource",
        "Journal"               => "Journal",
        "JournalArticle"        => "JournalArticle",
        "Model"                 => "Model",
        "OutputManagementPlan"  => "OutputManagementPlan",
        "PeerReview"            => "PeerReview",
        "PhysicalObject"        => "PhysicalObject",
        "Poster"                => "Poster",
        "Preprint"              => "Article",
        "Report"                => "Report",
        "Service"               => "Service",
        "Software"              => "Software",
        "Sound"                 => "Sound",
        "Standard"              => "Standard",
        "StudyRegistration"     => "StudyRegistration",
        "Text"                  => "Document",
        "Thesis"                => "Dissertation",
        "Workflow"              => "Workflow",
        "Other"                 => "Other",
        _                       => "",
    }
}

fn dc_to_cm_relation(rt: &str) -> &'static str {
    match rt {
        "Reviews"      => "IsReviewOf",
        "IsReviewedBy" => "HasReview",
        _              => "",
    }
}

fn parse_geo_coord(v: &Option<Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn is_reference_relation(rt: &str) -> bool {
    matches!(rt, "Cites" | "References")
}

fn is_supported_relation(rt: &str) -> bool {
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

fn is_recognized_role(role: &str) -> bool {
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
            | "WorkPackageLeader"
            | "Other"
    )
}

// ── Contributor conversion ─────────────────────────────────────────────────────

fn get_contributor(v: DcContributor, default_role: &str) -> Contributor {
    // DataCite uses "Personal"/"Organizational"; strip last 2 chars → "Person"/"Organization"
    let mut name_type = match v.name_type.as_str() {
        "Personal" | "Organizational" => v.name_type[..v.name_type.len() - 2].to_string(),
        "Person" | "Organization" => v.name_type.clone(),
        _ => String::new(),
    };

    let mut id = String::new();
    for ni in &v.name_identifiers {
        if ni.name_identifier_scheme == "ORCID"
            || ni.name_identifier_scheme.contains("orcid.org")
        {
            id = normalize_orcid(&ni.name_identifier);
            name_type = "Person".to_string();
            break;
        } else if ni.name_identifier_scheme == "ROR" {
            id = normalize_ror(&ni.name_identifier);
            name_type = "Organization".to_string();
            break;
        }
    }

    let mut name = v.name;
    let mut given_name = v.given_name;
    let mut family_name = v.family_name;

    if name_type.is_empty() {
        name_type = if !given_name.is_empty() || !family_name.is_empty() {
            "Person".to_string()
        } else {
            "Organization".to_string()
        };
    }

    // Split "Family, Given" format for Person type
    if name_type == "Person" && !name.is_empty() && given_name.is_empty() && family_name.is_empty()
        && let Some(comma) = name.find(',') {
            given_name = name[comma + 1..].trim().to_string();
            family_name = name[..comma].trim().to_string();
            name = String::new();
        }

    // Affiliation: Vec<String> (old) or Vec<struct> (new)
    let affiliations: Vec<Affiliation> = if let Some(aff_val) = v.affiliation {
        if let Ok(names) = serde_json::from_value::<Vec<String>>(aff_val.clone()) {
            names
                .into_iter()
                .filter(|n| !n.is_empty())
                .map(|n| Affiliation { name: n, ..Default::default() })
                .collect()
        } else if let Ok(structs) =
            serde_json::from_value::<Vec<DcAffiliationStruct>>(aff_val)
        {
            structs
                .into_iter()
                .map(|a| Affiliation {
                    id: normalize_ror(&a.affiliation_identifier),
                    name: a.name,
                    ..Default::default()
                })
                .collect()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    Contributor {
        id,
        type_: name_type,
        name,
        given_name,
        family_name,
        affiliations,
        contributor_roles: vec![default_role.to_string()],
    }
}

// ── Core conversion ────────────────────────────────────────────────────────────

fn from_attributes(attr: DcAttributes) -> Data {
    let mut data = Data::default();

    let doi_id = normalize_doi(&attr.doi);
    data.id = doi_id.clone();

    // Type
    let types = attr.types.unwrap_or_default();
    data.type_ = dc_to_cm_type(&types.resource_type_general).to_string();

    // resourceType overrides resourceTypeGeneral when it maps to a CM type (schema 4.4+)
    let additional = dc_to_cm_type(&types.resource_type);
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
        let id = contrib.id.clone();
        if id.is_empty() || !data.contributors.iter().any(|c| !c.id.is_empty() && c.id == id) {
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
        let id = contrib.id.clone();
        if id.is_empty() || !data.contributors.iter().any(|c| !c.id.is_empty() && c.id == id) {
            data.contributors.push(contrib);
        }
    }

    // Dates
    for d in attr.dates {
        match d.date_type.as_str() {
            "Accepted"           => data.date.accepted   = d.date,
            "Available"          => data.date.available  = d.date,
            "Collected"          => data.date.collected  = d.date,
            "Created"            => data.date.created    = d.date,
            "Issued" | "Published" => data.date.published = d.date,
            "Submitted"          => data.date.submitted  = d.date,
            "Updated"            => data.date.updated    = d.date,
            "Valid"              => data.date.valid      = d.date,
            "Withdrawn"          => data.date.withdrawn  = d.date,
            "Other"              => data.date.other      = d.date,
            _ => {}
        }
    }
    // Fall back to publicationYear
    if data.date.published.is_empty()
        && let Some(py) = attr.publication_year {
            data.date.published = match py {
                Value::Number(n) => n.as_i64().map(|y| y.to_string()).unwrap_or_default(),
                Value::String(s) => s,
                _ => String::new(),
            };
        }

    // Descriptions
    for d in attr.descriptions {
        let type_ = match d.description_type.as_str() {
            "Abstract" | "Summary" | "Methods" | "TechnicalInfo" | "Other" => {
                d.description_type.clone()
            }
            _ => "Other".to_string(),
        };
        data.descriptions.push(Description {
            description: sanitize(&d.description),
            type_,
            language: d.lang,
        });
    }

    // Funding references — ROR identifier pass-through; skip live lookups
    for f in attr.funding_references {
        let (funder_id, funder_id_type) = if f.funder_identifier_type == "ROR" {
            let id = normalize_ror(&f.funder_identifier);
            (id, "ROR".to_string())
        } else {
            (String::new(), String::new())
        };
        data.funding_references.push(FundingReference {
            funder_identifier: funder_id,
            funder_identifier_type: funder_id_type,
            funder_name: f.funder_name,
            award_number: f.award_number,
            award_title: f.award_title,
            award_uri: f.award_uri,
        });
    }

    // GeoLocations
    for g in attr.geo_locations {
        let point = g.geo_location_point.as_ref().map_or_else(GeoLocationPoint::default, |p| {
            GeoLocationPoint {
                point_longitude: parse_geo_coord(&p.point_longitude),
                point_latitude: parse_geo_coord(&p.point_latitude),
            }
        });
        let box_ = g.geo_location_box.as_ref().map_or_else(GeoLocationBox::default, |b| {
            GeoLocationBox {
                west_bound_longitude: parse_geo_coord(&b.west_bound_longitude),
                east_bound_longitude: parse_geo_coord(&b.east_bound_longitude),
                south_bound_latitude: parse_geo_coord(&b.south_bound_latitude),
                north_bound_latitude: parse_geo_coord(&b.north_bound_latitude),
            }
        });
        data.geo_locations.push(GeoLocation {
            geo_location_place: g.geo_location_place,
            geo_location_point: point,
            geo_location_box: box_,
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
            data.publisher = Publisher { name: name.to_string(), ..Default::default() };
        } else if let Ok(p) = serde_json::from_value::<DcPublisherStruct>(pub_val)
            && !p.name.is_empty() {
                data.publisher = Publisher {
                    id: normalize_ror(&p.publisher_identifier),
                    name: p.name,
                };
            }
    }

    // Subjects (deduplicated)
    for s in attr.subjects {
        let subject = Subject { subject: s.subject };
        if !data.subjects.contains(&subject) {
            data.subjects.push(subject);
        }
    }

    data.language = attr.language.unwrap_or_default();

    // License: use first entry only
    if let Some(r) = attr.rights_list.into_iter().next() {
        let (url, _) = normalize_cc_url(&r.rights_uri);
        let id = url_to_spdx(&url);
        data.license = License { id, url };
    }

    data.provider = "DataCite".to_string();

    // References (Cites / References relation types)
    for r in &attr.related_identifiers {
        let id = normalize_id(&r.related_identifier);
        if !id.is_empty() && is_reference_relation(&r.relation_type) {
            let type_ = dc_to_cm_type(&r.resource_type_general).to_string();
            data.references.push(Reference { id, type_, ..Default::default() });
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

    // Titles
    for t in attr.titles {
        let type_ = match t.title_type.as_str() {
            "MainTitle" | "Subtitle" | "TranslatedTitle" => t.title_type,
            _ => String::new(),
        };
        data.titles.push(Title { title: t.title, type_, language: t.lang });
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
    let bare =
        validate_doi(doi).ok_or_else(|| Error::Parse("invalid DOI".to_string()))?;
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
    #[serde(skip_serializing_if = "String::is_empty")]
    title: String,
}

impl OutContainer {
    fn is_empty(&self) -> bool {
        self.title.is_empty()
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
    #[serde(rename = "funderIdentifierType", skip_serializing_if = "String::is_empty")]
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
    #[serde(rename = "geoLocationPoint", skip_serializing_if = "OutGeoPoint::is_empty")]
    geo_location_point: OutGeoPoint,
    #[serde(rename = "geoLocationBox", skip_serializing_if = "OutGeoBox::is_empty")]
    geo_location_box: OutGeoBox,
}

#[derive(Serialize, Default)]
struct OutGeoPoint {
    #[serde(rename = "pointLongitude")]
    point_longitude: f64,
    #[serde(rename = "pointLatitude")]
    point_latitude: f64,
}

impl OutGeoPoint {
    fn is_empty(&self) -> bool {
        self.point_longitude == 0.0 && self.point_latitude == 0.0
    }
}

#[derive(Serialize, Default)]
struct OutGeoBox {
    #[serde(rename = "westBoundLongitude")]
    west_bound_longitude: f64,
    #[serde(rename = "eastBoundLongitude")]
    east_bound_longitude: f64,
    #[serde(rename = "southBoundLatitude")]
    south_bound_latitude: f64,
    #[serde(rename = "northBoundLatitude")]
    north_bound_latitude: f64,
}

impl OutGeoBox {
    fn is_empty(&self) -> bool {
        self.west_bound_longitude == 0.0
            && self.east_bound_longitude == 0.0
            && self.south_bound_latitude == 0.0
            && self.north_bound_latitude == 0.0
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
    #[serde(rename = "resourceTypeGeneral", skip_serializing_if = "String::is_empty")]
    resource_type_general: String,
}

// ── Type mappings (CM → DC) ───────────────────────────────────────────────────

fn cm_to_dc_type(cm: &str) -> &'static str {
    match cm {
        "Article"               => "Preprint",
        "Audiovisual"           => "Audiovisual",
        "BlogPost"              => "Preprint",
        "Book"                  => "Book",
        "BookChapter"           => "BookChapter",
        "Collection"            => "Collection",
        "ComputationalNotebook" => "ComputationalNotebook",
        "Dataset"               => "Dataset",
        "Dissertation"          => "Dissertation",
        "Document"              => "Text",
        "Entry"                 => "Text",
        "Event"                 => "Event",
        "Figure"                => "Image",
        "Image"                 => "Image",
        "Instrument"            => "Instrument",
        "InteractiveResource"   => "InteractiveResource",
        "JournalArticle"        => "JournalArticle",
        "LegalDocument"         => "Text",
        "Manuscript"            => "Text",
        "Map"                   => "Image",
        "Model"                 => "Model",
        "OutputManagementPlan"  => "OutputManagementPlan",
        "Patent"                => "Text",
        "PeerReview"            => "PeerReview",
        "Performance"           => "Audiovisual",
        "PersonalCommunication" => "Text",
        "PhysicalObject"        => "PhysicalObject",
        "Post"                  => "Text",
        "Poster"                => "Poster",
        "Presentation"          => "Audiovisual",
        "ProceedingsArticle"    => "ConferencePaper",
        "Proceedings"           => "ConferenceProceeding",
        "Report"                => "Report",
        "Review"                => "PeerReview",
        "Service"               => "Service",
        "Software"              => "Software",
        "Sound"                 => "Sound",
        "Standard"              => "Standard",
        "StudyRegistration"     => "StudyRegistration",
        "WebPage"               => "Text",
        "Workflow"              => "Workflow",
        _                       => "Other",
    }
}

fn cm_to_schema_org(cm: &str) -> &'static str {
    match cm {
        "Article" | "BlogPost" | "JournalArticle" | "ProceedingsArticle" | "PeerReview" => "ScholarlyArticle",
        "Book"                  => "Book",
        "BookChapter"           => "Chapter",
        "Collection"            => "Collection",
        "ComputationalNotebook" => "SoftwareSourceCode",
        "Dataset"               => "Dataset",
        "Dissertation"          => "Thesis",
        "Document" | "Report"   => "ScholarlyArticle",
        "Event"                 => "Event",
        "Figure" | "Image"      => "ImageObject",
        "Instrument"            => "IndividualProduct",
        "Software"              => "SoftwareSourceCode",
        "Sound"                 => "AudioObject",
        "Audiovisual"           => "VideoObject",
        _                       => "CreativeWork",
    }
}

fn cm_to_citeproc(cm: &str) -> &'static str {
    match cm {
        "Article" | "ProceedingsArticle" => "paper-conference",
        "BlogPost"              => "post-weblog",
        "Book"                  => "book",
        "BookChapter"           => "chapter",
        "ComputationalNotebook" => "article",
        "Dataset"               => "dataset",
        "Dissertation"          => "thesis",
        "Document" | "Report"   => "report",
        "Figure" | "Image"      => "figure",
        "JournalArticle"        => "article-journal",
        "PeerReview"            => "peer-review",
        "Software"              => "software",
        "Sound"                 => "song",
        "Audiovisual"           => "motion_picture",
        _                       => "article",
    }
}

fn cm_to_bibtex(cm: &str) -> &'static str {
    match cm {
        "Article"               => "misc",
        "BlogPost"              => "misc",
        "Book"                  => "book",
        "BookChapter"           => "inbook",
        "ComputationalNotebook" => "misc",
        "Dataset"               => "misc",
        "Dissertation"          => "phdthesis",
        "Document"              => "misc",
        "JournalArticle"        => "article",
        "ProceedingsArticle"    => "inproceedings",
        "Report"                => "techreport",
        "Software"              => "software",
        "Sound"                 => "misc",
        "Audiovisual"           => "misc",
        _                       => "misc",
    }
}

fn cm_to_ris(cm: &str) -> &'static str {
    match cm {
        "Article"               => "JOUR",
        "Audiovisual"           => "VIDEO",
        "BlogPost"              => "BLOG",
        "Book"                  => "BOOK",
        "BookChapter"           => "CHAP",
        "ComputationalNotebook" => "COMP",
        "Dataset"               => "DATA",
        "Dissertation"          => "THES",
        "Document"              => "GEN",
        "JournalArticle"        => "JOUR",
        "ProceedingsArticle"    => "CPAPER",
        "Report"                => "RPRT",
        "Software"              => "COMP",
        "Sound"                 => "SOUND",
        _                       => "GEN",
    }
}

fn cm_to_dc_relation(cm: &str) -> &'static str {
    match cm {
        "IsReviewOf" => "Reviews",
        "HasReview"  => "IsReviewedBy",
        _            => "",
    }
}

// ── Conversion ────────────────────────────────────────────────────────────────

fn convert(data: &Data) -> OutPayload {
    // DOI
    let doi = data
        .id
        .trim_start_matches("https://doi.org/")
        .to_string();

    // Types
    let resource_type_general = {
        let mapped = cm_to_dc_type(&data.type_);
        if mapped.is_empty() { "Other".to_string() } else { mapped.to_string() }
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
        schema_org:  cm_to_schema_org(&data.type_).to_string(),
        citeproc:    cm_to_citeproc(&data.type_).to_string(),
        bibtex:      cm_to_bibtex(&data.type_).to_string(),
        ris:         cm_to_ris(&data.type_).to_string(),
    };

    // Publication year
    let publication_year: Option<i32> = if data.date.published.len() >= 4 {
        data.date.published[..4].parse().ok()
    } else {
        None
    };

    // Titles
    let titles: Vec<OutTitle> = data
        .titles
        .iter()
        .map(|t| OutTitle {
            title: t.title.clone(),
            title_type: t.type_.clone(),
            lang: t.language.clone(),
        })
        .collect();

    // Contributors → split into creators (Author role) and contributors (other roles)
    let mut creators: Vec<OutContributor> = Vec::new();
    let mut contribs: Vec<OutContributor> = Vec::new();

    for v in &data.contributors {
        let name = if !v.name.is_empty() {
            v.name.clone()
        } else {
            // Go joins as "GivenName, FamilyName" — match that behaviour
            format!("{}, {}", v.given_name, v.family_name)
        };

        let name_identifiers = if !v.id.is_empty() {
            // Determine scheme from ID prefix
            let (scheme, scheme_uri): (&'static str, &'static str) =
                if v.id.starts_with("https://orcid.org/") {
                    ("ORCID", "https://orcid.org")
                } else if v.id.starts_with("https://ror.org/") {
                    ("ROR", "https://ror.org")
                } else {
                    ("URL", "")
                };
            vec![OutNameIdentifier {
                name_identifier:       v.id.clone(),
                name_identifier_scheme: scheme,
                scheme_uri,
            }]
        } else {
            vec![]
        };

        let affiliation: Vec<String> = v
            .affiliations
            .iter()
            .filter(|a| !a.name.is_empty())
            .map(|a| a.name.clone())
            .collect();

        // DataCite nameType is "Personal" / "Organizational" (add "al" suffix)
        let name_type = match v.type_.as_str() {
            "Person"       => "Personal".to_string(),
            "Organization" => "Organizational".to_string(),
            other          => other.to_string(),
        };

        let is_author = v.contributor_roles.contains(&"Author".to_string());
        if is_author {
            creators.push(OutContributor {
                name,
                given_name: v.given_name.clone(),
                family_name: v.family_name.clone(),
                name_type,
                name_identifiers,
                affiliation,
                contributor_type: String::new(),
            });
        } else {
            let contributor_type = v.contributor_roles.first().cloned().unwrap_or_default();
            contribs.push(OutContributor {
                name,
                given_name: v.given_name.clone(),
                family_name: v.family_name.clone(),
                name_type,
                name_identifiers,
                affiliation,
                contributor_type,
            });
        }
    }

    // Publisher
    let publisher = OutPublisher { name: data.publisher.name.clone() };

    // URL
    let url = data.url.clone();

    // Container (only title is mapped per the Go code)
    let container = OutContainer { title: data.container.title.clone() };

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

    // Dates — Go uses if-else chain; only the first non-empty date is emitted
    let dates: Vec<OutDate> = {
        let mut v: Vec<OutDate> = Vec::new();
        let d = &data.date;
        if !d.created.is_empty() {
            v.push(OutDate { date: d.created.clone(), date_type: "Created" });
        } else if !d.submitted.is_empty() {
            v.push(OutDate { date: d.submitted.clone(), date_type: "Submitted" });
        } else if !d.accepted.is_empty() {
            v.push(OutDate { date: d.accepted.clone(), date_type: "Accepted" });
        } else if !d.published.is_empty() {
            v.push(OutDate { date: d.published.clone(), date_type: "Issued" });
        } else if !d.updated.is_empty() {
            v.push(OutDate { date: d.updated.clone(), date_type: "Updated" });
        } else if !d.accessed.is_empty() {
            v.push(OutDate { date: d.accessed.clone(), date_type: "Accessed" });
        } else if !d.available.is_empty() {
            v.push(OutDate { date: d.available.clone(), date_type: "Available" });
        } else if !d.collected.is_empty() {
            v.push(OutDate { date: d.collected.clone(), date_type: "Collected" });
        } else if !d.valid.is_empty() {
            v.push(OutDate { date: d.valid.clone(), date_type: "Valid" });
        } else if !d.withdrawn.is_empty() {
            v.push(OutDate { date: d.withdrawn.clone(), date_type: "Withdrawn" });
        } else if !d.other.is_empty() {
            v.push(OutDate { date: d.other.clone(), date_type: "Other" });
        }
        v
    };

    // Descriptions
    let descriptions: Vec<OutDescription> = data
        .descriptions
        .iter()
        .map(|d| OutDescription {
            description: d.description.clone(),
            description_type: d.type_.clone(),
            lang: d.language.clone(),
        })
        .collect();

    // Funding references
    let funding_references: Vec<OutFundingReference> = data
        .funding_references
        .iter()
        .map(|f| OutFundingReference {
            funder_name: f.funder_name.clone(),
            funder_identifier: f.funder_identifier.clone(),
            funder_identifier_type: f.funder_identifier_type.clone(),
            award_number: f.award_number.clone(),
            award_uri: f.award_uri.clone(),
        })
        .collect();

    // GeoLocations
    let geo_locations: Vec<OutGeoLocation> = data
        .geo_locations
        .iter()
        .map(|g| OutGeoLocation {
            geo_location_place: g.geo_location_place.clone(),
            geo_location_point: OutGeoPoint {
                point_longitude: g.geo_location_point.point_longitude,
                point_latitude: g.geo_location_point.point_latitude,
            },
            geo_location_box: OutGeoBox {
                west_bound_longitude: g.geo_location_box.west_bound_longitude,
                east_bound_longitude: g.geo_location_box.east_bound_longitude,
                south_bound_latitude: g.geo_location_box.south_bound_latitude,
                north_bound_latitude: g.geo_location_box.north_bound_latitude,
            },
        })
        .collect();

    // Subjects
    let subjects: Vec<OutSubject> = data
        .subjects
        .iter()
        .map(|s| OutSubject { subject: s.subject.clone() })
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
        let relation_type = if mapped.is_empty() { r.type_.clone() } else { mapped.to_string() };
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
        let resource_type_general = cm_to_dc_type(&r.type_).to_string();
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
        assert_eq!(data.contributors[0].name, "Some Org");
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
        assert_eq!(data.descriptions[0].description, "");
        assert_eq!(data.descriptions[0].type_, "Other");
    }
}
