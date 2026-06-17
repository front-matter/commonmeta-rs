use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

fn null_as_empty<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

use crate::data::{
    Affiliation, Container, Contributor, Data, Date, Description, File, FundingReference,
    Identifier, License, Publisher, Reference, Subject, Title,
};
use crate::doi_utils::normalize_doi;
use crate::error::{Error, Result};
use crate::utils::{normalize_orcid, normalize_ror, validate_id, validate_openalex};

// ── OpenAlex API structs ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Work {
    #[serde(default)]
    id: String,
    #[serde(default)]
    doi: String,
    #[serde(default)]
    display_name: String,
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(default)]
    type_crossref: String,
    #[serde(default)]
    publication_date: String,
    #[serde(default)]
    created_date: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    language: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    version: String,
    #[serde(default)]
    abstract_inverted_index: HashMap<String, Vec<usize>>,
    #[serde(default)]
    authorships: Vec<Authorship>,
    #[serde(default)]
    ids: HashMap<String, String>,
    #[serde(default)]
    primary_location: Location,
    #[serde(default)]
    best_oa_location: Location,
    #[serde(default)]
    topics: Vec<Topic>,
    #[serde(default)]
    biblio: Biblio,
    #[serde(default)]
    referenced_works: Vec<String>,
    #[serde(default)]
    grants: Vec<Grant>,
}

#[derive(Debug, Default, Deserialize)]
struct Authorship {
    #[serde(default)]
    author: Author,
    #[serde(default)]
    institutions: Vec<Institution>,
}

#[derive(Debug, Default, Deserialize)]
struct Author {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    orcid: String,
}

#[derive(Debug, Default, Deserialize)]
struct Institution {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    ror: String,
}

#[derive(Debug, Default, Deserialize)]
struct Location {
    #[serde(default)]
    source: Option<Source>,
    #[serde(default, deserialize_with = "null_as_empty")]
    pdf_url: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    landing_page_url: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    license: String,
}

#[derive(Debug, Default, Deserialize)]
struct Source {
    #[serde(default)]
    id: String,
    #[serde(rename = "type", default)]
    type_: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    issn_l: String,
    #[serde(default)]
    host_organization_name: String,
}

#[derive(Debug, Default, Deserialize)]
struct Topic {
    #[serde(default)]
    subfield: Subfield,
}

#[derive(Debug, Default, Deserialize)]
struct Subfield {
    #[serde(default)]
    display_name: String,
}

#[derive(Debug, Default, Deserialize)]
struct Biblio {
    #[serde(default, deserialize_with = "null_as_empty")]
    volume: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    issue: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    first_page: String,
    #[serde(default, deserialize_with = "null_as_empty")]
    last_page: String,
}

#[derive(Debug, Default, Deserialize)]
struct Grant {
    #[serde(default)]
    funder: String,
    #[serde(default)]
    funder_display_name: String,
    #[serde(default)]
    award_id: String,
}

#[derive(Debug, Deserialize)]
struct OAListResponse {
    #[serde(default)]
    results: Vec<Work>,
}

// ── Type mappings ─────────────────────────────────────────────────────────────

fn oa_to_cm_type(oa_type: &str) -> &'static str {
    match oa_type {
        "article" => "Article",
        "book" => "Book",
        "book-chapter" => "BookChapter",
        "dataset" => "Dataset",
        "dissertation" => "Dissertation",
        "editorial" => "Document",
        "erratum" => "Other",
        "grant" => "Grant",
        "letter" => "Article",
        "libguides" => "InteractiveResource",
        "other" => "Other",
        "paratext" => "Component",
        "peer-review" => "PeerReview",
        "preprint" => "Article",
        "reference-entry" => "Other",
        "report" => "Report",
        "retraction" => "Other",
        "review" => "Article",
        "standard" => "Standard",
        "supplementary-materials" => "Component",
        _ => "Other",
    }
}

fn crossref_to_cm_type(cr_type: &str) -> &'static str {
    match cr_type {
        "journal-article" => "JournalArticle",
        "book-chapter" => "BookChapter",
        "book" | "monograph" | "edited-book" | "reference-book" => "Book",
        "proceedings-article" => "ProceedingsArticle",
        "proceedings" | "conference" => "Proceedings",
        "dataset" => "Dataset",
        "dissertation" => "Dissertation",
        "posted-content" => "Preprint",
        "report" | "report-series" => "Report",
        "peer-review" => "PeerReview",
        "reference-entry" => "Entry",
        "journal" => "Journal",
        "grant" => "Grant",
        "standard" => "Standard",
        _ => "",
    }
}

