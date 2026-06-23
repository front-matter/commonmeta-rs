use lazy_static::lazy_static;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use url::Url;

use crate::crockford::decode;
use crate::doi_utils::{normalize_doi, validate_doi};

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

/// Validates an identifier and returns the identifier and its type.
/// Type can be: DOI, UUID, PMID, PMCID, OpenAlex, ORCID, ROR, GRID,
/// RID, Wikidata, ISNI, ISSN, Crossref Funder ID, URL, or "".
pub fn validate_id(id: &str) -> (String, &'static str) {
    if let Some(fundref) = validate_crossref_funder_id(id) {
        return (fundref, "Crossref Funder ID");
    }
    if let Some(doi) = validate_doi(id) {
        return (doi, "DOI");
    }
    if let Some(uuid) = validate_uuid(id) {
        return (uuid, "UUID");
    }
    if let Some(pmid) = validate_pmid(id) {
        return (pmid, "PMID");
    }
    if let Some(pmcid) = validate_pmcid(id) {
        return (pmcid, "PMCID");
    }
    if let Some(openalex) = validate_openalex(id) {
        return (openalex, "OpenAlex");
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
    if let Some(rid) = validate_rid(id) {
        return (rid, "RID");
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

    match validate_url(id).as_str() {
        "DOI" => return (id.to_string(), "DOI"),
        "JSONFEEDID" => return (id.to_string(), "JSONFEEDID"),
        "URL" => return (id.to_string(), "URL"),
        _ => {}
    }

    (String::new(), "")
}

/// Validates an identifier and additionally returns its category.
/// Category: "Work", "Person", "Organization", "Contributor", "All", or "".
pub fn validate_id_category(id: &str) -> (String, &'static str, &'static str) {
    let (pid, type_) = validate_id(id);
    let category = match type_ {
        "ROR" | "Crossref Funder ID" | "GRID" => "Organization",
        "ORCID" => "Person",
        "ISNI" => "Contributor",
        "DOI" | "PMID" | "PMCID" => "Work",
        "Wikidata" | "OpenAlex" | "URL" | "UUID" => "All",
        _ => "",
    };
    (pid, type_, category)
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
                Some(clean_match)
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

/// Validates an OpenAlex ID.
/// First letter indicates resource type (A author, F funder, I institution,
/// P publisher, S source, W work), followed by 8-10 digits.
pub fn validate_openalex(openalex: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:(?:http|https)://openalex\.org/)?([AFIPSW]\d{8,10})$").unwrap();
    }
    RE.captures(openalex)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates a PubMed ID (PMID).
pub fn validate_pmid(pmid: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:(?:http|https)://pubmed\.ncbi\.nlm\.nih\.gov/)?(\d{4,8})$").unwrap();
    }
    RE.captures(pmid)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// Validates a PubMed Central ID (PMCID).
pub fn validate_pmcid(pmcid: &str) -> Option<String> {
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r"^(?:(?:http|https)://www\.ncbi\.nlm\.nih\.gov/pmc/articles/)?(\d{4,8})$")
                .unwrap();
    }
    RE.captures(pmcid)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

// ── Normalizers ───────────────────────────────────────────────────────────────

/// Normalizes any PID: DOI → canonical URL, UUID, Wikidata, or plain URL.
pub fn normalize_id(pid: &str) -> String {
    let doi = normalize_doi(pid);
    if !doi.is_empty() {
        return doi;
    }
    if let Some(uuid) = validate_uuid(pid) {
        return uuid;
    }
    if let Some(wikidata) = validate_wikidata(pid) {
        return format!("https://www.wikidata.org/wiki/{}", wikidata);
    }
    match Url::parse(pid) {
        Err(_) => String::new(),
        Ok(mut u) => {
            if u.scheme().is_empty() {
                return String::new();
            }
            if u.scheme() == "http" {
                let _ = u.set_scheme("https");
            }
            let s = u.to_string();
            if s.ends_with('/') {
                s[..s.len() - 1].to_string()
            } else {
                s
            }
        }
    }
}

/// Normalizes a work identifier (DOI, UUID, URL, Wikidata).
pub fn normalize_work_id(id: &str) -> String {
    let (pid, type_, category) = validate_id_category(id);
    if !["Work", "All"].contains(&category) {
        return String::new();
    }
    match type_ {
        "DOI" => normalize_doi(&pid),
        "UUID" | "URL" => pid,
        "Wikidata" => format!("https://www.wikidata.org/wiki/{}", pid),
        _ => String::new(),
    }
}

/// Normalizes an organization identifier (ROR, Crossref Funder ID, GRID,
/// Wikidata, ISNI).
pub fn normalize_organization_id(id: &str) -> String {
    let (pid, type_, category) = validate_id_category(id);
    if !["Organization", "Contributor", "All"].contains(&category) {
        return String::new();
    }
    match type_ {
        "ROR" => format!("https://ror.org/{}", pid),
        "Crossref Funder ID" => format!("https://doi.org/{}", pid),
        "GRID" => format!("https://grid.ac/institutes/{}", pid),
        "Wikidata" => format!("https://www.wikidata.org/wiki/{}", pid),
        "ISNI" => format!("https://isni.org/isni/{}", pid),
        _ => String::new(),
    }
}

/// Normalizes a person identifier (ORCID, ISNI, Wikidata).
pub fn normalize_person_id(id: &str) -> String {
    let (pid, type_, category) = validate_id_category(id);
    if !["Person", "Contributor", "All"].contains(&category) {
        return String::new();
    }
    match type_ {
        "ORCID" => format!("https://orcid.org/{}", pid),
        "ISNI" => format!("https://isni.org/isni/{}", pid),
        "Wikidata" => format!("https://www.wikidata.org/wiki/{}", pid),
        _ => String::new(),
    }
}

/// Returns a normalized ORCID URL.
pub fn normalize_orcid(orcid: &str) -> String {
    match validate_orcid(orcid) {
        Some(id) => format!("https://orcid.org/{}", id),
        None => String::new(),
    }
}

/// Returns a normalized ROR URL.
pub fn normalize_ror(ror: &str) -> String {
    match validate_ror(ror) {
        Some(id) => format!("https://ror.org/{}", id),
        None => String::new(),
    }
}

/// Normalizes a URL: upgrades http→https when `secure`, lowercases when `lower`.
pub fn normalize_url(s: &str, secure: bool, lower: bool) -> Option<String> {
    let mut u = Url::parse(s).ok()?;
    u.host_str()?;
    if secure && u.scheme() == "http" {
        let _ = u.set_scheme("https");
    }
    let result = u.to_string();
    Some(if lower { result.to_lowercase() } else { result })
}

/// Normalizes a Creative Commons license URL to the canonical `/legalcode` form.
/// Returns `(normalized_url, true)` on success, `("", false)` otherwise.
pub fn normalize_cc_url(url_: &str) -> (String, bool) {
    lazy_static! {
        static ref CC_MAP: std::collections::HashMap<&'static str, &'static str> = {
            let mut m = std::collections::HashMap::new();
            m.insert(
                "https://creativecommons.org/licenses/by/1.0",
                "https://creativecommons.org/licenses/by/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by/2.0",
                "https://creativecommons.org/licenses/by/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by/2.5",
                "https://creativecommons.org/licenses/by/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by/3.0",
                "https://creativecommons.org/licenses/by/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by/3.0/us",
                "https://creativecommons.org/licenses/by/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by/4.0",
                "https://creativecommons.org/licenses/by/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc/1.0",
                "https://creativecommons.org/licenses/by-nc/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc/2.0",
                "https://creativecommons.org/licenses/by-nc/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc/2.5",
                "https://creativecommons.org/licenses/by-nc/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc/3.0",
                "https://creativecommons.org/licenses/by-nc/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc/4.0",
                "https://creativecommons.org/licenses/by-nc/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd-nc/1.0",
                "https://creativecommons.org/licenses/by-nd-nc/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd-nc/2.0",
                "https://creativecommons.org/licenses/by-nd-nc/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd-nc/2.5",
                "https://creativecommons.org/licenses/by-nd-nc/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd-nc/3.0",
                "https://creativecommons.org/licenses/by-nd-nc/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd-nc/4.0",
                "https://creativecommons.org/licenses/by-nd-nc/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/1.0",
                "https://creativecommons.org/licenses/by-nc-sa/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/2.0",
                "https://creativecommons.org/licenses/by-nc-sa/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/2.5",
                "https://creativecommons.org/licenses/by-nc-sa/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/3.0",
                "https://creativecommons.org/licenses/by-nc-sa/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/3.0/us",
                "https://creativecommons.org/licenses/by-nc-sa/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-sa/4.0",
                "https://creativecommons.org/licenses/by-nc-sa/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd/1.0",
                "https://creativecommons.org/licenses/by-nd/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd/2.0",
                "https://creativecommons.org/licenses/by-nd/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd/2.5",
                "https://creativecommons.org/licenses/by-nd/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd/3.0",
                "https://creativecommons.org/licenses/by-nd/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nd/4.0",
                "https://creativecommons.org/licenses/by-nd/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-sa/1.0",
                "https://creativecommons.org/licenses/by-sa/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-sa/2.0",
                "https://creativecommons.org/licenses/by-sa/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-sa/2.5",
                "https://creativecommons.org/licenses/by-sa/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-sa/3.0",
                "https://creativecommons.org/licenses/by-sa/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-sa/4.0",
                "https://creativecommons.org/licenses/by-sa/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-nd/1.0",
                "https://creativecommons.org/licenses/by-nc-nd/1.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-nd/2.0",
                "https://creativecommons.org/licenses/by-nc-nd/2.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-nd/2.5",
                "https://creativecommons.org/licenses/by-nc-nd/2.5/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-nd/3.0",
                "https://creativecommons.org/licenses/by-nc-nd/3.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/by-nc-nd/4.0",
                "https://creativecommons.org/licenses/by-nc-nd/4.0/legalcode",
            );
            m.insert(
                "https://creativecommons.org/licenses/publicdomain",
                "https://creativecommons.org/licenses/publicdomain/",
            );
            m.insert(
                "https://creativecommons.org/publicdomain/zero/1.0",
                "https://creativecommons.org/publicdomain/zero/1.0/legalcode",
            );
            m
        };
    }

    if url_.is_empty() {
        return (String::new(), false);
    }
    let normalized = match normalize_url(url_, true, false) {
        Some(u) => u,
        None => return (String::new(), false),
    };
    let mut u = match Url::parse(&normalized) {
        Ok(u) => u,
        Err(_) => return (String::new(), false),
    };
    // strip trailing slash when no query string
    if u.query().is_none() {
        let path = u.path().to_string();
        if path.len() > 1 && path.ends_with('/') {
            u.set_path(&path[..path.len() - 1]);
        }
    }
    let key = u.to_string();
    if let Some(v) = CC_MAP.get(key.as_str()) {
        return (v.to_string(), true);
    }
    // Some providers (e.g. DataCite) return URLs that already end with /legalcode.
    // Strip that suffix and try again.
    let stripped = key.strip_suffix("/legalcode").unwrap_or(key.as_str());
    match CC_MAP.get(stripped) {
        Some(v) => (v.to_string(), true),
        None => (String::new(), false),
    }
}


// ── ISSN / slug / community helpers ──────────────────────────────────────────

/// Returns an ISSN expressed as a portal.issn.org URL.
pub fn issn_as_url(issn: &str) -> String {
    if issn.is_empty() {
        return String::new();
    }
    format!("https://portal.issn.org/resource/ISSN/{}", issn)
}

/// Returns a community slug as a Rogue Scholar API URL.
pub fn community_slug_as_url(slug: &str, host: &str) -> String {
    if slug.is_empty() {
        return String::new();
    }
    let h = if host.is_empty() {
        "rogue-scholar.org"
    } else {
        host
    };
    format!("https://{}/api/communities/{}", h, slug)
}

// ── String utilities ──────────────────────────────────────────────────────────

/// Strips HTML, allowing only safe inline elements.
pub fn sanitize(html: &str) -> String {
    let allowed: std::collections::HashSet<&str> =
        ["b", "br", "code", "em", "i", "sub", "sup", "strong"]
            .iter()
            .copied()
            .collect();
    let clean = ammonia::Builder::new()
        .tags(allowed)
        .clean(html)
        .to_string();
    clean.trim_matches('\n').to_string()
}

/// Uppercases only the first character of a string.
pub fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Removes duplicate elements from a Vec while preserving order.
pub fn dedupe_slice<T: Eq + std::hash::Hash + Clone>(v: Vec<T>) -> Vec<T> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|x| seen.insert(x.clone())).collect()
}

