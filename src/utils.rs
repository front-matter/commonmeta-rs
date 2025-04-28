use lazy_static::lazy_static;
use regex::Regex;
use url::Url;

use crate::crockford::decode;
use crate::doi_utils::validate_doi;

/// Validates the checksum of a string using the ISO 7064 Mod 11-2 algorithm.
fn validate_mod11_2(input: &str) -> Result<(), String> {
    if !input.chars().all(|c| c.is_ascii_digit() || c == 'X') {
        return Err("Invalid characters in input".to_string());
    }

    // the last character is the checksum
    let checksum_char = input.chars().last().unwrap();
    let body = &input[..input.len() - 1];

    let mut m = 0;

    for c in body.chars() {
        let d = c.to_digit(10).unwrap() as i32;

        m = ((m + d) * 2) % 11;
    }

    let check_value = (12 - m) % 11;
    let expected_char = if check_value == 10 {
        'X'
    } else {
        char::from_digit(check_value as u32, 10).unwrap()
    };

    // compare with expected checksum
    if checksum_char == expected_char {
        Ok(())
    } else {
        Err("Invalid checksum".to_string())
    }
}

pub fn decode_id(id: &str) -> Result<i64, String> {
    let (identifier, identifier_type) = validate_id(id);

    match identifier_type {
        "DOI" => {
            // the format of a DOI is a prefix and a suffix separated by a slash
            // the prefix starts with 10. and is followed by 4-5 digits
            // the suffix is a string of characters and is not case-sensitive
            // suffixes from Rogue Scholar are base32-encoded numbers with checksums
            let parts: Vec<&str> = identifier.split('/').collect();
            if parts.len() < 2 {
                return Err(format!("Invalid DOI format: {}", id));
            }
            let suffix = parts[1];
            decode(suffix, true).map_err(|e| e.to_string())
        }
        "ROR" => {
            // ROR ID is a 9-character string that starts with 0
            // and is a base32-encoded number with a mod 97-1
            decode(&identifier, true).map_err(|e| e.to_string())
        }
        "RID" => {
            // RID is a 10-character string with a hyphen after five digits.
            // It is a base32-encoded numbers with checksum.
            decode(&identifier, true).map_err(|e| e.to_string())
        }
        "ORCID" => {
            let cleaned = identifier.replace("-", "");

            // Verify checksum using iso7064 mod 11-2
            if let Err(e) = validate_mod11_2(&cleaned) {
                return Err(format!("Invalid checksum for ORCID {}: {}", identifier, e));
            }

            // Parse the identifier without the checksum
            let number_str = &cleaned[..cleaned.len() - 1];
            match number_str.parse::<i64>() {
                Ok(n) => Ok(n),
                Err(e) => Err(format!("Failed to parse ORCID: {}", e)),
            }
        }
        _ => Err(format!("identifier {} not recognized", id)),
    }
}

/// ValidateID validates an identifier and returns the type
/// Can be DOI, UUID, ISSN, ORCID, ROR, URL, RID, Wikidata, ISNI
/// or GRID
pub fn validate_id(id: &str) -> (String, &str) {
    if let Some(fundref) = validate_crossref_funder_id(id) {
        return (fundref, "Crossref Funder ID");
    }
    if let Some(doi) = validate_doi(id) {
        return (doi, "DOI");
    }
    if let Some(uuid) = validate_uuid(id) {
        return (uuid, "UUID");
    }
    if let Some(rid) = validate_rid(id) {
        return (rid, "RID");
    }
    if let Some(orcid) = validate_orcid(id) {
        return (orcid, "ORCID");
    }
    if let Some(ror) = validate_ror(id) {
        return (ror, "ROR");
    }
    if let Some(grid) = validate_grid(id) {
        return (grid, "GRID");
    }
    if let Some(wikidata) = validate_wikidata(id) {
        return (wikidata, "Wikidata");
    }
    if let Some(isni) = validate_isni(id) {
        return (isni, "ISNI");
    }
    if let Some(issn) = validate_issn(id) {
        return (issn, "ISSN");
    }

    let url = validate_url(id);
    if !url.is_empty() {
        return (id.to_string(), "URL");
    }

    (String::new(), "")
}

/// Validates a Crossref Funder ID
pub fn validate_crossref_funder_id(fundref: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:https?://doi\.org/)?(?:10\.13039/)?((501)?1000[0-9]{5})$").unwrap();
    }

    RE.captures(fundref)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates a GRID ID
