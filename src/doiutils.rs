//! Utilities for working with DOIs
//!
//! This module provides functionality for:
//! - Validating, normalizing and escaping DOIs
//! - Encoding and decoding DOI identifiers
//! - Checking DOI registration status
//! - Working with DOI prefixes and registration agencies
//! - Generating DOIs for specific blogging platforms like WordPress and Substack
use lazy_static::lazy_static;
use regex::Regex;
use reqwest::{Client};
use std::error::Error;
use std::time::Duration;
use url::Url;
use base32::Alphabet;
use std::string::ToString;


/// Extracts DOI prefix from URL
pub fn prefix_from_url(s: &str) -> Result<String, Box<dyn Error>> {
    let url = Url::parse(s)?;
    
    if url.host_str() != Some("doi.org") || !url.path().starts_with("/10.") {
        return Ok(String::new());
    }
    
    let path: Vec<&str> = url.path().split('/').collect();
    if path.len() < 2 {
        return Ok(String::new());
    }
    
    Ok(path[1].to_string())
}

/// Normalizes a DOI
pub fn normalize_doi(doi: &str) -> String {
    if let Some(doi_str) = validate_doi(doi) {
        let resolver = doi_resolver(doi, false);
        return format!("{}{}", resolver, doi_str.to_lowercase());
    }
    String::new()
}

/// Validates a DOI
pub fn validate_doi(doi: &str) -> Option<String> {
    lazy_static! {
        static ref DOI_REGEX: Regex = Regex::new(
            r"^(?:(http|https):/(/)?(dx\.)?(doi\.org|handle\.stage\.datacite\.org|handle\.test\.datacite\.org)/)?(doi:)?(10\.\d{4,5}/[^\s]+)$"
        ).unwrap();
    }
    
    if let Some(captures) = DOI_REGEX.captures(doi) {
        return captures.get(6).map(|m| m.as_str().to_string());
    }
    None
}

/// Escapes a DOI, i.e. replaces '/' with '%2F'
pub fn escape_doi(doi: &str) -> String {
    if let Some(doi_str) = validate_doi(doi) {
        return doi_str.replace("/", "%2F");
    }
    String::new()
}

/// Encodes a DOI with a randomly generated suffix
pub fn encode_doi(prefix: &str) -> String {
    let suffix = crockford::generate(10, 5, true);
    let doi = format!("https://doi.org/{}/{}", prefix, suffix);
    doi
}

/// Decodes a DOI suffix to an integer
pub fn decode_doi(doi: &str) -> i64 {
    if let Some(d) = validate_doi(doi) {
        let parts: Vec<&str> = d.split('/').collect();
        if parts.len() < 2 {
            return 0;
        }
        
        let suffix = parts[1];
        match crockford::decode(suffix, true) {
            Ok(number) => return number,
            Err(e) => {
                eprintln!("Error decoding DOI suffix: {}", e);
                return 0;
            }
        }
    }
    0
}

/// Checks if a DOI resolves (i.e. redirects) via the DOI handle servers
pub async fn is_registered_doi(doi: &str) -> bool {
    let url = normalize_doi(doi);
    if url.is_empty() {
        return false;
    }
    
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_default();
        
    match client.head(&url).send().await {
        Ok(resp) => resp.status().as_u16() <= 308,
        Err(_) => false,
    }
}

/// Validates a DOI prefix for a given DOI
pub fn validate_prefix(doi: &str) -> Option<String> {
    lazy_static! {
        static ref PREFIX_REGEX: Regex = Regex::new(
            r"^(?:(http|https):/(/)?(dx\.)?(doi\.org|handle\.stage\.datacite\.org|handle\.test\.datacite\.org)/)?(doi:)?(10\.\d{4,5})"
        ).unwrap();
    }
    
    if let Some(captures) = PREFIX_REGEX.captures(doi) {
        return captures.get(6).map(|m| m.as_str().to_string());
    }
    None
}

/// Returns a DOI resolver for a given DOI
pub fn doi_resolver(doi: &str, sandbox: bool) -> String {
    if let Ok(d) = Url::parse(doi) {
        if d.host_str() == Some("stage.datacite.org") || sandbox {
            return "https://handle.stage.datacite.org/".to_string();
        }
    }
    "https://doi.org/".to_string()
}