/// Converts a PascalCase / camelCase string to "Title Words" form.
pub fn camel_case_to_words(s: &str) -> String {
    lazy_static! {
        static ref RE1: Regex = Regex::new("(.)([A-Z][a-z]+)").unwrap();
        static ref RE2: Regex = Regex::new("([a-z0-9])([A-Z])").unwrap();
    }
    let words = RE1.replace_all(s, "${1} ${2}");
    let words = RE2.replace_all(&words, "${1} ${2}");
    title_case(&words.to_lowercase())
}

/// Converts "words in a string" to camelCase.
pub fn words_to_camel_case(s: &str) -> String {
    lazy_static! {
        static ref RE1: Regex = Regex::new("(.)([A-Z][a-z]+)").unwrap();
        static ref RE2: Regex = Regex::new("([a-z0-9])([A-Z])").unwrap();
    }
    let words = RE1.replace_all(s, "${1} ${2}");
    let words = RE2.replace_all(&words, "${1} ${2}");
    let pascal: String = words.split_whitespace().map(title_case).collect::<String>();
    let pascal = pascal.replace([' ', '-'], "");
    if pascal.is_empty() {
        return pascal;
    }
    let mut chars = pascal.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

/// Lowercases the first character of a PascalCase string (PascalCase → camelCase).
pub fn camel_case_string(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_lowercase().collect::<String>() + chars.as_str(),
    }
}

