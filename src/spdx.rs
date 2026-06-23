//! SPDX license vocabulary lookup.
//!
//! Ported from <https://github.com/front-matter/commonmeta/blob/main/spdx/reader.go>.
//! [`search`]/[`from_url`]/[`from_id`] consume the bundled snapshot in
//! `src/vocabularies/licenses.json` (see [`crate::vocabularies`]);
//! [`fetch_all`]/[`refresh_bundled_vocabulary`] mirror the Go version's
//! ability to pull a fresh copy from the upstream SPDX license-list-data repo.

use lazy_static::lazy_static;
use serde::Deserialize;
use url::Url;

use crate::error::{Error, Result};
use crate::utils::normalize_cc_url;
use crate::vocabularies::load_vocabulary;

/// Upstream URL for the canonical SPDX license list, used by [`fetch_all`].
pub const SPDX_DOWNLOAD_URL: &str =
    "https://raw.githubusercontent.com/spdx/license-list-data/main/json/licenses.json";

/// Filename the bundled vocabulary is stored under, both in
/// `src/vocabularies/` and as the default target of
/// [`refresh_bundled_vocabulary`].
pub const SPDX_FILENAME: &str = "licenses.json";

/// A single SPDX license entry, as listed in the SPDX license-list-data
/// `licenses.json`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub reference: String,
    #[serde(default)]
    pub is_deprecated_license_id: bool,
    #[serde(default)]
    pub details_url: String,
    #[serde(default)]
    pub reference_number: i64,
    #[serde(default)]
    pub name: String,
    pub license_id: String,
    #[serde(default)]
    pub see_also: Vec<String>,
    #[serde(default)]
    pub is_osi_approved: bool,
}

#[derive(Debug, Deserialize)]
struct Spdx {
    licenses: Vec<License>,
}

lazy_static! {
    static ref LICENSES: Vec<License> = {
        let raw =
            load_vocabulary("SPDX.Licenses").expect("bundled SPDX vocabulary should be loadable");
        let parsed: Spdx =
            serde_json::from_str(raw).expect("bundled SPDX vocabulary should be valid JSON");
        parsed.licenses
    };
}

/// Downloads the latest SPDX license list from [`SPDX_DOWNLOAD_URL`] and
/// parses it, without touching the bundled snapshot used by [`search`].
/// Mirrors Go's `spdx.FetchAll`.
pub fn fetch_all() -> Result<Vec<License>> {
    let bytes = crate::file_utils::download_file(SPDX_DOWNLOAD_URL)
        .map_err(|e| Error::Http(e.to_string()))?;
    let parsed: Spdx = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Parse(format!("invalid SPDX license list: {e}")))?;
    Ok(parsed.licenses)
}

/// Downloads the latest SPDX license list and writes the raw JSON to
/// `path` (e.g. `src/vocabularies/licenses.json`), for refreshing the
/// bundled snapshot ahead of a release. Mirrors Go's `spdx.FetchAll`, which
/// downloads and writes the file as a side effect of fetching.
///
/// This only rewrites the file on disk — since [`search`] reads the
/// snapshot via `include_str!` at compile time, the crate must be rebuilt
/// for the refreshed data to take effect.
pub fn refresh_bundled_vocabulary<P: AsRef<std::path::Path>>(path: P) -> Result<()> {
    let bytes = crate::file_utils::download_file(SPDX_DOWNLOAD_URL)
        .map_err(|e| Error::Http(e.to_string()))?;
    // Validate before overwriting the bundled file.
    let _: Spdx = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Parse(format!("invalid SPDX license list: {e}")))?;
    crate::file_utils::write_file(path, &bytes).map_err(|e| Error::Http(e.to_string()))
}

/// Searches the bundled SPDX metadata for a given SPDX license id or URL.
///
/// If `id` parses as an absolute URL, it is matched (after normalizing known
/// Creative Commons URL variants) against each license's `seeAlso` list.
/// Otherwise it is matched case-insensitively against `licenseId`.
pub fn search(id: &str) -> Option<&'static License> {
    if let Ok(u) = Url::parse(id)
        && u.host_str().is_some()
    {
        let target = u.to_string();
        if let Some(found) = LICENSES.iter().find(|l| l.see_also.contains(&target)) {
            return Some(found);
        }
        let (canonical, ok) = normalize_cc_url(id);
        if ok {
            return LICENSES.iter().find(|l| l.see_also.contains(&canonical));
        }
        return None;
    }
    LICENSES
        .iter()
        .find(|l| l.license_id.eq_ignore_ascii_case(id))
}

/// Builds a [`crate::data::License`] from a license URL, filling in the SPDX
/// `id` and `title` (license name) when recognized. The `url` field is kept
/// as given.
pub fn from_url(url: &str) -> crate::data::License {
    let entry = search(url);
    crate::data::License {
        id: entry.map(|l| l.license_id.clone()).unwrap_or_default(),
        title: entry.map(|l| l.name.clone()).unwrap_or_default(),
        url: url.to_string(),
    }
}

/// Builds a [`crate::data::License`] from an SPDX license id, filling in the
/// canonical URL (first `seeAlso` entry) and title when recognized. Falls
/// back to the given id verbatim when it isn't found.
pub fn from_id(id: &str) -> crate::data::License {
    let entry = search(id);
    crate::data::License {
        id: entry
            .map(|l| l.license_id.clone())
            .unwrap_or_else(|| id.to_string()),
        title: entry.map(|l| l.name.clone()).unwrap_or_default(),
        url: entry
            .and_then(|l| l.see_also.first().cloned())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "hits the network; run explicitly with `cargo test -- --ignored`"]
    fn fetch_all_downloads_current_spdx_list() {
        let list = fetch_all().expect("download should succeed");
        assert!(list.iter().any(|l| l.license_id == "MIT"));
    }

    #[test]
    fn search_by_id_is_case_insensitive() {
        let l = search("cc-by-4.0").expect("known license id");
        assert_eq!(l.license_id, "CC-BY-4.0");
        assert_eq!(l.name, "Creative Commons Attribution 4.0 International");
    }

    #[test]
    fn search_by_canonical_url() {
        let l = search("https://creativecommons.org/licenses/by/4.0/legalcode")
            .expect("known license url");
        assert_eq!(l.license_id, "CC-BY-4.0");
    }

    #[test]
    fn search_by_variant_cc_url_normalizes_first() {
        let l = search("https://creativecommons.org/licenses/by/4.0/").expect("known license url");
        assert_eq!(l.license_id, "CC-BY-4.0");
    }

    #[test]
    fn search_unknown_returns_none() {
        assert!(search("https://example.com/unknown").is_none());
        assert!(search("Not-A-Real-License").is_none());
    }

    #[test]
    fn from_url_fills_id_and_title() {
        let l = from_url("https://creativecommons.org/publicdomain/zero/1.0/legalcode");
        assert_eq!(l.id, "CC0-1.0");
        assert_eq!(l.title, "Creative Commons Zero v1.0 Universal");
        assert_eq!(l.url, "https://creativecommons.org/publicdomain/zero/1.0/legalcode");
    }

    #[test]
    fn from_id_fills_title_and_url() {
        let l = from_id("mit");
        assert_eq!(l.id, "MIT");
        assert_eq!(l.title, "MIT License");
        assert!(!l.url.is_empty());
    }

    #[test]
    fn from_id_falls_back_to_input_when_unrecognized() {
        let l = from_id("Not-A-Real-License");
        assert_eq!(l.id, "Not-A-Real-License");
        assert_eq!(l.title, "");
        assert_eq!(l.url, "");
    }
}