fn oa_license_to_spdx(license: &str) -> &'static str {
    match license {
        "cc-by" => "CC-BY-4.0",
        "cc0" => "CC0-1.0",
        "cc-by-sa" => "CC-BY-SA-4.0",
        "cc-by-nc" => "CC-BY-NC-4.0",
        "cc-by-nd" => "CC-BY-ND-4.0",
        "cc-by-nc-sa" => "CC-BY-NC-SA-4.0",
        "cc-by-nc-nd" => "CC-BY-NC-ND-4.0",
        _ => "",
    }
}

fn oa_source_to_container_type(source_type: &str) -> &'static str {
    match source_type {
        "journal" => "Journal",
        "repository" => "Repository",
        "conference" => "Proceedings",
        "ebook platform" => "Book",
        "book series" => "BookSeries",
        _ => "",
    }
}

fn oa_identifier_type(key: &str) -> &'static str {
    match key {
        "openalex" => "OpenAlex",
        "doi" => "DOI",
        "mag" => "MAG",
        "pmid" => "PMID",
        "pmcid" => "PMCID",
        _ => "",
    }
}

// ── Abstract reconstruction ───────────────────────────────────────────────────

fn get_abstract(index: &HashMap<String, Vec<usize>>) -> String {
    if index.is_empty() {
        return String::new();
    }
    // Find the maximum position to size the buffer
    let max_pos = index
        .values()
        .flat_map(|positions| positions.iter().copied())
        .max()
        .unwrap_or(0);

    let mut words: Vec<&str> = vec![""; max_pos + 1];
    for (word, positions) in index {
        for &pos in positions {
            if pos < words.len() {
                words[pos] = word.as_str();
            }
        }
    }
    // Filter out any unfilled slots (empty strings)
    words
        .into_iter()
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Contributor parsing ───────────────────────────────────────────────────────

fn get_contributors(authorships: &[Authorship]) -> Vec<Contributor> {
    authorships
        .iter()
        .map(|a| {
            let name = &a.author.display_name;
            let orcid = normalize_orcid(&a.author.orcid);

            // Split display_name at last space → given + family
            let (given_name, family_name) = if let Some(pos) = name.rfind(' ') {
                (name[..pos].to_string(), name[pos + 1..].to_string())
            } else {
                (String::new(), name.clone())
            };

            let affiliations: Vec<Affiliation> = a
                .institutions
                .iter()
                .filter(|inst| !inst.display_name.is_empty())
                .map(|inst| {
                    let id = if !inst.ror.is_empty() {
                        normalize_ror(&inst.ror)
                    } else {
                        String::new()
                    };
                    Affiliation {
                        id,
                        name: inst.display_name.clone(),
                        ..Default::default()
                    }
                })
                .collect();

            Contributor {
                id: orcid,
                type_: "Person".to_string(),
                given_name,
                family_name,
                affiliations,
                contributor_roles: vec!["Author".to_string()],
                ..Default::default()
            }
        })
        .collect()
}

// ── Core conversion ───────────────────────────────────────────────────────────

fn from_work(work: Work) -> Data {
    // ID: DOI if present, else OpenAlex work URL
    let id = if !work.doi.is_empty() {
        normalize_doi(&work.doi)
    } else if !work.id.is_empty() {
        work.id.clone()
    } else {
        String::new()
    };

    // URL from primary_location or work.id
    let url = if !work.primary_location.landing_page_url.is_empty() {
        work.primary_location.landing_page_url.clone()
    } else if !work.id.is_empty() {
        work.id.clone()
    } else {
        String::new()
    };

    // Type: type_crossref first, then type with OA mappings, default "Other"
    let type_ = if !work.type_crossref.is_empty() {
        let mapped = crossref_to_cm_type(&work.type_crossref);
        if !mapped.is_empty() { mapped } else { oa_to_cm_type(&work.type_) }
    } else if !work.type_.is_empty() {
        oa_to_cm_type(&work.type_)
    } else {
        "Other"
    }
    .to_string();

    // Title from display_name
    let titles = if !work.display_name.is_empty() {
        vec![Title { title: work.display_name.clone(), ..Default::default() }]
    } else {
        vec![]
    };

    // Dates
    let date = Date {
        published: work.publication_date.clone(),
        created: work.created_date.clone(),
        ..Default::default()
    };

    // Contributors from authorships
    let contributors = get_contributors(&work.authorships);

    // Publisher from primary_location.source.host_organization_name
    let publisher = work
        .primary_location
        .source
        .as_ref()
        .filter(|s| !s.host_organization_name.is_empty())
        .map(|s| Publisher { name: s.host_organization_name.clone(), ..Default::default() })
        .unwrap_or_default();

    // Abstract from inverted index
    let abstract_text = get_abstract(&work.abstract_inverted_index);
    let descriptions = if !abstract_text.is_empty() {
        vec![Description {
            description: abstract_text,
            type_: "Abstract".to_string(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

    // License from best_oa_location
    let license = {
        let spdx = oa_license_to_spdx(&work.best_oa_location.license);
        if !spdx.is_empty() {
            License { id: spdx.to_string(), ..Default::default() }
        } else {
            License::default()
        }
    };

    // Container from primary_location.source + biblio
    let container = match work.primary_location.source.as_ref().filter(|s| !s.display_name.is_empty()) {
        Some(src) => {
            let container_type = oa_source_to_container_type(&src.type_).to_string();
            let (identifier, identifier_type) = if !src.issn_l.is_empty() {
                (src.issn_l.clone(), "ISSN".to_string())
            } else if !src.id.is_empty() {
                (src.id.clone(), "URL".to_string())
            } else {
                (String::new(), String::new())
            };
            Container {
                type_: container_type,
                title: src.display_name.clone(),
                identifier,
                identifier_type,
                volume: work.biblio.volume.clone(),
                issue: work.biblio.issue.clone(),
                first_page: work.biblio.first_page.clone(),
                last_page: work.biblio.last_page.clone(),
                ..Default::default()
            }
        }
        None => Container::default(),
    };

    // Identifiers from work.ids map
    let identifiers: Vec<Identifier> = {
        let mut order = vec!["openalex", "doi", "pmid", "pmcid", "mag"];
        order.retain(|k| work.ids.contains_key(*k));
        order
            .into_iter()
            .filter_map(|key| {
                let value = work.ids.get(key)?;
                let id_type = oa_identifier_type(key);
                if id_type.is_empty() || value.is_empty() {
                    return None;
                }
                Some(Identifier {
                    identifier: value.clone(),
                    identifier_type: id_type.to_string(),
                })
            })
            .collect()
    };

    // Subjects: deduplicated subfield display names from topics
    let subjects: Vec<Subject> = {
        let mut seen = std::collections::HashSet::new();
        work.topics
            .iter()
            .filter(|t| !t.subfield.display_name.is_empty())
            .filter_map(|t| {
                if seen.insert(t.subfield.display_name.clone()) {
                    Some(Subject { subject: t.subfield.display_name.clone() })
                } else {
                    None
                }
            })
            .collect()
    };

    // Files from best_oa_location.pdf_url
    let files = if !work.best_oa_location.pdf_url.is_empty() {
        vec![File {
            url: work.best_oa_location.pdf_url.clone(),
            mime_type: "application/pdf".to_string(),
            ..Default::default()
        }]
    } else {
        vec![]
    };

    // References from referenced_works (OpenAlex IDs — no extra HTTP calls)
    let references: Vec<Reference> = work
        .referenced_works
        .iter()
        .filter_map(|oa_url| {
            validate_openalex(oa_url).map(|id| Reference {
                id: format!("https://openalex.org/{}", id),
                ..Default::default()
            })
        })
        .collect();

    // Funding from grants
    let funding_references: Vec<FundingReference> = work
        .grants
        .iter()
        .map(|g| {
            let funder_id = normalize_ror(&g.funder);
            FundingReference {
                funder_identifier: funder_id,
                funder_identifier_type: if !g.funder.is_empty() {
                    "ROR".to_string()
                } else {
                    String::new()
                },
                funder_name: g.funder_display_name.clone(),
                award_number: g.award_id.clone(),
                ..Default::default()
            }
        })
        .filter(|f| !f.funder_name.is_empty())
        .collect();

    Data {
        id,
        type_,
        url,
        titles,
        contributors,
        date,
        publisher,
        descriptions,
        license,
        container,
        identifiers,
        subjects,
        files,
        references,
        funding_references,
        language: work.language.clone(),
        version: work.version.clone(),
        provider: "OpenAlex".to_string(),
        ..Data::default()
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

pub fn read_json(input: &str) -> Result<Data> {
    let work: Work = serde_json::from_str(input).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(from_work(work))
}

fn build_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
}

fn fetch_work(client: &reqwest::blocking::Client, api_url: &str) -> Result<Work> {
    let resp = client
        .get(api_url)
        .send()
        .map_err(|e| Error::Http(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Http(e.to_string()))?;

    // Check if this is a list response (filter queries) or a single work
    let text = resp.text().map_err(|e| Error::Http(e.to_string()))?;
    if text.trim_start().starts_with(r#"{"meta""#)
        || text.contains(r#""results":"#)
    {
        let list: OAListResponse =
            serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))?;
        list.results
            .into_iter()
            .next()
            .ok_or_else(|| Error::Parse("No results found in OpenAlex response".to_string()))
    } else {
        serde_json::from_str(&text).map_err(|e| Error::Parse(e.to_string()))
    }
}

/// Fetch an OpenAlex work by DOI, OpenAlex ID, PMID, or PMCID.
pub fn fetch(input: &str) -> Result<Data> {
    let client = build_client().map_err(|e| Error::Http(e.to_string()))?;

    let (id, id_type) = validate_id(input);

    let api_url = match id_type {
        "DOI" => {
            // OpenAlex accepts the full DOI URL
            let doi_url = normalize_doi(&id);
            format!("https://api.openalex.org/works/{}", doi_url)
        }
        "OpenAlex" => {
            format!("https://api.openalex.org/works/{}", id)
        }
        "PMID" => {
            format!("https://api.openalex.org/works?filter=ids.pmid:{}", id)
        }
        "PMCID" => {
            // OpenAlex expects PMC prefix
            let pmcid = if id.starts_with("PMC") { id.clone() } else { format!("PMC{}", id) };
            format!("https://api.openalex.org/works?filter=ids.pmcid:{}", pmcid)
        }
        _ => {
            // Try as OpenAlex ID directly
            if let Some(oa_id) = validate_openalex(input) {
                format!("https://api.openalex.org/works/{}", oa_id)
            } else {
                return Err(Error::Parse(format!(
                    "Cannot construct OpenAlex API URL from: {}",
                    input
                )));
            }
        }
    };

    let work = fetch_work(&client, &api_url)?;
    Ok(from_work(work))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OA_WORK: &str = r#"{
  "id": "https://openalex.org/W2741809807",
  "doi": "https://doi.org/10.7717/peerj.4375",
  "display_name": "The state of OA: a large-scale analysis of the prevalence and impact of Open Access articles",
  "type": "article",
  "type_crossref": "journal-article",
  "publication_date": "2018-02-13",
  "created_date": "2017-11-09",
  "language": "en",
  "version": null,
  "abstract_inverted_index": {
    "Despite": [0],
    "growing": [1],
    "interest": [2],
    "in": [3, 57],
    "Open": [4],
    "Access": [5]
  },
  "authorships": [
    {
      "author": {
        "display_name": "Heather Piwowar",
        "orcid": "https://orcid.org/0000-0003-1613-5981"
      },
      "institutions": [
        {
          "display_name": "Impactstory",
          "ror": "https://ror.org/02nr0ka47"
        }
      ]
    },
    {
      "author": {
        "display_name": "Jason Priem",
        "orcid": ""
      },
      "institutions": []
    }
  ],
  "ids": {
    "openalex": "https://openalex.org/W2741809807",
    "doi": "https://doi.org/10.7717/peerj.4375",
    "pmid": "https://pubmed.ncbi.nlm.nih.gov/29456894"
  },
  "primary_location": {
    "source": {
      "id": "https://openalex.org/S1983995261",
      "type": "journal",
      "display_name": "PeerJ",
      "issn_l": "2167-8359",
      "host_organization_name": "PeerJ"
    },
    "landing_page_url": "https://peerj.com/articles/4375",
    "pdf_url": null,
    "license": null
  },
  "best_oa_location": {
    "source": null,
    "pdf_url": "https://peerj.com/articles/4375.pdf",
    "landing_page_url": "https://peerj.com/articles/4375",
    "license": "cc-by"
  },
  "topics": [
    {"subfield": {"display_name": "Library and Information Sciences"}},
    {"subfield": {"display_name": "Computer Science Applications"}},
    {"subfield": {"display_name": "Library and Information Sciences"}}
  ],
  "biblio": {
    "volume": "6",
    "issue": null,
    "first_page": "e4375",
    "last_page": "e4375"
  },
  "referenced_works": [
    "https://openalex.org/W2141540132",
    "https://openalex.org/W2144745932"
  ],
  "grants": [
    {
      "funder": "https://ror.org/021nxhr62",
      "funder_display_name": "Alfred P. Sloan Foundation",
      "award_id": "G-2014-13918"
    }
  ]
}"#;

    #[test]
    fn test_read_openalex_basic() {
        let data = read_json(OA_WORK).unwrap();

        assert_eq!(data.id, "https://doi.org/10.7717/peerj.4375");
        assert_eq!(data.type_, "JournalArticle");
        assert_eq!(data.provider, "OpenAlex");
        assert_eq!(
            data.titles[0].title,
            "The state of OA: a large-scale analysis of the prevalence and impact of Open Access articles"
        );
    }

    #[test]
    fn test_openalex_dates() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.date.published, "2018-02-13");
        assert_eq!(data.date.created, "2017-11-09");
    }

    #[test]
    fn test_openalex_contributors() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.contributors.len(), 2);

        let author = &data.contributors[0];
        assert_eq!(author.given_name, "Heather");
        assert_eq!(author.family_name, "Piwowar");
        assert_eq!(author.id, "https://orcid.org/0000-0003-1613-5981");
        assert_eq!(author.affiliations[0].name, "Impactstory");
        assert!(author.contributor_roles.contains(&"Author".to_string()));

        let author2 = &data.contributors[1];
        assert_eq!(author2.given_name, "Jason");
        assert_eq!(author2.family_name, "Priem");
        assert!(author2.id.is_empty());
    }

    #[test]
    fn test_openalex_container() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.container.type_, "Journal");
        assert_eq!(data.container.title, "PeerJ");
        assert_eq!(data.container.identifier, "2167-8359");
        assert_eq!(data.container.identifier_type, "ISSN");
        assert_eq!(data.container.volume, "6");
        assert_eq!(data.container.first_page, "e4375");
    }

    #[test]
    fn test_openalex_license() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.license.id, "CC-BY-4.0");
    }

    #[test]
    fn test_openalex_abstract() {
        let data = read_json(OA_WORK).unwrap();
        assert!(!data.descriptions.is_empty());
        let abstract_text = &data.descriptions[0].description;
        // The inverted index has "Despite growing interest in Open Access"
        // position 3 ("in") appears twice (pos 3 and 57) — only 6 words filled slots 0-5
        assert!(abstract_text.contains("Despite"));
        assert!(abstract_text.contains("Open"));
        assert!(abstract_text.contains("Access"));
    }

    #[test]
    fn test_openalex_subjects_dedup() {
        let data = read_json(OA_WORK).unwrap();
        // Topics has "Library and Information Sciences" twice — should be deduplicated
        assert_eq!(data.subjects.len(), 2);
        assert_eq!(data.subjects[0].subject, "Library and Information Sciences");
        assert_eq!(data.subjects[1].subject, "Computer Science Applications");
    }

    #[test]
    fn test_openalex_files() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.files.len(), 1);
        assert_eq!(data.files[0].url, "https://peerj.com/articles/4375.pdf");
        assert_eq!(data.files[0].mime_type, "application/pdf");
    }

    #[test]
    fn test_openalex_references() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.references.len(), 2);
        assert_eq!(data.references[0].id, "https://openalex.org/W2141540132");
    }

    #[test]
    fn test_openalex_funding() {
        let data = read_json(OA_WORK).unwrap();
        assert_eq!(data.funding_references.len(), 1);
        assert_eq!(data.funding_references[0].funder_name, "Alfred P. Sloan Foundation");
        assert_eq!(data.funding_references[0].award_number, "G-2014-13918");
    }

    #[test]
    fn test_get_abstract() {
        let mut index = HashMap::new();
        index.insert("Hello".to_string(), vec![0]);
        index.insert("world".to_string(), vec![1]);
        index.insert("foo".to_string(), vec![2]);
        let result = get_abstract(&index);
        assert_eq!(result, "Hello world foo");
    }

    #[test]
    fn test_oa_to_cm_type() {
        assert_eq!(oa_to_cm_type("article"), "Article");
        assert_eq!(oa_to_cm_type("preprint"), "Article");
        assert_eq!(oa_to_cm_type("book-chapter"), "BookChapter");
        assert_eq!(oa_to_cm_type("dissertation"), "Dissertation");
        assert_eq!(oa_to_cm_type("unknown"), "Other");
    }

    #[test]
    fn test_oa_license_to_spdx() {
        assert_eq!(oa_license_to_spdx("cc-by"), "CC-BY-4.0");
        assert_eq!(oa_license_to_spdx("cc0"), "CC0-1.0");
        assert_eq!(oa_license_to_spdx("proprietary"), "");
    }
}
