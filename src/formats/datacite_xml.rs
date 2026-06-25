use quick_xml::de::from_str as xml_from_str;
use serde::Deserialize;

use super::datacite::{
    dc_to_cm_relation, is_recognized_role, is_reference_relation,
    is_supported_relation, normalize_commonmeta_role,
};
use crate::constants as C;
use crate::author_utils::{
    cleanup_author, infer_contributor_type, normalize_contributor_roles, split_person_name,
};
use crate::data::{
    Affiliation, Container, Contributor, Data, Description, FundingReference, GeoLocation,
    Identifier, Organization, Person, Publisher, Reference, Relation, Subject, Title,
};
use crate::doi_utils::{normalize_doi, validate_doi};
use crate::error::{Error, Result};
use crate::utils::{
    normalize_cc_url, normalize_id, normalize_orcid, normalize_ror, sanitize,
};

// ── XML struct definitions ─────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlResource {
    #[serde(rename = "@xmlns", default)]
    xmlns: String,
    #[serde(rename = "identifier", default)]
    identifier: XmlIdentifier,
    #[serde(rename = "creators", default)]
    creators: XmlCreators,
    #[serde(rename = "contributors", default)]
    contributors: XmlContributors,
    #[serde(rename = "titles", default)]
    titles: XmlTitles,
    #[serde(rename = "publisher", default)]
    publisher: Option<XmlPublisher>,
    #[serde(rename = "publicationYear", default)]
    publication_year: String,
    #[serde(rename = "resourceType", default)]
    resource_type: Option<XmlResourceType>,
    #[serde(rename = "subjects", default)]
    subjects: XmlSubjects,
    #[serde(rename = "dates", default)]
    dates: XmlDates,
    #[serde(rename = "language", default)]
    language: String,
    #[serde(rename = "alternateIdentifiers", default)]
    alternate_identifiers: XmlAlternateIdentifiers,
    #[serde(rename = "relatedIdentifiers", default)]
    related_identifiers: XmlRelatedIdentifiers,
    #[serde(rename = "relatedItems", default)]
    related_items: XmlRelatedItems,
    #[serde(rename = "rightsList", default)]
    rights_list: XmlRightsList,
    #[serde(rename = "descriptions", default)]
    descriptions: XmlDescriptions,
    #[serde(rename = "geoLocations", default)]
    geo_locations: XmlGeoLocations,
    #[serde(rename = "fundingReferences", default)]
    funding_references: XmlFundingReferences,
    #[serde(rename = "version", default)]
    version: String,
}

