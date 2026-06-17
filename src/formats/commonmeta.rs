use serde::Serialize;

use crate::data::Data;
use crate::error::{Error, Result};

pub fn read(json: &str) -> Result<Data> {
    serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    serde_json::to_vec(data).map_err(|e| Error::Serialize(e.to_string()))
}

// ── Bulk Parquet writer (catalog dumps) ───────────────────────────────────────
//
// Parquet needs a flat, scalar schema, but `Data` is deeply nested (titles,
// contributors, identifiers, etc. are all lists). `CommonmetaRow` is a lossy
// tabular projection of the fields most useful for analysis, in the same
// spirit as the `RorCsv` flattening in ror.rs.

/// A flattened, Parquet-friendly view of a single commonmeta `Data` record.
#[derive(Debug, Default, Clone, Serialize, parquet_derive::ParquetRecordWriter)]
pub struct CommonmetaRow {
    pub id: String,
    pub record_type: String,
    pub title: String,
    pub url: String,
    pub doi: String,
    pub publisher: String,
    pub language: String,
    pub version: String,
    pub license: String,
    pub container_title: String,
    pub container_type: String,
    pub volume: String,
    pub issue: String,
    pub first_page: String,
    pub last_page: String,
    pub date_published: String,
    pub date_created: String,
    pub date_updated: String,
    pub contributor_count: i32,
    pub first_author_name: String,
    pub first_author_orcid: String,
    pub subjects: String,
    pub description: String,
    pub provider: String,
    pub additional_type: String,
}

fn contributor_name(contributor: &crate::data::Contributor) -> String {
    if !contributor.name.is_empty() {
        return contributor.name.clone();
    }
    format!("{} {}", contributor.given_name, contributor.family_name)
        .trim()
        .to_string()
}

/// Flatten a `Data` record into its tabular `CommonmetaRow` representation.
fn flatten_row(data: &Data) -> CommonmetaRow {
    let title = data.titles.first().map(|t| t.title.clone()).unwrap_or_default();

    let doi = data
        .identifiers
        .iter()
        .find(|i| i.identifier_type == "DOI")
        .map(|i| i.identifier.clone())
        .unwrap_or_else(|| {
            if data.id.contains("doi.org") { data.id.clone() } else { String::new() }
        });

    let (first_author_name, first_author_orcid) = data
        .contributors
        .first()
        .map(|c| (contributor_name(c), c.id.clone()))
        .unwrap_or_default();

    let subjects = data
        .subjects
        .iter()
        .map(|s| s.subject.as_str())
        .collect::<Vec<_>>()
        .join("; ");

    let description = data.descriptions.first().map(|d| d.description.clone()).unwrap_or_default();

    CommonmetaRow {
        id: data.id.clone(),
        record_type: data.type_.clone(),
        title,
        url: data.url.clone(),
        doi,
        publisher: data.publisher.name.clone(),
        language: data.language.clone(),
        version: data.version.clone(),
        license: data.license.id.clone(),
        container_title: data.container.title.clone(),
        container_type: data.container.type_.clone(),
        volume: data.container.volume.clone(),
        issue: data.container.issue.clone(),
        first_page: data.container.first_page.clone(),
        last_page: data.container.last_page.clone(),
        date_published: data.date.published.clone(),
        date_created: data.date.created.clone(),
        date_updated: data.date.updated.clone(),
        contributor_count: data.contributors.len() as i32,
        first_author_name,
        first_author_orcid,
        subjects,
        description,
        provider: data.provider.clone(),
        additional_type: data.additional_type.clone(),
    }
}

/// Write a list of commonmeta records as Parquet using the flattened
/// `CommonmetaRow` schema.
pub fn write_parquet_all(list: &[Data]) -> Result<Vec<u8>> {
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::record::RecordWriter;

    let rows: Vec<CommonmetaRow> = list.iter().map(flatten_row).collect();
    let schema = rows.as_slice().schema().map_err(|e| Error::Serialize(e.to_string()))?;
    let props = std::sync::Arc::new(WriterProperties::builder().build());

    let buffer: Vec<u8> = Vec::new();
    let mut writer = SerializedFileWriter::new(buffer, schema, props)
        .map_err(|e| Error::Serialize(e.to_string()))?;

    let mut row_group = writer.next_row_group().map_err(|e| Error::Serialize(e.to_string()))?;
    rows.as_slice()
        .write_to_row_group(&mut row_group)
        .map_err(|e| Error::Serialize(e.to_string()))?;
    row_group.close().map_err(|e| Error::Serialize(e.to_string()))?;

    writer.into_inner().map_err(|e| Error::Serialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{Contributor, Identifier, Title};

    fn sample_data() -> Data {
        Data {
            id: "https://doi.org/10.1234/abc".to_string(),
            type_: "JournalArticle".to_string(),
            titles: vec![Title { title: "A Sample Title".to_string(), ..Default::default() }],
            identifiers: vec![Identifier {
                identifier: "10.1234/abc".to_string(),
                identifier_type: "DOI".to_string(),
            }],
            contributors: vec![Contributor {
                given_name: "Jane".to_string(),
                family_name: "Doe".to_string(),
                id: "https://orcid.org/0000-0002-1825-0097".to_string(),
                ..Default::default()
            }],
            ..Data::default()
        }
    }

    #[test]
    fn test_flatten_row_basic() {
        let row = flatten_row(&sample_data());
        assert_eq!(row.id, "https://doi.org/10.1234/abc");
        assert_eq!(row.record_type, "JournalArticle");
        assert_eq!(row.title, "A Sample Title");
        assert_eq!(row.doi, "10.1234/abc");
        assert_eq!(row.first_author_name, "Jane Doe");
        assert_eq!(row.first_author_orcid, "https://orcid.org/0000-0002-1825-0097");
        assert_eq!(row.contributor_count, 1);
    }

    #[test]
    fn test_flatten_row_doi_fallback_from_id() {
        let mut data = sample_data();
        data.identifiers.clear();
        let row = flatten_row(&data);
        assert_eq!(row.doi, "https://doi.org/10.1234/abc");
    }

    #[test]
    fn test_write_parquet_all_roundtrip() {
        let list = vec![sample_data()];
        let bytes = write_parquet_all(&list).unwrap();
        assert!(!bytes.is_empty());
        assert_eq!(&bytes[0..4], b"PAR1");
        assert_eq!(&bytes[bytes.len() - 4..], b"PAR1");
    }

    #[test]
    fn test_write_parquet_all_empty() {
        let list: Vec<Data> = vec![];
        let bytes = write_parquet_all(&list).unwrap();
        assert_eq!(&bytes[0..4], b"PAR1");
    }

    #[test]
    fn test_write_parquet_all_readable_schema_and_rows() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let list = vec![sample_data(), sample_data()];
        let bytes = write_parquet_all(&list).unwrap();

        let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes)).unwrap();
        let metadata = reader.metadata();
        assert_eq!(metadata.file_metadata().num_rows(), 2);

        let schema = metadata.file_metadata().schema_descr();
        let column_names: Vec<String> = (0..schema.num_columns())
            .map(|i| schema.column(i).name().to_string())
            .collect();
        assert!(column_names.iter().any(|c| c == "id"));
        assert!(column_names.iter().any(|c| c == "record_type"));
        assert!(column_names.iter().any(|c| c == "title"));
        assert!(column_names.iter().any(|c| c == "doi"));
        assert!(column_names.iter().any(|c| c == "first_author_name"));
    }
}
