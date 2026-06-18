use serde::Serialize;

use crate::data::Data;
use crate::error::{Error, Result};

pub fn read(json: &str) -> Result<Data> {
    serde_json::from_str(json).map_err(|e| Error::Parse(e.to_string()))
}

pub fn write(data: &Data) -> Result<Vec<u8>> {
    let mut sanitized = data.clone();
    for contributor in &mut sanitized.contributors {
        if contributor.type_ == "Person" {
            contributor.name.clear();
        }
    }

    serde_json::to_vec(&sanitized).map_err(|e| Error::Serialize(e.to_string()))
}

// ── Bulk Parquet writer (catalog dumps) ───────────────────────────────────────
//
// Parquet needs a flat, scalar schema, but `Data` is deeply nested (titles,
// contributors, identifiers, etc. are all lists). `CommonmetaRow` flattens
// the fields most useful for analysis/filtering (e.g. in DuckDB) without
// needing to parse JSON, in the same spirit as the `RorCsv` flattening in
// ror.rs — but unlike that one, it also carries a `json` column with the
// complete record's JSON serialization, so `read_parquet_all` can
// reconstruct the original `Data` exactly rather than just the flattened
// subset. The other columns are a queryable convenience layer on top of
// that, not the source of truth.

/// A flattened, Parquet-friendly view of a single commonmeta `Data` record.
#[derive(
    Debug, Default, Clone, Serialize, parquet_derive::ParquetRecordWriter, parquet_derive::ParquetRecordReader,
)]
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
    /// Complete JSON serialization of the original `Data` record. The
    /// authoritative source for `read_parquet_all`; the columns above exist
    /// for filtering/analysis without needing to parse this.
    pub json: String,
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

    let json = serde_json::to_string(data).unwrap_or_default();

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
        json,
    }
}

/// Write a list of commonmeta records as Parquet using the flattened
/// `CommonmetaRow` schema.
/// Records per Parquet row group. `flatten_row` is the CPU-heavy step here
/// (each row's `json` column is a full JSON serialization of the original
/// record), so it's parallelized across chunks of this size; the resulting
/// row groups are then written into the output sequentially, since Parquet
/// row-group data has to land in the underlying buffer in order. A single
/// file with multiple row groups is normal Parquet practice, not a
/// workaround — unlike writing one row group per output *file*, which is
/// what `cmd::list` used to do before merging this batching in here.
const ROW_GROUP_SIZE: usize = 100_000;

pub fn write_parquet_all(list: &[Data]) -> Result<Vec<u8>> {
    write_parquet_chunked(list, ROW_GROUP_SIZE)
}

/// `write_parquet_all`, parameterized over the row-group size so tests can
/// force multiple row groups without constructing 100,000+ records.
fn write_parquet_chunked(list: &[Data], row_group_size: usize) -> Result<Vec<u8>> {
    use parquet::file::properties::WriterProperties;
    use parquet::file::writer::SerializedFileWriter;
    use parquet::record::RecordWriter;

    let chunks: Vec<&[Data]> =
        if list.is_empty() { vec![&[][..]] } else { list.chunks(row_group_size).collect() };

    let row_chunks: Vec<Vec<CommonmetaRow>> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || chunk.iter().map(flatten_row).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().map_err(|_| Error::Serialize("parquet flatten thread panicked".to_string())))
            .collect::<Result<Vec<_>>>()
    })?;

    let schema = row_chunks[0].as_slice().schema().map_err(|e| Error::Serialize(e.to_string()))?;
    let props = std::sync::Arc::new(WriterProperties::builder().build());

    let buffer: Vec<u8> = Vec::new();
    let mut writer = SerializedFileWriter::new(buffer, schema, props)
        .map_err(|e| Error::Serialize(e.to_string()))?;

    for rows in &row_chunks {
        let mut row_group = writer.next_row_group().map_err(|e| Error::Serialize(e.to_string()))?;
        rows.as_slice()
            .write_to_row_group(&mut row_group)
            .map_err(|e| Error::Serialize(e.to_string()))?;
        row_group.close().map_err(|e| Error::Serialize(e.to_string()))?;
    }

    writer.into_inner().map_err(|e| Error::Serialize(e.to_string()))
}

/// Reconstruct a `Data` record from a `CommonmetaRow`.
///
/// Prefers the `json` column, which holds the complete original record, so
/// the round trip through Parquet is lossless. Falls back to rebuilding from
/// the flattened columns (the inverse of `flatten_row`, lossy in the same
/// direction: only the fields captured there, e.g. the first author, first
/// title, are restored) for Parquet files written before the `json` column
/// existed, or if it's somehow empty/invalid.
fn unflatten_row(row: &CommonmetaRow) -> Data {
    if !row.json.is_empty()
        && let Ok(data) = serde_json::from_str::<Data>(&row.json)
    {
        return data;
    }
    unflatten_row_lossy(row)
}