#[derive(Deserialize, Default)]
struct XmlIdentifier {
    #[allow(dead_code)]
    #[serde(rename = "@identifierType", default)]
    identifier_type: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Creators / Contributors ────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlCreators {
    #[serde(rename = "creator", default)]
    creator: Vec<XmlCreator>,
}

#[derive(Deserialize, Default)]
struct XmlCreator {
    #[serde(rename = "creatorName", default)]
    creator_name: XmlCreatorName,
    #[serde(rename = "givenName", default)]
    given_name: String,
    #[serde(rename = "familyName", default)]
    family_name: String,
    #[serde(rename = "nameIdentifier", default)]
    name_identifiers: Vec<XmlNameIdentifier>,
    #[serde(rename = "affiliation", default)]
    affiliations: Vec<XmlAffiliation>,
}

#[derive(Deserialize, Default)]
struct XmlCreatorName {
    #[serde(rename = "@nameType", default)]
    name_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

#[derive(Deserialize, Default)]
struct XmlNameIdentifier {
    #[serde(rename = "@nameIdentifierScheme", default)]
    scheme: String,
    #[serde(rename = "@schemeURI", default)]
    scheme_uri: String,
    #[serde(rename = "$text", default)]
    value: String,
}

#[derive(Deserialize, Default)]
struct XmlAffiliation {
    #[serde(rename = "@affiliationIdentifier", default)]
    id: String,
    #[serde(rename = "@affiliationIdentifierScheme", default)]
    scheme: String,
    #[serde(rename = "$text", default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct XmlContributors {
    #[serde(rename = "contributor", default)]
    contributor: Vec<XmlContributor>,
}

#[derive(Deserialize, Default)]
struct XmlContributor {
    #[serde(rename = "@contributorType", default)]
    contributor_type: String,
    #[serde(rename = "contributorName", default)]
    contributor_name: XmlCreatorName,
    #[serde(rename = "givenName", default)]
    given_name: String,
    #[serde(rename = "familyName", default)]
    family_name: String,
    #[serde(rename = "nameIdentifier", default)]
    name_identifiers: Vec<XmlNameIdentifier>,
    #[serde(rename = "affiliation", default)]
    affiliations: Vec<XmlAffiliation>,
}

// ── Titles ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlTitles {
    #[serde(rename = "title", default)]
    title: Vec<XmlTitle>,
}

#[derive(Deserialize, Default)]
struct XmlTitle {
    #[serde(rename = "@xml:lang", default)]
    lang: String,
    #[serde(rename = "@titleType", default)]
    title_type: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Publisher ─────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlPublisher {
    #[allow(dead_code)]
    #[serde(rename = "@xml:lang", default)]
    lang: String,
    #[serde(rename = "@publisherIdentifier", default)]
    publisher_identifier: String,
    #[serde(rename = "$text", default)]
    name: String,
}

// ── Resource type ─────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlResourceType {
    #[serde(rename = "@resourceTypeGeneral", default)]
    resource_type_general: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Subjects ──────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlSubjects {
    #[serde(rename = "subject", default)]
    subject: Vec<XmlSubject>,
}

#[derive(Deserialize, Default)]
struct XmlSubject {
    #[allow(dead_code)]
    #[serde(rename = "@subjectScheme", default)]
    subject_scheme: String,
    #[allow(dead_code)]
    #[serde(rename = "@xml:lang", default)]
    lang: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Dates ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlDates {
    #[serde(rename = "date", default)]
    date: Vec<XmlDate>,
}

#[derive(Deserialize, Default)]
struct XmlDate {
    #[serde(rename = "@dateType", default)]
    date_type: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Alternate identifiers ─────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlAlternateIdentifiers {
    #[serde(rename = "alternateIdentifier", default)]
    alternate_identifier: Vec<XmlAlternateIdentifier>,
}

#[derive(Deserialize, Default)]
struct XmlAlternateIdentifier {
    #[serde(rename = "@alternateIdentifierType", default)]
    identifier_type: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Related identifiers ───────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlRelatedIdentifiers {
    #[serde(rename = "relatedIdentifier", default)]
    related_identifier: Vec<XmlRelatedIdentifier>,
}

#[derive(Deserialize, Default)]
struct XmlRelatedIdentifier {
    #[allow(dead_code)]
    #[serde(rename = "@relatedIdentifierType", default)]
    identifier_type: String,
    #[serde(rename = "@relationType", default)]
    relation_type: String,
    #[serde(rename = "@resourceTypeGeneral", default)]
    resource_type_general: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Related items (container) ─────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlRelatedItems {
    #[serde(rename = "relatedItem", default)]
    related_item: Vec<XmlRelatedItem>,
}

#[derive(Deserialize, Default)]
struct XmlRelatedItem {
    #[serde(rename = "@relationType", default)]
    relation_type: String,
    #[serde(rename = "@relatedItemType", default)]
    related_item_type: String,
    #[serde(rename = "relatedItemIdentifier", default)]
    identifier: Option<XmlRelatedItemIdentifier>,
    #[serde(rename = "titles", default)]
    titles: XmlTitles,
    #[allow(dead_code)]
    #[serde(rename = "publicationYear", default)]
    publication_year: String,
    #[serde(rename = "volume", default)]
    volume: String,
    #[serde(rename = "issue", default)]
    issue: String,
    #[serde(rename = "firstPage", default)]
    first_page: String,
    #[serde(rename = "lastPage", default)]
    last_page: String,
}

#[derive(Deserialize, Default)]
struct XmlRelatedItemIdentifier {
    #[serde(rename = "@relatedItemIdentifierType", default)]
    identifier_type: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Rights ────────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlRightsList {
    #[serde(rename = "rights", default)]
    rights: Vec<XmlRights>,
}

#[derive(Deserialize, Default)]
struct XmlRights {
    #[serde(rename = "@rightsURI", default)]
    rights_uri: String,
    #[allow(dead_code)]
    #[serde(rename = "$text", default)]
    text: String,
}

// ── Descriptions ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlDescriptions {
    #[serde(rename = "description", default)]
    description: Vec<XmlDescription>,
}

#[derive(Deserialize, Default)]
struct XmlDescription {
    #[serde(rename = "@descriptionType", default)]
    description_type: String,
    #[serde(rename = "@xml:lang", default)]
    lang: String,
    #[serde(rename = "$text", default)]
    text: String,
}

// ── GeoLocations ──────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlGeoLocations {
    #[serde(rename = "geoLocation", default)]
    geo_location: Vec<XmlGeoLocation>,
}

#[derive(Deserialize, Default)]
struct XmlGeoLocation {
    #[serde(rename = "geoLocationPlace", default)]
    geo_location_place: String,
    #[serde(rename = "geoLocationPoint", default)]
    geo_location_point: Option<XmlGeoPoint>,
    #[serde(rename = "geoLocationBox", default)]
    geo_location_box: Option<XmlGeoBox>,
}

#[derive(Deserialize, Default)]
struct XmlGeoPoint {
    #[serde(rename = "pointLongitude", default)]
    point_longitude: String,
    #[serde(rename = "pointLatitude", default)]
    point_latitude: String,
}

#[derive(Deserialize, Default)]
struct XmlGeoBox {
    #[serde(rename = "westBoundLongitude", default)]
    west_bound_longitude: String,
    #[serde(rename = "eastBoundLongitude", default)]
    east_bound_longitude: String,
    #[serde(rename = "southBoundLatitude", default)]
    south_bound_latitude: String,
    #[serde(rename = "northBoundLatitude", default)]
    north_bound_latitude: String,
}

// ── Funding references ────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct XmlFundingReferences {
    #[serde(rename = "fundingReference", default)]
    funding_reference: Vec<XmlFundingReference>,
}

#[derive(Deserialize, Default)]
struct XmlFundingReference {
    #[serde(rename = "funderName", default)]
    funder_name: String,
    #[serde(rename = "funderIdentifier", default)]
    funder_identifier: Option<XmlFunderIdentifier>,
    #[serde(rename = "awardNumber", default)]
    award_number: Option<XmlAwardNumber>,
    #[serde(rename = "awardTitle", default)]
    award_title: String,
}

#[derive(Deserialize, Default)]
struct XmlFunderIdentifier {
    #[serde(rename = "@funderIdentifierType", default)]
    identifier_type: String,
    #[serde(rename = "$text", default)]
    value: String,
}

#[derive(Deserialize, Default)]
struct XmlAwardNumber {
    #[serde(rename = "@awardURI", default)]
    award_uri: String,
    #[serde(rename = "$text", default)]
    value: String,
}

// ── Contributor conversion ─────────────────────────────────────────────────────

fn parse_xml_affiliations(affiliations: &[XmlAffiliation]) -> Vec<Affiliation> {
    affiliations
        .iter()
        .filter(|a| !a.name.trim().is_empty())
        .map(|a| {
            let id = if a.scheme.eq_ignore_ascii_case("ROR") {
                normalize_ror(&a.id)
            } else if a.scheme.eq_ignore_ascii_case("ISNI") {
                a.id.clone()
            } else {
                String::new()
            };
            Affiliation {
                id,
                name: a.name.trim().to_string(),
                ..Default::default()
            }
        })
        .collect()
}

fn get_xml_id(name_identifiers: &[XmlNameIdentifier]) -> String {
    for ni in name_identifiers {
        if ni.scheme == "ORCID" || ni.scheme_uri.contains("orcid.org") {
            return normalize_orcid(&ni.value);
        }
        if ni.scheme == "ROR" || ni.scheme_uri.contains("ror.org") {
            return normalize_ror(&ni.value);
        }
    }
    String::new()
}

fn get_xml_contributor(
    name_type: &str,
    name: &str,
    given_name: String,
    family_name: String,
    name_identifiers: &[XmlNameIdentifier],
    affiliations: &[XmlAffiliation],
    default_role: &str,
) -> Contributor {
    let name_type = match name_type {
        "Personal" | "Organizational" => &name_type[..name_type.len() - 2],
        other => other,
    };

    let id = get_xml_id(name_identifiers);

    let mut name = cleanup_author(Some(name)).unwrap_or_else(|| name.to_string());
    let mut given_name = given_name;
    let mut family_name = family_name;

    let mut inferred_type =
        infer_contributor_type(name_type, &id, &given_name, &family_name, &name, None);
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

    let affils = parse_xml_affiliations(affiliations);

    let normalized_role = normalize_commonmeta_role(default_role);
    let roles = normalize_contributor_roles(&[normalized_role.clone()], &normalized_role);

    if inferred_type == "Person" {
        Contributor::person(
            Person { id, given_name, family_name, affiliations: affils, asserted_by: String::new() },
            roles,
        )
    } else {
        Contributor::organization(Organization { id, name, asserted_by: String::new() }, roles)
    }
}

// ── Core conversion ────────────────────────────────────────────────────────────

fn from_xml_resource(r: XmlResource) -> Data {
    let mut data = Data::default();

    // DOI
    let doi_id = normalize_doi(r.identifier.value.trim());
    data.id = doi_id.clone();

    // Schema version from xmlns
    if !r.xmlns.is_empty() {
        data.schema_version = r.xmlns.clone();
    }

    // Type
    let (resource_type_general, resource_type) = r
        .resource_type
        .as_ref()
        .map(|rt| (rt.resource_type_general.as_str(), rt.text.trim()))
        .unwrap_or(("", ""));

    data.type_ = C::dc_to_cm(resource_type_general).to_string();

    let additional = C::dc_to_cm(resource_type);
    if !additional.is_empty() {
        data.type_ = additional.to_string();
    } else if !resource_type.is_empty()
        && !resource_type.eq_ignore_ascii_case(&data.type_)
    {
        data.additional_type = resource_type.to_string();
    }

    // Creators → role "Author"
    for c in r.creators.creator {
        let name = c.creator_name.text.trim().to_string();
        if name.is_empty() && c.given_name.is_empty() && c.family_name.is_empty() {
            continue;
        }
        let contrib = get_xml_contributor(
            &c.creator_name.name_type,
            &name,
            c.given_name.trim().to_string(),
            c.family_name.trim().to_string(),
            &c.name_identifiers,
            &c.affiliations,
            "Author",
        );
        let id = contrib.id().to_string();
        if id.is_empty()
            || !data.contributors.iter().any(|e| !e.id().is_empty() && e.id() == id)
        {
            data.contributors.push(contrib);
        }
    }

    // Contributors → merge in with contributorType as role
    for c in r.contributors.contributor {
        let name = c.contributor_name.text.trim().to_string();
        if name.is_empty() && c.given_name.is_empty() && c.family_name.is_empty() {
            continue;
        }
        let role = if is_recognized_role(&c.contributor_type) {
            c.contributor_type.clone()
        } else {
            "Author".to_string()
        };
        let contrib = get_xml_contributor(
            &c.contributor_name.name_type,
            &name,
            c.given_name.trim().to_string(),
            c.family_name.trim().to_string(),
            &c.name_identifiers,
            &c.affiliations,
            &role,
        );
        let id = contrib.id().to_string();
        if id.is_empty()
            || !data.contributors.iter().any(|e| !e.id().is_empty() && e.id() == id)
        {
            data.contributors.push(contrib);
        }
    }

    // Dates
    for d in r.dates.date {
        let v = d.value.trim().to_string();
        match d.date_type.as_str() {
            "Accepted" => data.dates.accepted = v,
            "Available" => data.dates.available = v,
            "Collected" => data.dates.collected = v,
            "Created" => data.dates.created = v,
            "Issued" | "Published" => data.date_published = v,
            "Submitted" => data.dates.submitted = v,
            "Updated" => data.date_updated = v,
            "Valid" => data.dates.valid = v,
            "Withdrawn" => data.dates.withdrawn = v,
            "Other" => data.dates.other = v,
            _ => {}
        }
    }
    if data.date_published.is_empty() && !r.publication_year.is_empty() {
        data.date_published = r.publication_year.trim().to_string();
    }

    // Descriptions
    for d in r.descriptions.description {
        let type_ = match d.description_type.as_str() {
            "Abstract" | "Summary" | "Methods" | "TechnicalInfo" | "Other" => {
                d.description_type.clone()
            }
            _ => "Other".to_string(),
        };
        let text = sanitize(d.text.trim());
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
    for f in r.funding_references.funding_reference {
        let funder_id = match f
            .funder_identifier
            .as_ref()
            .map(|fi| fi.identifier_type.as_str())
        {
            Some("ROR") => {
                let raw = f
                    .funder_identifier
                    .as_ref()
                    .map(|fi| fi.value.trim())
                    .unwrap_or("");
                normalize_ror(raw)
            }
            _ => String::new(),
        };
        let award_number = f
            .award_number
            .as_ref()
            .map(|a| a.value.trim().to_string())
            .unwrap_or_default();
        let award_id = f
            .award_number
            .as_ref()
            .map(|a| a.award_uri.clone())
            .unwrap_or_default();
        data.funding_references.push(FundingReference {
            funder_id,
            funder_name: f.funder_name.trim().to_string(),
            award_number,
            award_title: f.award_title.trim().to_string(),
            award_id,
            ..Default::default()
        });
    }

    // GeoLocations
    for g in r.geo_locations.geo_location {
        data.geo_locations.push(GeoLocation {
            geo_location_place: g.geo_location_place.trim().to_string(),
            geo_location_point_longitude: g
                .geo_location_point
                .as_ref()
                .and_then(|p| p.point_longitude.trim().parse().ok()),
            geo_location_point_latitude: g
                .geo_location_point
                .as_ref()
                .and_then(|p| p.point_latitude.trim().parse().ok()),
            geo_location_box_west_longitude: g
                .geo_location_box
                .as_ref()
                .and_then(|b| b.west_bound_longitude.trim().parse().ok()),
            geo_location_box_east_longitude: g
                .geo_location_box
                .as_ref()
                .and_then(|b| b.east_bound_longitude.trim().parse().ok()),
            geo_location_box_south_latitude: g
                .geo_location_box
                .as_ref()
                .and_then(|b| b.south_bound_latitude.trim().parse().ok()),
            geo_location_box_north_latitude: g
                .geo_location_box
                .as_ref()
                .and_then(|b| b.north_bound_latitude.trim().parse().ok()),
            ..Default::default()
        });
    }

    // Alternate identifiers
    for ai in r.alternate_identifiers.alternate_identifier {
        let v = ai.value.trim();
        if !v.is_empty() {
            data.identifiers.push(Identifier {
                identifier: v.to_string(),
                identifier_type: ai.identifier_type,
            });
        }
    }
    if !doi_id.is_empty() && !data.identifiers.iter().any(|i| i.identifier == doi_id) {
        data.identifiers.push(Identifier {
            identifier: doi_id.clone(),
            identifier_type: "DOI".to_string(),
        });
    }

    // Publisher
    if let Some(pub_) = r.publisher {
        let name = pub_.name.trim().to_string();
        if !name.is_empty() {
            let id = if !pub_.publisher_identifier.is_empty() {
                normalize_ror(&pub_.publisher_identifier)
            } else {
                String::new()
            };
            data.publisher = Publisher { id, name, asserted_by: String::new() };
        }
    }

    // Subjects
    for s in r.subjects.subject {
        let text = s.text.trim().to_string();
        if !text.is_empty() {
            let subject = Subject { subject: text, ..Default::default() };
            if !data.subjects.contains(&subject) {
                data.subjects.push(subject);
            }
        }
    }

    data.language = r.language.trim().to_string();

    // License: first rights entry
    if let Some(rights) = r.rights_list.rights.into_iter().next() {
        let uri = rights.rights_uri.trim();
        if !uri.is_empty() {
            let (url, ok) = normalize_cc_url(uri);
            let url = if ok { url } else { uri.to_string() };
            data.license = crate::spdx::from_url(&url);
        }
    }

    data.provider = "DataCite".to_string();

    // Related identifiers → references and relations
    for ri in &r.related_identifiers.related_identifier {
        let id = normalize_id(ri.value.trim());
        if id.is_empty() {
            continue;
        }
        if is_reference_relation(&ri.relation_type) {
            let type_ = C::dc_to_cm(&ri.resource_type_general).to_string();
            data.references.push(Reference { id, type_, ..Default::default() });
        } else if is_supported_relation(&ri.relation_type) {
            let mapped = dc_to_cm_relation(&ri.relation_type);
            let type_ = if mapped.is_empty() {
                ri.relation_type.clone()
            } else {
                mapped.to_string()
            };
            let relation = Relation { id, type_, ..Default::default() };
            if !data.relations.contains(&relation) {
                data.relations.push(relation);
            }
        }
    }

    // Titles
    for t in r.titles.title {
        let text = t.text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        let type_ = match t.title_type.as_str() {
            "MainTitle" | "Subtitle" | "TranslatedTitle" => t.title_type,
            _ => String::new(),
        };
        if data.title.is_empty() && (type_.is_empty() || type_ == "MainTitle") {
            data.title = text;
        } else {
            data.additional_titles.push(Title { title: text, type_, language: t.lang });
        }
    }

    // Container from relatedItems (IsPublishedIn)
    for item in r.related_items.related_item {
        if item.relation_type != "IsPublishedIn" {
            continue;
        }
        let title = item.titles.title.into_iter().next().map(|t| t.text.trim().to_string()).unwrap_or_default();
        let (identifier, identifier_type) = item
            .identifier
            .map(|id| (id.value.trim().to_string(), id.identifier_type))
            .unwrap_or_default();
        data.container = Container {
            type_: item.related_item_type,
            title,
            identifier,
            identifier_type,
            volume: item.volume,
            issue: item.issue,
            first_page: item.first_page,
            last_page: item.last_page,
            ..Default::default()
        };
        break;
    }

    data.version = r.version.trim().to_string();
    data
}

// ── Public API ─────────────────────────────────────────────────────────────────

pub fn read_xml(input: &str) -> Result<Data> {
    let resource: XmlResource = xml_from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_xml_resource(resource))
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
    let xml = client
        .get(&url)
        .header("Accept", "application/vnd.datacite.datacite+xml")
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
    use super::*;

    #[test]
    fn read_xml_geolocation_example() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/geolocation.xml");
        let data = read_xml(xml).expect("parse should succeed");
        assert_eq!(data.type_, "Dataset");
        assert_eq!(data.id, "https://doi.org/10.5072/geopointexample");
        assert!(!data.contributors.is_empty());
        assert!(!data.geo_locations.is_empty());
        assert_eq!(data.geo_locations[0].geo_location_point_longitude, Some(-52.0));
        assert_eq!(data.geo_locations[0].geo_location_point_latitude, Some(69.0));
    }

