use quick_xml::de::from_str as xml_from_str;
use quick_xml::se::Serializer;
use serde::{Deserialize, Serialize};

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
                ..Default::default()
            });
        }
    }
    if !doi_id.is_empty() && !data.identifiers.iter().any(|i| i.identifier == doi_id) {
        data.identifiers.push(Identifier {
            identifier: doi_id.clone(),
            identifier_type: "DOI".to_string(),
            ..Default::default()
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

// ── Output structs (XML serialization) ────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename = "resource")]
struct OutResource {
    #[serde(rename = "@xmlns")]
    xmlns: &'static str,
    #[serde(rename = "@xmlns:xsi")]
    xmlns_xsi: &'static str,
    #[serde(rename = "@xsi:schemaLocation")]
    xsi_schema_location: &'static str,
    identifier: OutIdentifier,
    creators: OutCreators,
    titles: OutTitles,
    publisher: OutPublisher,
    #[serde(rename = "publicationYear")]
    publication_year: String,
    #[serde(rename = "resourceType")]
    resource_type: OutResourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    subjects: Option<OutSubjects>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contributors: Option<OutContributors>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dates: Option<OutDates>,
    #[serde(skip_serializing_if = "str::is_empty")]
    language: String,
    #[serde(rename = "alternateIdentifiers", skip_serializing_if = "Option::is_none")]
    alternate_identifiers: Option<OutAlternateIdentifiers>,
    #[serde(rename = "relatedIdentifiers", skip_serializing_if = "Option::is_none")]
    related_identifiers: Option<OutRelatedIdentifiers>,
    #[serde(rename = "rightsList", skip_serializing_if = "Option::is_none")]
    rights_list: Option<OutRightsList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    descriptions: Option<OutDescriptions>,
    #[serde(rename = "geoLocations", skip_serializing_if = "Option::is_none")]
    geo_locations: Option<OutGeoLocations>,
    #[serde(rename = "fundingReferences", skip_serializing_if = "Option::is_none")]
    funding_references: Option<OutFundingReferences>,
    #[serde(rename = "relatedItems", skip_serializing_if = "Option::is_none")]
    related_items: Option<OutRelatedItems>,
    #[serde(skip_serializing_if = "str::is_empty")]
    version: String,
}

#[derive(Serialize)]
struct OutIdentifier {
    #[serde(rename = "@identifierType")]
    identifier_type: &'static str,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutCreators {
    creator: Vec<OutCreator>,
}

#[derive(Serialize)]
struct OutCreator {
    #[serde(rename = "creatorName")]
    creator_name: OutNameElement,
    #[serde(rename = "givenName", skip_serializing_if = "str::is_empty")]
    given_name: String,
    #[serde(rename = "familyName", skip_serializing_if = "str::is_empty")]
    family_name: String,
    #[serde(rename = "nameIdentifier", skip_serializing_if = "Vec::is_empty")]
    name_identifiers: Vec<OutNameIdentifier>,
    #[serde(rename = "affiliation", skip_serializing_if = "Vec::is_empty")]
    affiliations: Vec<OutAffiliation>,
}

#[derive(Serialize)]
struct OutContributors {
    contributor: Vec<OutContributor>,
}

#[derive(Serialize)]
struct OutContributor {
    #[serde(rename = "@contributorType")]
    contributor_type: String,
    #[serde(rename = "contributorName")]
    contributor_name: OutNameElement,
    #[serde(rename = "givenName", skip_serializing_if = "str::is_empty")]
    given_name: String,
    #[serde(rename = "familyName", skip_serializing_if = "str::is_empty")]
    family_name: String,
    #[serde(rename = "nameIdentifier", skip_serializing_if = "Vec::is_empty")]
    name_identifiers: Vec<OutNameIdentifier>,
    #[serde(rename = "affiliation", skip_serializing_if = "Vec::is_empty")]
    affiliations: Vec<OutAffiliation>,
}

#[derive(Serialize)]
struct OutNameElement {
    #[serde(rename = "@nameType", skip_serializing_if = "str::is_empty")]
    name_type: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutNameIdentifier {
    #[serde(rename = "@nameIdentifierScheme")]
    scheme: &'static str,
    #[serde(rename = "@schemeURI", skip_serializing_if = "str::is_empty")]
    scheme_uri: &'static str,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutAffiliation {
    #[serde(rename = "@affiliationIdentifier", skip_serializing_if = "str::is_empty")]
    id: String,
    #[serde(rename = "@affiliationIdentifierScheme", skip_serializing_if = "str::is_empty")]
    scheme: &'static str,
    #[serde(rename = "@schemeURI", skip_serializing_if = "str::is_empty")]
    scheme_uri: &'static str,
    #[serde(rename = "$text")]
    name: String,
}

#[derive(Serialize)]
struct OutTitles {
    title: Vec<OutTitle>,
}

#[derive(Serialize)]
struct OutTitle {
    #[serde(rename = "@titleType", skip_serializing_if = "str::is_empty")]
    title_type: String,
    #[serde(rename = "@xml:lang", skip_serializing_if = "str::is_empty")]
    lang: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutPublisher {
    #[serde(rename = "@publisherIdentifier", skip_serializing_if = "str::is_empty")]
    publisher_identifier: String,
    #[serde(rename = "@publisherIdentifierScheme", skip_serializing_if = "str::is_empty")]
    publisher_identifier_scheme: &'static str,
    #[serde(rename = "@schemeURI", skip_serializing_if = "str::is_empty")]
    scheme_uri: &'static str,
    #[serde(rename = "$text")]
    name: String,
}

#[derive(Serialize)]
struct OutResourceType {
    #[serde(rename = "@resourceTypeGeneral")]
    resource_type_general: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutSubjects {
    subject: Vec<OutSubject>,
}

#[derive(Serialize)]
struct OutSubject {
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutDates {
    date: Vec<OutDate>,
}

#[derive(Serialize)]
struct OutDate {
    #[serde(rename = "@dateType")]
    date_type: &'static str,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutAlternateIdentifiers {
    #[serde(rename = "alternateIdentifier")]
    alternate_identifier: Vec<OutAlternateIdentifier>,
}

#[derive(Serialize)]
struct OutAlternateIdentifier {
    #[serde(rename = "@alternateIdentifierType")]
    identifier_type: String,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutRelatedIdentifiers {
    #[serde(rename = "relatedIdentifier")]
    related_identifier: Vec<OutRelatedIdentifier>,
}

#[derive(Serialize)]
struct OutRelatedIdentifier {
    #[serde(rename = "@relatedIdentifierType")]
    identifier_type: String,
    #[serde(rename = "@relationType")]
    relation_type: String,
    #[serde(rename = "@resourceTypeGeneral", skip_serializing_if = "str::is_empty")]
    resource_type_general: String,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutRightsList {
    rights: Vec<OutRights>,
}

#[derive(Serialize)]
struct OutRights {
    #[serde(rename = "@rightsURI", skip_serializing_if = "str::is_empty")]
    rights_uri: String,
    #[serde(rename = "@rightsIdentifier", skip_serializing_if = "str::is_empty")]
    rights_identifier: String,
    #[serde(rename = "@rightsIdentifierScheme", skip_serializing_if = "str::is_empty")]
    rights_identifier_scheme: &'static str,
    #[serde(rename = "@schemeURI", skip_serializing_if = "str::is_empty")]
    scheme_uri: &'static str,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutDescriptions {
    description: Vec<OutDescription>,
}

#[derive(Serialize)]
struct OutDescription {
    #[serde(rename = "@descriptionType")]
    description_type: String,
    #[serde(rename = "@xml:lang", skip_serializing_if = "str::is_empty")]
    lang: String,
    #[serde(rename = "$text")]
    text: String,
}

#[derive(Serialize)]
struct OutGeoLocations {
    #[serde(rename = "geoLocation")]
    geo_location: Vec<OutGeoLocation>,
}

#[derive(Serialize)]
struct OutGeoLocation {
    #[serde(rename = "geoLocationPlace", skip_serializing_if = "str::is_empty")]
    geo_location_place: String,
    #[serde(rename = "geoLocationPoint", skip_serializing_if = "Option::is_none")]
    geo_location_point: Option<OutGeoPoint>,
    #[serde(rename = "geoLocationBox", skip_serializing_if = "Option::is_none")]
    geo_location_box: Option<OutGeoBox>,
}

#[derive(Serialize)]
struct OutGeoPoint {
    #[serde(rename = "pointLongitude")]
    point_longitude: f64,
    #[serde(rename = "pointLatitude")]
    point_latitude: f64,
}

#[derive(Serialize)]
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

#[derive(Serialize)]
struct OutFundingReferences {
    #[serde(rename = "fundingReference")]
    funding_reference: Vec<OutFundingReference>,
}

#[derive(Serialize)]
struct OutFundingReference {
    #[serde(rename = "funderName")]
    funder_name: String,
    #[serde(rename = "funderIdentifier", skip_serializing_if = "Option::is_none")]
    funder_identifier: Option<OutFunderIdentifier>,
    #[serde(rename = "awardNumber", skip_serializing_if = "Option::is_none")]
    award_number: Option<OutAwardNumber>,
    #[serde(rename = "awardTitle", skip_serializing_if = "str::is_empty")]
    award_title: String,
}

#[derive(Serialize)]
struct OutFunderIdentifier {
    #[serde(rename = "@funderIdentifierType")]
    identifier_type: String,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutAwardNumber {
    #[serde(rename = "@awardURI", skip_serializing_if = "str::is_empty")]
    award_uri: String,
    #[serde(rename = "$text")]
    value: String,
}

#[derive(Serialize)]
struct OutRelatedItems {
    #[serde(rename = "relatedItem")]
    related_item: Vec<OutRelatedItem>,
}

#[derive(Serialize)]
struct OutRelatedItem {
    #[serde(rename = "@relationType")]
    relation_type: &'static str,
    #[serde(rename = "@relatedItemType")]
    related_item_type: String,
    #[serde(rename = "relatedItemIdentifier", skip_serializing_if = "Option::is_none")]
    identifier: Option<OutRelatedItemIdentifier>,
    #[serde(skip_serializing_if = "Option::is_none")]
    titles: Option<OutTitles>,
    #[serde(skip_serializing_if = "str::is_empty")]
    volume: String,
    #[serde(skip_serializing_if = "str::is_empty")]
    issue: String,
    #[serde(rename = "firstPage", skip_serializing_if = "str::is_empty")]
    first_page: String,
    #[serde(rename = "lastPage", skip_serializing_if = "str::is_empty")]
    last_page: String,
}

#[derive(Serialize)]
struct OutRelatedItemIdentifier {
    #[serde(rename = "@relatedItemIdentifierType", skip_serializing_if = "str::is_empty")]
    identifier_type: String,
    #[serde(rename = "$text")]
    value: String,
}

// ── Conversion (Data → output XML structs) ────────────────────────────────────

fn build_name_identifiers(id: &str) -> Vec<OutNameIdentifier> {
    if id.is_empty() { return vec![]; }
    let (scheme, scheme_uri): (&'static str, &'static str) =
        if id.starts_with("https://orcid.org/") { ("ORCID", "https://orcid.org") }
        else if id.starts_with("https://ror.org/")  { ("ROR",   "https://ror.org") }
        else { ("URL", "") };
    vec![OutNameIdentifier { scheme, scheme_uri, value: id.to_string() }]
}

fn build_affiliations(v: &crate::data::Contributor) -> Vec<OutAffiliation> {
    v.affiliations().iter()
        .filter(|a| !a.name.is_empty())
        .map(|a| {
            let (scheme, scheme_uri): (&'static str, &'static str) =
                if a.id.starts_with("https://ror.org/") { ("ROR", "https://ror.org") }
                else { ("", "") };
            OutAffiliation {
                id: if scheme.is_empty() { String::new() } else { a.id.clone() },
                scheme,
                scheme_uri,
                name: a.name.clone(),
            }
        })
        .collect()
}

fn build_contributor_entry(v: &crate::data::Contributor) -> (OutNameElement, String, String, Vec<OutNameIdentifier>, Vec<OutAffiliation>) {
    let name_type = match v.type_.as_str() {
        "Person"       => "Personal",
        "Organization" => "Organizational",
        other          => other,
    }.to_string();

    let display_name = if !v.family_name().is_empty() || !v.given_name().is_empty() {
        let fam = v.family_name();
        let giv = v.given_name();
        if fam.is_empty() { giv.to_string() }
        else if giv.is_empty() { fam.to_string() }
        else { format!("{fam}, {giv}") }
    } else {
        v.name().to_string()
    };

    let name_el = OutNameElement { name_type, text: display_name };
    let given   = v.given_name().to_string();
    let family  = v.family_name().to_string();
    let ni      = build_name_identifiers(v.id());
    let affils  = build_affiliations(v);
    (name_el, given, family, ni, affils)
}

fn funder_identifier_type(id: &str) -> String {
    if id.starts_with("https://ror.org/")             { "ROR".to_string() }
    else if id.starts_with("https://doi.org/10.13039/"){ "Crossref Funder ID".to_string() }
    else if !id.is_empty()                             { "Other".to_string() }
    else                                               { String::new() }
}

fn cm_to_dc_relation_xml(cm: &str) -> &'static str {
    match cm {
        "IsReviewOf" => "Reviews",
        "HasReview"  => "IsReviewedBy",
        _            => "",
    }
}

fn convert_to_xml(data: &Data) -> OutResource {
    use crate::utils::validate_id;

    // DOI: strip https://doi.org/ prefix for the bare value
    let doi_val = data.id
        .trim_start_matches("https://doi.org/")
        .to_string();

    // Types
    let resource_type_general = {
        let m = C::cm_to_dc(&data.type_);
        if m.is_empty() { "Other".to_string() } else { m.to_string() }
    };
    let additional_type = if data.type_ == "BlogPost" {
        "BlogPost".to_string()
    } else if !data.additional_type.is_empty() {
        data.additional_type.clone()
    } else {
        String::new()
    };

    // Publication year
    let pub_year = if data.date_published.len() >= 4 {
        data.date_published[..4].to_string()
    } else {
        String::new()
    };

    // Creators and contributors
    let mut creators: Vec<OutCreator> = Vec::new();
    let mut contribs: Vec<OutContributor> = Vec::new();

    for v in &data.contributors {
        let (name_el, given, family, ni, affils) = build_contributor_entry(v);
        if v.roles.contains(&"Author".to_string()) {
            creators.push(OutCreator {
                creator_name: name_el,
                given_name: given,
                family_name: family,
                name_identifiers: ni,
                affiliations: affils,
            });
        } else {
            let role = v.roles.first().cloned().unwrap_or_default();
            // Map commonmeta role to DataCite contributorType
            let contributor_type = C::cm_to_dc_role(&role).to_string();
            let contributor_type = if contributor_type.is_empty() { "Other".to_string() } else { contributor_type };
            contribs.push(OutContributor {
                contributor_type,
                contributor_name: name_el,
                given_name: given,
                family_name: family,
                name_identifiers: ni,
                affiliations: affils,
            });
        }
    }

    // Titles
    let mut titles: Vec<OutTitle> = Vec::new();
    if !data.title.is_empty() {
        titles.push(OutTitle { title_type: String::new(), lang: String::new(), text: data.title.clone() });
    }
    for t in &data.additional_titles {
        titles.push(OutTitle { title_type: t.type_.clone(), lang: t.language.clone(), text: t.title.clone() });
    }

    // Publisher
    let (pub_id, pub_id_scheme, pub_scheme_uri): (String, &'static str, &'static str) =
        if data.publisher.id.starts_with("https://ror.org/") {
            (data.publisher.id.clone(), "ROR", "https://ror.org")
        } else {
            (String::new(), "", "")
        };

    // Subjects
    let subjects = if data.subjects.is_empty() { None } else {
        Some(OutSubjects {
            subject: data.subjects.iter().map(|s| OutSubject { text: s.subject.clone() }).collect()
        })
    };

    // Dates
    let mut date_list: Vec<OutDate> = Vec::new();
    if !data.dates.created.is_empty()   { date_list.push(OutDate { date_type: "Created",   value: data.dates.created.clone() }); }
    if !data.dates.submitted.is_empty() { date_list.push(OutDate { date_type: "Submitted", value: data.dates.submitted.clone() }); }
    if !data.dates.accepted.is_empty()  { date_list.push(OutDate { date_type: "Accepted",  value: data.dates.accepted.clone() }); }
    if !data.date_published.is_empty()  { date_list.push(OutDate { date_type: "Issued",    value: data.date_published.clone() }); }
    if !data.date_updated.is_empty()    { date_list.push(OutDate { date_type: "Updated",   value: data.date_updated.clone() }); }
    if !data.dates.available.is_empty() { date_list.push(OutDate { date_type: "Available", value: data.dates.available.clone() }); }
    if !data.dates.collected.is_empty() { date_list.push(OutDate { date_type: "Collected", value: data.dates.collected.clone() }); }
    if !data.dates.valid.is_empty()     { date_list.push(OutDate { date_type: "Valid",      value: data.dates.valid.clone() }); }
    if !data.dates.withdrawn.is_empty() { date_list.push(OutDate { date_type: "Withdrawn", value: data.dates.withdrawn.clone() }); }
    if !data.dates.other.is_empty()     { date_list.push(OutDate { date_type: "Other",     value: data.dates.other.clone() }); }
    let dates = if date_list.is_empty() { None } else { Some(OutDates { date: date_list }) };

    // Alternate identifiers (exclude DOI)
    let doi_normalized = crate::doi_utils::normalize_doi(&data.id);
    let alt_ids: Vec<OutAlternateIdentifier> = data.identifiers.iter()
        .filter(|i| i.identifier != doi_normalized)
        .map(|i| OutAlternateIdentifier { identifier_type: i.identifier_type.clone(), value: i.identifier.clone() })
        .collect();
    let alternate_identifiers = if alt_ids.is_empty() { None } else {
        Some(OutAlternateIdentifiers { alternate_identifier: alt_ids })
    };

    // Related identifiers (relations + references)
    let mut rel_ids: Vec<OutRelatedIdentifier> = Vec::new();
    for r in &data.relations {
        let (id, id_type) = validate_id(&r.id);
        if id.is_empty() { continue; }
        let mapped = cm_to_dc_relation_xml(&r.type_);
        let relation_type = if mapped.is_empty() { r.type_.clone() } else { mapped.to_string() };
        rel_ids.push(OutRelatedIdentifier {
            identifier_type: id_type.to_string(),
            relation_type,
            resource_type_general: String::new(),
            value: id,
        });
    }
    for r in &data.references {
        let (id, id_type) = validate_id(&r.id);
        if id.is_empty() { continue; }
        rel_ids.push(OutRelatedIdentifier {
            identifier_type: id_type.to_string(),
            relation_type: "References".to_string(),
            resource_type_general: C::cm_to_dc(&r.type_).to_string(),
            value: id,
        });
    }
    let related_identifiers = if rel_ids.is_empty() { None } else {
        Some(OutRelatedIdentifiers { related_identifier: rel_ids })
    };

    // Rights
    let rights_list = if data.license.url.is_empty() { None } else {
        Some(OutRightsList { rights: vec![OutRights {
            rights_uri: data.license.url.clone(),
            rights_identifier: data.license.id.to_lowercase(),
            rights_identifier_scheme: "SPDX",
            scheme_uri: "https://spdx.org/licenses/",
            text: String::new(),
        }]})
    };

    // Descriptions
    let mut desc_list: Vec<OutDescription> = Vec::new();
    if !data.description.is_empty() {
        desc_list.push(OutDescription { description_type: "Abstract".to_string(), lang: String::new(), text: data.description.clone() });
    }
    for d in &data.additional_descriptions {
        let dtype = match d.type_.as_str() {
            "Abstract" | "Methods" | "Other" | "SeriesInformation" | "TableOfContents" | "TechnicalInfo" => d.type_.clone(),
            _ => "Other".to_string(),
        };
        desc_list.push(OutDescription { description_type: dtype, lang: d.language.clone(), text: d.description.clone() });
    }
    let descriptions = if desc_list.is_empty() { None } else { Some(OutDescriptions { description: desc_list }) };

    // GeoLocations
    let geo_locs: Vec<OutGeoLocation> = data.geo_locations.iter().map(|g| OutGeoLocation {
        geo_location_place: g.geo_location_place.clone(),
        geo_location_point: match (g.geo_location_point_longitude, g.geo_location_point_latitude) {
            (Some(lon), Some(lat)) => Some(OutGeoPoint { point_longitude: lon, point_latitude: lat }),
            _ => None,
        },
        geo_location_box: match (
            g.geo_location_box_west_longitude, g.geo_location_box_east_longitude,
            g.geo_location_box_south_latitude, g.geo_location_box_north_latitude,
        ) {
            (Some(w), Some(e), Some(s), Some(n)) => Some(OutGeoBox {
                west_bound_longitude: w, east_bound_longitude: e,
                south_bound_latitude: s, north_bound_latitude: n,
            }),
            _ => None,
        },
    }).collect();
    let geo_locations = if geo_locs.is_empty() { None } else { Some(OutGeoLocations { geo_location: geo_locs }) };

    // Funding references
    let fund_refs: Vec<OutFundingReference> = data.funding_references.iter().map(|f| {
        let id_type = funder_identifier_type(&f.funder_id);
        let funder_identifier = if id_type.is_empty() { None } else {
            Some(OutFunderIdentifier { identifier_type: id_type, value: f.funder_id.clone() })
        };
        let award_number = if f.award_number.is_empty() { None } else {
            Some(OutAwardNumber { award_uri: f.award_id.clone(), value: f.award_number.clone() })
        };
        OutFundingReference {
            funder_name: f.funder_name.clone(),
            funder_identifier,
            award_number,
            award_title: f.award_title.clone(),
        }
    }).collect();
    let funding_references = if fund_refs.is_empty() { None } else { Some(OutFundingReferences { funding_reference: fund_refs }) };

    // Related items (container → IsPublishedIn)
    let related_items = if data.container.type_.is_empty() && data.container.title.is_empty() {
        None
    } else {
        let c = &data.container;
        let item_identifier = if c.identifier.is_empty() { None } else {
            Some(OutRelatedItemIdentifier { identifier_type: c.identifier_type.clone(), value: c.identifier.clone() })
        };
        let item_titles = if c.title.is_empty() { None } else {
            Some(OutTitles { title: vec![OutTitle { title_type: String::new(), lang: String::new(), text: c.title.clone() }] })
        };
        Some(OutRelatedItems { related_item: vec![OutRelatedItem {
            relation_type: "IsPublishedIn",
            related_item_type: c.type_.clone(),
            identifier: item_identifier,
            titles: item_titles,
            volume: c.volume.clone(),
            issue: c.issue.clone(),
            first_page: c.first_page.clone(),
            last_page: c.last_page.clone(),
        }]})
    };

    OutResource {
        xmlns: "http://datacite.org/schema/kernel-4",
        xmlns_xsi: "http://www.w3.org/2001/XMLSchema-instance",
        xsi_schema_location: "http://datacite.org/schema/kernel-4 https://schema.datacite.org/meta/kernel-4.7/metadata.xsd",
        identifier: OutIdentifier { identifier_type: "DOI", value: doi_val },
        creators: OutCreators { creator: creators },
        titles: OutTitles { title: titles },
        publisher: OutPublisher {
            publisher_identifier: pub_id,
            publisher_identifier_scheme: pub_id_scheme,
            scheme_uri: pub_scheme_uri,
            name: data.publisher.name.clone(),
        },
        publication_year: pub_year,
        resource_type: OutResourceType { resource_type_general, text: additional_type },
        subjects,
        contributors: if contribs.is_empty() { None } else { Some(OutContributors { contributor: contribs }) },
        dates,
        language: data.language.clone(),
        alternate_identifiers,
        related_identifiers,
        rights_list,
        descriptions,
        geo_locations,
        funding_references,
        related_items,
        version: data.version.clone(),
    }
}

fn serialize_resource(resource: OutResource) -> Result<Vec<u8>> {
    let mut buf = String::new();
    let mut ser = Serializer::new(&mut buf);
    ser.indent(' ', 2);
    resource.serialize(ser).map_err(|e| Error::Serialize(e.to_string()))?;
    let xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}", buf);
    Ok(xml.into_bytes())
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

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let resource = convert_to_xml(data);
    serialize_resource(resource)
}

pub fn write_all(list: &[Data]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for data in list {
        let bytes = write(data)?;
        if !out.is_empty() { out.push(b'\n'); }
        out.extend_from_slice(&bytes);
    }
    Ok(out)
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

    #[test]
    fn write_xml_round_trip_full_v4_4() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/full_v4_4.xml");
        let data = read_xml(xml).expect("parse should succeed");
        let bytes = write(&data).expect("write should succeed");
        let out = std::str::from_utf8(&bytes).expect("valid UTF-8");
        // Re-parse what we wrote
        let data2 = read_xml(out).expect("round-trip parse should succeed");
        assert_eq!(data2.id, data.id);
        assert_eq!(data2.type_, data.type_);
        assert_eq!(data2.title, data.title);
        assert_eq!(data2.date_published, data.date_published);
    }

    #[test]
    fn write_xml_produces_identifier_element() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/full_v4_4.xml");
        let data = read_xml(xml).expect("parse should succeed");
        let bytes = write(&data).expect("write should succeed");
        let out = std::str::from_utf8(&bytes).unwrap();
        assert!(out.contains("<identifier identifierType=\"DOI\">"), "expected DOI identifier");
        assert!(out.contains("<resourceType"), "expected resourceType element");
    }

    #[test]
    fn write_xml_geolocation_round_trip() {
        let xml = include_str!("../../tests/fixtures/datacite_xml/geolocation.xml");
        let data = read_xml(xml).expect("parse should succeed");
        let bytes = write(&data).expect("write should succeed");
        let out = std::str::from_utf8(&bytes).unwrap();
        let data2 = read_xml(out).expect("round-trip parse should succeed");
        assert!(!data2.geo_locations.is_empty());
    }

    #[test]
    fn write_xml_validates_against_xsd() {
        use crate::schema_utils::xml_schema_errors;
        let xml = include_str!("../../tests/fixtures/datacite_xml/full_v4_4.xml");
        let data = read_xml(xml).expect("parse should succeed");
        let bytes = write(&data).expect("write should succeed");
        let result = xml_schema_errors(&bytes, Some("datacite_xml"));
        if let Err(ref e) = result {
            assert!(
                !e.to_string().contains("failed to compile"),
                "DataCite XSD schema failed to compile: {e}"
            );
        }
    }
}