/// Converts kebab-case to camelCase.
pub fn kebab_case_to_camel_case(s: &str) -> String {
    lazy_static! {
        static ref RE: Regex = Regex::new("-([a-z])").unwrap();
    }
    RE.replace_all(s, |caps: &regex::Captures| caps[1].to_uppercase())
        .to_string()
}

/// Converts kebab-case to PascalCase.
pub fn kebab_case_to_pascal_case(s: &str) -> String {
    let camel = kebab_case_to_camel_case(s);
    title_case(&camel)
}

/// Unicode-normalizes a string: NFD decomposition, strip combining diacritics, NFC recompose.
pub fn normalize_string(s: &str) -> String {
    s.nfd()
        .filter(|c| !('\u{0300}'..='\u{036F}').contains(c))
        .nfc()
        .collect()
}

/// Converts a string to a URL slug: normalize, keep only lowercase letters/digits.
pub fn string_to_slug(s: &str) -> String {
    normalize_string(s)
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else {
                None
            }
        })
        .collect()
}

/// Inserts `sep` every `n` characters.
pub fn split_string(s: &str, n: usize, sep: &str) -> String {
    if n == 0 {
        return s.to_string();
    }
    s.as_bytes()
        .chunks(n)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(sep)
}

/// Returns a language code in the requested format.
/// `format`: "iso639-3" for 3-letter code, "name" for English name, otherwise ISO 639-1 alpha-2.
/// Accepts alpha-2, alpha-3, or English name as input.
pub fn get_language(lang: &str, format: &str) -> String {
    if lang.is_empty() {
        return String::new();
    }
    let found = isolang::Language::from_639_1(lang)
        .or_else(|| isolang::Language::from_639_3(lang))
        .or_else(|| isolang::Language::from_name(lang));
    match found {
        None => String::new(),
        Some(l) => match format {
            "iso639-3" => l.to_639_3().to_string(),
            "name" => l.to_name().to_string(),
            _ => l.to_639_1().unwrap_or_default().to_string(),
        },
    }
}