    #[test]
    fn read_xml_full_v4_4() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/full_v4_4.xml");
        let data = read_xml(xml).expect("parse should succeed");
        assert_eq!(data.type_, "Software");
        assert_eq!(data.id, "https://doi.org/10.5072/example-full");
        assert_eq!(data.title, "Full DataCite XML Example");
        assert_eq!(data.date_published, "2014");
        assert!(!data.contributors.is_empty());
        assert_eq!(data.contributors[0].given_name(), "Elizabeth");
        assert_eq!(data.contributors[0].family_name(), "Miller");
        assert_eq!(
            data.contributors[0].id(),
            "https://orcid.org/0000-0001-5000-0007"
        );
        assert!(!data.funding_references.is_empty());
        assert_eq!(
            data.funding_references[0].funder_name,
            "National Science Foundation"
        );
        assert!(!data.geo_locations.is_empty());
        assert_eq!(data.geo_locations[0].geo_location_place, "Atlantic Ocean");
    }

    #[test]
    fn read_xml_schema_4_0() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/schema_4_0.xml");
        let data = read_xml(xml).expect("parse should succeed");
        assert_eq!(data.type_, "Dataset");
        assert_eq!(data.id, "https://doi.org/10.6071/z7wc73");
        assert!(!data.title.is_empty());
        assert!(!data.contributors.is_empty());
    }
}