fn unflatten_row_lossy(row: &CommonmetaRow) -> Data {
    Data {
        id: row.id.clone(),
        type_: row.record_type.clone(),
        additional_type: row.additional_type.clone(),
        titles: if row.title.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Title { title: row.title.clone(), ..Default::default() }]
        },
        url: row.url.clone(),
        identifiers: if row.doi.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Identifier {
                identifier: row.doi.clone(),
                identifier_type: "DOI".to_string(),
            }]
        },
        publisher: crate::data::Publisher { name: row.publisher.clone(), ..Default::default() },
        language: row.language.clone(),
        version: row.version.clone(),
        license: crate::data::License { id: row.license.clone(), ..Default::default() },
        container: crate::data::Container {
            title: row.container_title.clone(),
            type_: row.container_type.clone(),
            volume: row.volume.clone(),
            issue: row.issue.clone(),
            first_page: row.first_page.clone(),
            last_page: row.last_page.clone(),
            ..Default::default()
        },
        date: crate::data::Date {
            published: row.date_published.clone(),
            created: row.date_created.clone(),
            updated: row.date_updated.clone(),
            ..Default::default()
        },
        contributors: if row.first_author_name.is_empty() && row.first_author_orcid.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Contributor {
                id: row.first_author_orcid.clone(),
                name: row.first_author_name.clone(),
                ..Default::default()
            }]
        },
        subjects: row
            .subjects
            .split("; ")
            .filter(|s| !s.is_empty())
            .map(|s| crate::data::Subject { subject: s.to_string() })
            .collect(),
        descriptions: if row.description.is_empty() {
            Vec::new()
        } else {
            vec![crate::data::Description { description: row.description.clone(), ..Default::default() }]
        },
        provider: row.provider.clone(),
        ..Default::default()
    }
}

/// Read a list of commonmeta records back from the `CommonmetaRow` Parquet
/// schema written by `write_parquet_all`. Lossless: each record is restored
/// from its `json` column, the complete original serialization.
pub fn read_parquet_all(bytes: &[u8]) -> Result<Vec<Data>> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::RecordReader;

    let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes.to_vec()))
        .map_err(|e| Error::Parse(e.to_string()))?;

    let mut rows: Vec<CommonmetaRow> = Vec::new();
    for i in 0..reader.num_row_groups() {
        let mut row_group_reader =
            reader.get_row_group(i).map_err(|e| Error::Parse(e.to_string()))?;
        let num_rows = row_group_reader.metadata().num_rows() as usize;
        rows.read_from_row_group(&mut *row_group_reader, num_rows)
            .map_err(|e| Error::Parse(e.to_string()))?;
    }

    Ok(rows.iter().map(unflatten_row).collect())
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

    #[test]
    fn test_write_parquet_chunked_uses_multiple_row_groups_in_one_file() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let list = vec![sample_data(), sample_data(), sample_data()];
        // row_group_size=1 forces 3 row groups without needing 100,000+ rows.
        let bytes = write_parquet_chunked(&list, 1).unwrap();

        let reader = SerializedFileReader::new(::bytes::Bytes::from(bytes.clone())).unwrap();
        assert_eq!(reader.num_row_groups(), 3);
        assert_eq!(reader.metadata().file_metadata().num_rows(), 3);

        // A multi-row-group file is still a single, fully readable Parquet
        // file: read_parquet_all already loops over every row group.
        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert_eq!(roundtripped.len(), 3);
    }

    #[test]
    fn test_write_read_parquet_roundtrip() {
        let list = vec![sample_data()];
        let bytes = write_parquet_all(&list).unwrap();

        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert_eq!(roundtripped.len(), 1);
        // Lossless: the round-tripped record is byte-for-byte identical to
        // the original, not just the fields the flattened columns capture.
        assert_eq!(roundtripped[0], list[0]);
    }

    #[test]
    fn test_write_read_parquet_roundtrip_preserves_fields_outside_flattened_view() {
        use crate::data::{Affiliation, Description, Subject, Title};

        let mut data = sample_data();
        // Fields the old flattened-only reconstruction dropped: a second
        // title, a second contributor with affiliations, a second
        // identifier, and a second description.
        data.titles.push(Title {
            title: "An Alternative Title".to_string(),
            type_: "TranslatedTitle".to_string(),
            ..Default::default()
        });
        data.contributors.push(Contributor {
            given_name: "John".to_string(),
            family_name: "Smith".to_string(),
            affiliations: vec![Affiliation {
                id: "https://ror.org/02catss52".to_string(),
                name: "Example University".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });
        data.identifiers.push(Identifier {
            identifier: "1234-5678".to_string(),
            identifier_type: "ISSN".to_string(),
        });
        data.descriptions.push(Description {
            description: "A second description".to_string(),
            type_: "TechnicalInfo".to_string(),
            ..Default::default()
        });
        data.subjects = vec![Subject { subject: "Biology".to_string() }, Subject {
            subject: "Chemistry".to_string(),
        }];

        let bytes = write_parquet_all(&[data.clone()]).unwrap();
        let roundtripped = read_parquet_all(&bytes).unwrap();

        assert_eq!(roundtripped.len(), 1);
        assert_eq!(roundtripped[0], data);
        assert_eq!(roundtripped[0].titles.len(), 2);
        assert_eq!(roundtripped[0].contributors.len(), 2);
        assert_eq!(roundtripped[0].contributors[1].affiliations[0].name, "Example University");
        assert_eq!(roundtripped[0].identifiers.len(), 2);
        assert_eq!(roundtripped[0].descriptions.len(), 1);
        assert_eq!(roundtripped[0].subjects.len(), 2);
    }

    #[test]
    fn test_read_parquet_all_empty() {
        let bytes = write_parquet_all(&[]).unwrap();
        let roundtripped = read_parquet_all(&bytes).unwrap();
        assert!(roundtripped.is_empty());
    }
}