// ── Format detection ──────────────────────────────────────────────────────────

/// Auto-detects the commonmeta reader format from various hints.
pub fn find_from_format(
    pid: Option<&str>,
    str_: Option<&str>,
    ext: Option<&str>,
    filename: Option<&str>,
) -> &'static str {
    if let Some(p) = pid
        && !p.is_empty()
    {
        return find_from_format_by_id(p);
    }
    if let (Some(s), Some(e)) = (str_, ext)
        && !s.is_empty()
        && !e.is_empty()
    {
        return find_from_format_by_ext(e);
    }
    if let Some(s) = str_
        && !s.is_empty()
    {
        return find_from_format_by_string(s);
    }
    if let Some(f) = filename
        && !f.is_empty()
    {
        return find_from_format_by_filename(f);
    }
    "datacite"
}

/// Detects format by PID (DOI, URL patterns).
pub fn find_from_format_by_id(id: &str) -> &'static str {
    if validate_doi(id).is_some() {
        // TODO: query DOI RA to distinguish crossref / datacite
        return "crossref";
    }
    if id.ends_with("codemeta.json") {
        return "codemeta";
    }
    if id.ends_with("CITATION.cff") || id.contains("github.com") {
        return "cff";
    }
    if id.contains("jsonfeed") {
        return "jsonfeed";
    }
    lazy_static! {
        static ref RE_ROGUE: Regex =
            Regex::new(r"^https:/(/)?api\.rogue-scholar\.org/posts/(.+)$").unwrap();
        static ref RE_INVENIO: Regex = Regex::new(r"^https:/(/)(.+)/(api/)?records/(.+)$").unwrap();
    }
    if RE_ROGUE.is_match(id) {
        return "jsonfeed";
    }
    if RE_INVENIO.is_match(id) {
        return "inveniordm";
    }
    "schemaorg"
}

