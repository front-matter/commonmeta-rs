pub mod bibtex;
pub mod citation;
pub mod commonmeta;
pub mod crossref;
pub mod crossref_xml;
pub mod csl;
pub mod datacite;
pub mod jsonfeed;
pub mod schemaorg;

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
        "csl" => csl::write(data),
        "datacite" => datacite::write(data),
        "bibtex" => bibtex::write(data),
        "schemaorg" => schemaorg::write(data),
        "citation" => citation::write(data, style, locale),
        other => Err(Error::UnsupportedFormat(other.to_string())),
    }
}
