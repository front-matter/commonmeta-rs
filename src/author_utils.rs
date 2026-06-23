use serde_json::Value;

use crate::data::Affiliation;
use crate::utils::{normalize_id, validate_orcid, validate_ror};
use crate::constants::CONTRIBUTOR_ROLES;

const ORG_HINT_WORDS: &[&str] = &[
    "University",
    "College",
    "Institute",
    "School",
    "Center",
    "Department",
    "Laboratory",
    "Library",
    "Museum",
    "Foundation",
    "Society",
    "Association",
    "Company",
    "Corporation",
    "Collaboration",
    "Consortium",
    "Incorporated",
    "Inc.",
    "Institut",
    "Research",
    "Science",
    "Team",
    "Ministry",
    "Government",
    "Count",
    "Reviewers",
    "Staff",
    "Lab",
    "Redaktion",
    "Group",
    "area",
];

pub fn cleanup_author(author: Option<&str>) -> Option<String> {
    let Some(author) = author else {
        return None;
    };
    let trimmed = author.trim();
    if trimmed.is_empty() || trimmed.starts_with(',') {
        return None;
    }

    let cleaned = trimmed
        .replace(" - ", "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

pub fn to_ror_id(id: Option<&str>) -> Option<String> {
    let Some(id) = id else {
        return None;
    };
    validate_ror(id).map(|ror| format!("https://ror.org/{}", ror))
}

pub fn is_personal_name(name: &str) -> bool {
    if name.contains(';') {
        return false;
    }

    if name.split_whitespace().count() == 1 && !name.contains(',') {
        return false;
    }

    if ORG_HINT_WORDS.iter().any(|word| name.contains(word)) {
        return false;
    }

    if let Some(last) = name.rsplit(", ").next()
        && matches!(last, "MD" | "PhD" | "BS")
    {
        return true;
    }

    name.contains(',') || name.split_whitespace().count() >= 2
}

pub fn split_person_name(name: &str) -> (String, String, String) {
    let name = name.trim();
    if name.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    if let Some(comma) = name.find(',') {
        let family = name[..comma].trim().to_string();
        let given = name[comma + 1..].trim().to_string();
        return (given, family, String::new());
    }

    if let Some(space) = name.rfind(' ') {
        let given = name[..space].trim().to_string();
        let family = name[space + 1..].trim().to_string();
        if !given.is_empty() && !family.is_empty() {
            return (given, family, String::new());
        }
    }

    (String::new(), String::new(), name.to_string())
}

pub fn infer_contributor_type(
    raw_type: &str,
    id: &str,
    given_name: &str,
    family_name: &str,
    name: &str,
    via: Option<&str>,
) -> String {
    let mut type_ = raw_type.to_string();
    if type_.ends_with("al") {
        type_.truncate(type_.len() - 2);
    }

    if type_.is_empty() && validate_ror(id).is_some() {
        return "Organization".to_string();
    }
    if type_.is_empty() && validate_orcid(id).is_some() {
        return "Person".to_string();
    }
    if type_.is_empty() && (!given_name.is_empty() || !family_name.is_empty()) {
        return "Person".to_string();
    }
    if type_.is_empty() && !name.is_empty() && via == Some("crossref") {
        return "Organization".to_string();
    }
    if type_.is_empty() && is_personal_name(name) {
        return "Person".to_string();
    }
    if type_.is_empty() && !name.is_empty() {
        return "Organization".to_string();
    }
    type_
}

pub fn normalize_contributor_roles(raw_roles: &[String], default_role: &str) -> Vec<String> {
    let filtered: Vec<String> = raw_roles
        .iter()
        .filter(|r| CONTRIBUTOR_ROLES.contains(&r.as_str()))
        .cloned()
        .collect();
    if filtered.is_empty() {
        vec![default_role.to_string()]
    } else {
        filtered
    }
}

pub fn parse_affiliation_value(v: &Value) -> Option<Affiliation> {
    if let Some(name) = v.as_str() {
        if name.is_empty() {
            return None;
        }
        return Some(Affiliation {
            name: name.to_string(),
            ..Default::default()
        });
    }

    let obj = v.as_object()?;
    let mut affiliation_identifier = String::new();
    let mut name = obj
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| obj.get("#text").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    if let Some(raw_aff_id) = obj.get("affiliationIdentifier").and_then(Value::as_str) {
        let normalized = if !raw_aff_id.starts_with("https://") {
            if let Some(scheme_uri) = obj.get("schemeURI").and_then(Value::as_str) {
                let normalized_scheme = if scheme_uri.ends_with('/') {
                    scheme_uri.to_string()
                } else {
                    format!("{}/", scheme_uri)
                };
                normalize_id(&format!("{}{}", normalized_scheme, raw_aff_id))
            } else {
                normalize_id(raw_aff_id)
            }
        } else {
            normalize_id(raw_aff_id)
        };
        affiliation_identifier = normalized;
    } else if let Some(id_val) = obj
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| obj.get("@id").and_then(Value::as_str))
        && (id_val.starts_with("http://") || id_val.starts_with("https://"))
    {
        affiliation_identifier = id_val.to_string();
    } else if let Some(same_as) = obj.get("sameAs").and_then(Value::as_str)
        && (same_as.starts_with("http://") || same_as.starts_with("https://"))
    {
        affiliation_identifier = same_as.to_string();
    }

    if name.is_empty() && affiliation_identifier.is_empty() {
        return None;
    }

    let id = to_ror_id(Some(&affiliation_identifier)).unwrap_or_default();
    if name.is_empty() {
        name = String::new();
    }
    Some(Affiliation {
        id,
        name,
        ..Default::default()
    })
}

pub fn parse_affiliations(values: &[Value]) -> Vec<Affiliation> {
    let mut out = Vec::new();
    for value in values {
        if let Some(aff) = parse_affiliation_value(value) {
            let duplicate = out
                .iter()
                .any(|existing: &Affiliation| existing.id == aff.id && existing.name == aff.name);
            if !duplicate {
                out.push(aff);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_personal_name() {
        assert!(is_personal_name("Doe, Jane"));
        assert!(is_personal_name("Jane Doe"));
        assert!(!is_personal_name("Big Science Collaboration"));
    }

    #[test]
    fn infers_type_from_orcid_and_ror() {
        assert_eq!(
            infer_contributor_type("", "https://orcid.org/0000-0001-5000-0007", "", "", "", None),
            "Person"
        );
        assert_eq!(
            infer_contributor_type("", "https://ror.org/05dxps055", "", "", "", None),
            "Organization"
        );
    }

    #[test]
    fn parses_affiliation_from_string_and_object() {
        let values = vec![
            Value::String("Example University".to_string()),
            serde_json::json!({"id": "https://ror.org/05dxps055", "name": "Example University"}),
        ];
        let affiliations = parse_affiliations(&values);
        assert_eq!(affiliations.len(), 2);
        assert_eq!(affiliations[0].name, "Example University");
        assert_eq!(affiliations[1].id, "https://ror.org/05dxps055");
    }
}