/// Detects format by file extension.
pub fn find_from_format_by_ext(ext: &str) -> &'static str {
    match ext {
        ".bib" => "bibtex",
        ".ris" => "ris",
        _ => "",
    }
}

/// Detects format by parsing the JSON string and examining key fields.
pub fn find_from_format_by_string(s: &str) -> &'static str {
    let data: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return "",
    };
    if let Some(v) = data.get("schema_version").and_then(|v| v.as_str())
        && v.starts_with("https://commonmeta.org")
    {
        return "commonmeta";
    }
    if let Some(v) = data.get("@context").and_then(|v| v.as_str()) {
        if v == "http://schema.org" {
            return "schemaorg";
        }
        if v.contains("codemeta") {
            return "codemeta";
        }
    }
    if data.get("guid").is_some() {
        return "jsonfeed";
    }
    if let Some(v) = data.get("schemaVersion").and_then(|v| v.as_str())
        && v.starts_with("http://datacite.org/schema/kernel")
    {
        return "datacite";
    }
    if data.get("source").and_then(|v| v.as_str()) == Some("Crossref") {
        return "crossref";
    }
    if data.get("conceptdoi").is_some() {
        return "inveniordm";
    }
    if data.get("credit_metadata").is_some() {
        return "kbase";
    }
    ""
}