/// GRID ID is a string prefixed with grid followed by dot number dot string
pub fn validate_grid(grid: &str) -> Option<String> {
    lazy_static! {
      static ref RE: Regex = Regex::new(r"^(?:(?:http|https)://(?:(?:www)?\.)?grid\.ac/)?(?:institutes/)?(grid\.[0-9]+\.[a-f0-9]{1,2})$").unwrap();
  }

    RE.captures(grid)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates an ISNI
/// ISNI is a 16-character string in blocks of four
/// optionally separated by hyphens or spaces and NOT
/// between 0000-0001-5000-0007 and 0000-0003-5000-0001,
/// or between 0009-0000-0000-0000 and 0009-0010-0000-0000
/// (the ranged reserved for ORCID).
pub fn validate_isni(isni: &str) -> Option<String> {
    lazy_static! {
      static ref RE: Regex = Regex::new(r"^(?:(?:http|https)://(?:(?:www)?\.)?isni\.org/)?(?:isni/)?(0000[ -]?00\d{2}[ -]?\d{4}[ -]?\d{3}[0-9X]+)$").unwrap();
    }

    RE.captures(isni)
        .and_then(|captures| captures.get(1))
        .and_then(|m| {
            let clean_match = m.as_str().replace(" ", "").replace("-", "");

            // Return None if it's in the ORCID range
            if !check_orcid_number_range(&clean_match) {
                Some(m.as_str().to_string())
            } else {
                None
            }
        })
}

/// Validates an ISSN
pub fn validate_issn(issn: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:https://portal\.issn\.org/resource/ISSN/)?(\d{4}\-\d{3}(\d|x|X))$")
                .unwrap();
    }

    RE.captures(issn)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates an ORCID
/// ORCID is a 16-character string in blocks of four
/// separated by hyphens between
/// 0000-0001-5000-0007 and 0000-0003-5000-0001,
/// or between 0009-0000-0000-0000 and 0009-0010-0000-0000.
pub fn validate_orcid(orcid: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^(?:(?:http|https)://(?:(?:www|sandbox)?\.)?orcid\.org/)?(000[09][ -]000[123][ -]\d{4}[ -]\d{3}[0-9X]+)$").unwrap();
    }

    RE.captures(orcid)
        .and_then(|captures| captures.get(1))
        .filter(|m| check_orcid_number_range(m.as_str()))
        .map(|m| m.as_str().to_string())
}

/// Check if ORCID is in the range 0000-0001-5000-0007 and 0000-0003-5000-0001
/// or between 0009-0000-0000-0000 and 0009-0010-0000-0000
fn check_orcid_number_range(orcid: &str) -> bool {
    // ORCID ranges
    const RANGE1_START: &str = "0000000150000007";
    const RANGE1_END: &str = "0000000350000001";
    const RANGE2_START: &str = "0009000000000000";
    const RANGE2_END: &str = "0009001000000000";

    // Remove any spaces and hyphens
    let number = orcid.replace('-', "").replace(" ", "");

    // Check if the ORCID is in either of the valid ranges
    is_in_range(&number, RANGE1_START, RANGE1_END) || is_in_range(&number, RANGE2_START, RANGE2_END)
}

/// Helper function to check if a string is within a specific range
fn is_in_range(value: &str, start: &str, end: &str) -> bool {
    value >= start && value <= end
}

/// Validates a RID
/// RID is the unique identifier used by the InvenioRDM platform
pub fn validate_rid(rid: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(r"^[0-9A-Z]{5}-[0-9A-Z]{3}[0-9]{2}$").unwrap();
    }

    if RE.is_match(rid) {
        Some(rid.to_string())
    } else {
        None
    }
}

/// Validates a ROR ID
/// The ROR ID starts with 0 followed by a 6-character
/// alphanumeric string which is base32-encoded and a 2-digit checksum.
pub fn validate_ror(ror: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:(?:http|https)://ror\.org/)?(0[0-9a-z]{6}\d{2})$").unwrap();
    }

    RE.captures(ror)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates a URL and checks if it is a DOI
pub fn validate_url(str: &str) -> String {
    // Check if it's a DOI
    if validate_doi(str).is_some() {
        return "DOI".to_string();
    }

    // Try to parse as URL
    match url::Url::parse(str) {
        Err(_) => String::new(),
        Ok(url) => {
            // Check for disallowed URL fragments
            if has_disallowed_fragments(&url) {
                return String::new();
            }

            // Handle Rogue Scholar URLs
            if is_rogue_scholar_url(&url) {
                let path_segments: Vec<&str> = url.path().split('/').collect();

                if is_valid_rogue_scholar_post(&path_segments) {
                    return "JSONFEEDID".to_string();
                }
            }
            // Handle standard HTTP(S) URLs
            else if url.scheme() == "http" || url.scheme() == "https" {
                return "URL".to_string();
            }

            String::new()
        }
    }
}

/// Checks if URL has disallowed fragments
fn has_disallowed_fragments(url: &Url) -> bool {
    let disallowed_fragments = [";origin=", ";jsessionid="];

    for fragment in &disallowed_fragments {
        if url.as_str().contains(fragment) {
            return true;
        }
    }
    false
}

/// Checks if URL is from Rogue Scholar
fn is_rogue_scholar_url(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("api.rogue-scholar.org")
}

/// Validates if path segments represent a valid Rogue Scholar post
fn is_valid_rogue_scholar_post(path_segments: &[&str]) -> bool {
    if path_segments.len() >= 2 && path_segments[1] == "posts" {
        // UUID-based post path
        if path_segments.len() == 3 {
            return validate_uuid(path_segments[2]).is_some();
        }
        // DOI-based post path
        else if path_segments.len() == 4 {
            let doi = format!("{}/{}", path_segments[2], path_segments[3]);
            return validate_doi(&doi).is_some();
        }
    }
    false
}

/// Validates a UUID
pub fn validate_uuid(uuid: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex = Regex::new(
            r"^[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-4[a-fA-F0-9]{3}-[89aAbB][a-fA-F0-9]{3}-[a-fA-F0-9]{12}$"
        )
        .unwrap();
    }

    if RE.is_match(uuid) {
        Some(uuid.to_string())
    } else {
        None
    }
}

/// Validates a Wikidata item ID
/// Wikidata item ID is a string prefixed with Q followed by a number
pub fn validate_wikidata(wikidata: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:(?:http|https)://(?:(?:www)?\.)?wikidata\.org/wiki/)?(Q\d+)$")
                .unwrap();
    }

    RE.captures(wikidata)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str().to_string())
}
