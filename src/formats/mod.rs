pub mod bibtex;
pub mod cff;
pub mod ror;
pub mod ror_countries;
pub mod citation;
pub mod codemeta;
pub mod commonmeta;
pub mod crossref;
pub mod crossref_xml;
pub mod csl;
pub mod datacite;
pub mod inveniordm;
pub mod jsonfeed;
pub mod openalex;
pub mod ris;
pub mod schemaorg;
pub mod vraix;

use crate::data::Data;
use crate::error::{Error, Result};

pub fn read(format: &str, input: &str) -> Result<Data> {
    match format {
        "commonmeta" => commonmeta::read(input),
        "crossref" => {
            if input.trim_start().starts_with('{') {
                crossref::read_json(input)
            } else {
                crossref::fetch(input)
            }
        }
        "crossref_xml" => {
            if input.trim_start().starts_with('<') {
                crossref_xml::read_xml(input)
            } else {
                crossref_xml::fetch(input)
            }
        }
        "datacite" => {
            if input.trim_start().starts_with('{') {
                datacite::read_json(input)
            } else {
                datacite::fetch(input)
            }
        }
        "inveniordm" => {
            if input.trim_start().starts_with('{') {
                inveniordm::read_json(input)
            } else {
                inveniordm::fetch(input)
            }
        }
        "jsonfeed" => {
            if input.trim_start().starts_with('{') {
                jsonfeed::read_json(input)
            } else {
                jsonfeed::fetch(input)
            }
        }
        "csl" => {
            if input.trim_start().starts_with('{') {
                csl::read_json(input)
            } else {
                Err(Error::UnsupportedFormat("csl fetch not supported".to_string()))
            }
        }
        "schemaorg" => {
            if input.trim_start().starts_with('{') {
                schemaorg::read_json(input)
            } else {
                schemaorg::fetch(input)
            }
        }
        "bibtex" => bibtex::read(input),
        "cff" => {
            if input.trim_start().starts_with('{') || input.contains("cff-version") {
                cff::read_yaml(input)
            } else {
                cff::fetch(input)
            }
        }
        "codemeta" => {
            if input.trim_start().starts_with('{') {
                codemeta::read_json(input)
            } else {
                codemeta::fetch(input)
            }
        }
        "ris" => ris::read(input),
        "openalex" => {
            if input.trim_start().starts_with('{') {
                openalex::read_json(input)
            } else {
                openalex::fetch(input)
            }
        }
        "ror" => {
            if input.trim_start().starts_with('{') {
                ror::read_json(input)
            } else {
                ror::fetch(input)
            }
        }
        "vraix" => vraix::read(input),
        other => Err(Error::UnsupportedFormat(other.to_string())),
    }
}

pub fn write(format: &str, data: &Data) -> Result<Vec<u8>> {
    write_citation(format, data, None, None)
}

pub fn write_citation(
    format: &str,
    data: &Data,
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<u8>> {
    match format {
        "commonmeta" => commonmeta::write(data),
        "crossref_xml" => crossref_xml::write(data),
        "ris" => ris::write(data),
        "csl" => csl::write(data),
        "datacite" => datacite::write(data),
        "inveniordm" => inveniordm::write(data),
        "bibtex" => bibtex::write(data),
        "schemaorg" => schemaorg::write(data),
        "citation" => citation::write(data, style, locale),
        "ror" => ror::write(data),
        other => Err(Error::UnsupportedFormat(other.to_string())),
    }
}

pub fn write_all_citation(
    format: &str,
    list: &[Data],
    style: Option<&str>,
    locale: Option<&str>,
) -> Result<Vec<u8>> {
    match format {
        "commonmeta" => commonmeta::write_all(list),
        "csl" => csl::write_all(list),
        "datacite" => datacite::write_all(list),
        "inveniordm" => inveniordm::write_all(list),
        "schemaorg" => schemaorg::write_all(list),
        "ror" => ror::write_json_all(list),
        "citation" => citation::write_all(list, style, locale),
        "crossref_xml" => crossref_xml::write_all(list),
        other => Err(Error::UnsupportedFormat(other.to_string())),
    }
}