/// Detects format by filename.
pub fn find_from_format_by_filename(filename: &str) -> &'static str {
    if filename == "CITATION.cff" {
        return "cff";
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_openalex() {
        assert_eq!(validate_openalex("W1234567890"), Some("W1234567890".into()));
        assert_eq!(
            validate_openalex("https://openalex.org/W1234567890"),
            Some("W1234567890".into())
        );
        assert_eq!(validate_openalex("X123"), None);
    }

    #[test]
    fn test_validate_pmid() {
        assert_eq!(validate_pmid("12345678"), Some("12345678".into()));
        assert_eq!(
            validate_pmid("https://pubmed.ncbi.nlm.nih.gov/12345678"),
            Some("12345678".into())
        );
        assert_eq!(validate_pmid("123"), None); // too short
    }

    #[test]
    fn test_normalize_orcid() {
        assert_eq!(
            normalize_orcid("0000-0001-5000-0007"),
            "https://orcid.org/0000-0001-5000-0007"
        );
        assert_eq!(normalize_orcid("not-an-orcid"), "");
    }

    #[test]
    fn test_normalize_ror() {
        assert_eq!(
            normalize_ror("https://ror.org/0521rfr06"),
            "https://ror.org/0521rfr06"
        );
    }

    #[test]
    fn test_issn_as_url() {
        assert_eq!(
            issn_as_url("1234-5678"),
            "https://portal.issn.org/resource/ISSN/1234-5678"
        );
        assert_eq!(issn_as_url(""), "");
    }

    #[test]
    fn test_community_slug_as_url() {
        assert_eq!(
            community_slug_as_url("my-blog", ""),
            "https://rogue-scholar.org/api/communities/my-blog"
        );
        assert_eq!(
            community_slug_as_url("blog", "example.org"),
            "https://example.org/api/communities/blog"
        );
    }

    #[test]
    fn test_camel_case_string() {
        assert_eq!(camel_case_string("IsVersionOf"), "isVersionOf");
        assert_eq!(camel_case_string("HasPreprint"), "hasPreprint");
        assert_eq!(camel_case_string(""), "");
    }

    #[test]
    fn test_kebab_case_to_camel_case() {
        assert_eq!(kebab_case_to_camel_case("foo-bar-baz"), "fooBarBaz");
        assert_eq!(kebab_case_to_pascal_case("foo-bar"), "FooBar");
    }

    #[test]
    fn test_normalize_string() {
        assert_eq!(normalize_string("Héllo Wörld"), "Hello World");
        assert_eq!(normalize_string("café"), "cafe");
    }

    #[test]
    fn test_string_to_slug() {
        assert_eq!(string_to_slug("Héllo Wörld!"), "helloworld");
        assert_eq!(string_to_slug("café au lait"), "cafeaulait");
    }

    #[test]
    fn test_split_string() {
        assert_eq!(split_string("1234567890", 4, "-"), "1234-5678-90");
        assert_eq!(split_string("abcdef", 2, "_"), "ab_cd_ef");
    }

    #[test]
    fn test_get_language() {
        assert_eq!(get_language("en", "iso639-3"), "eng");
        assert_eq!(get_language("deu", ""), "de");
        assert_eq!(get_language("French", "iso639-3"), "fra");
        assert_eq!(get_language("xyz", ""), "");
    }

    #[test]
    fn test_normalize_cc_url() {
        let (url, ok) = normalize_cc_url("https://creativecommons.org/licenses/by/4.0/");
        assert!(ok);
        assert_eq!(url, "https://creativecommons.org/licenses/by/4.0/legalcode");

        let (_, ok) = normalize_cc_url("https://example.com/license");
        assert!(!ok);
    }

    #[test]
    fn test_dedupe_slice() {
        assert_eq!(dedupe_slice(vec![1, 2, 2, 3, 1]), vec![1, 2, 3]);
        assert_eq!(dedupe_slice(vec!["a", "b", "a"]), vec!["a", "b"]);
    }

    #[test]
    fn test_find_from_format_by_string() {
        let json = r#"{"schema_version":"https://commonmeta.org/commonmeta_v0.14","id":"x"}"#;
        assert_eq!(find_from_format_by_string(json), "commonmeta");

        let json = r#"{"guid":"abc-123","url":"https://example.com"}"#;
        assert_eq!(find_from_format_by_string(json), "jsonfeed");
    }

    #[test]
    fn test_validate_id_category() {
        let (id, type_, cat) = validate_id_category("https://ror.org/0521rfr06");
        assert_eq!(type_, "ROR");
        assert_eq!(cat, "Organization");
        assert_eq!(id, "0521rfr06");

        let (_, type_, cat) = validate_id_category("https://orcid.org/0000-0001-5000-0007");
        assert_eq!(type_, "ORCID");
        assert_eq!(cat, "Person");
    }

    #[test]
    fn test_validate_orcid_parity_cases() {
        let cases = [
            (
                "http://orcid.org/0000-0002-2590-225X",
                Some("0000-0002-2590-225X"),
            ),
            (
                "https://orcid.org/0000-0002-1825-0097",
                Some("0000-0002-1825-0097"),
            ),
            ("0000-0002-1825-0097", Some("0000-0002-1825-0097")),
            (
                "https://sandbox.orcid.org/0000-0002-1825-0097",
                Some("0000-0002-1825-0097"),
            ),
            ("0000-0002-1825-009", None),
        ];

        for (input, expected) in cases {
            assert_eq!(validate_orcid(input).as_deref(), expected, "input: {input}");
        }
    }

    #[test]
    fn test_validate_isni_parity_cases() {
        let cases = [
            (
                "https://isni.org/isni/0000000121122291",
                Some("0000000121122291"),
            ),
            (
                "https://isni.org/isni/0000 0001 2112 2291",
                Some("0000000121122291"),
            ),
            ("0000-0001-2112-2291", Some("0000000121122291")),
            ("https://isni.org/isni/000000021825009", None),
        ];

        for (input, expected) in cases {
            assert_eq!(validate_isni(input).as_deref(), expected, "input: {input}");
        }
    }

    #[test]
    fn test_validate_wikidata_parity_cases() {
        let cases = [
            ("https://www.wikidata.org/wiki/Q7186", Some("Q7186")),
            ("https://www.wikidata.org/wiki/Q251061", Some("Q251061")),
            ("Q251061", Some("Q251061")),
            ("https://www.wikidata.org/wiki/Property:P610", None),
        ];

        for (input, expected) in cases {
            assert_eq!(
                validate_wikidata(input).as_deref(),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn test_validate_ror_parity_cases() {
        let cases = [
            ("https://ror.org/0342dzm54", Some("0342dzm54")),
            ("0342dzm54", Some("0342dzm54")),
            ("invalid", None),
        ];

        for (input, expected) in cases {
            assert_eq!(validate_ror(input).as_deref(), expected, "input: {input}");
        }
    }

    #[test]
    fn test_validate_crossref_funder_id_parity_cases() {
        let cases = [
            (
                "https://doi.org/10.13039/501100000155",
                Some("501100000155"),
            ),
            ("10.13039/501100000155", Some("501100000155")),
            ("100010540", Some("100010540")),
            ("not-a-funder-id", None),
        ];

        for (input, expected) in cases {
            assert_eq!(
                validate_crossref_funder_id(input).as_deref(),
                expected,
                "input: {input}"
            );
        }
    }

    #[test]
    fn test_validate_url_and_id_parity_cases() {
        assert_eq!(
            validate_url("https://elifesciences.org/articles/91729"),
            "URL"
        );
        assert_eq!(validate_url("https://doi.org/10.7554/eLife.91729.3"), "DOI");
        assert_eq!(validate_url("10.7554/eLife.91729.3"), "DOI");
        assert_eq!(validate_url("https://doi.org/10.1101"), "URL");
        assert_eq!(validate_url("10.1101"), "");

        let (_, id_type) = validate_id("https://isni.org/isni/0000000121122291");
        assert_eq!(id_type, "ISNI");

        let (_, id_type) = validate_id("https://orcid.org/0000-0002-1825-0097");
        assert_eq!(id_type, "ORCID");

        let (_, id_type) =
            validate_id("https://datadryad.org/stash/dataset/doi:10.5061/dryad.8515");
        assert_eq!(id_type, "URL");
    }

    #[test]
    fn test_find_from_format_helpers_parity_cases() {
        assert_eq!(find_from_format_by_ext(".bib"), "bibtex");
        assert_eq!(find_from_format_by_ext(".ris"), "ris");
        assert_eq!(find_from_format_by_ext(".json"), "");

        assert_eq!(find_from_format_by_filename("CITATION.cff"), "cff");
        assert_eq!(find_from_format_by_filename("citation.cff"), "");
    }
}
