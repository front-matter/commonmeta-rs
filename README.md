# commonmeta-rs

commonmeta-rs is a Rust library to implement Commonmeta, the common Metadata Model for Scholarly Metadata. Use commonmeta to convert scholarly metadata in a variety of formats, listed below. Commonmeta-rs is work in progress, the first release was on June 17, 2026. Implementations in other languages are also available ([Go](https://github.com/front-matter/commonmeta), [Python](https://github.com/front-matter/commonmeta-py), [Ruby](https://github.com/front-matter/commonmeta-ruby)).

commonmeta uses semantic versioning. Currently, its major version number is still at 0, meaning the API is not yet stable, and breaking changes are expected in the internal API and commonmeta JSON format.

## Supported Metadata Formats

Commonmeta-rs reads and/or writes these metadata formats:

| Format                                                                                   | Name         | Content Type                            | Read  | Write |
| ---------------------------------------------------------------------------------------- | ------------ | --------------------------------------- | ----- | ----- |
| Commonmeta                                                                               | commonmeta   | application/vnd.commonmeta+json         | yes   | yes   |
| [CrossRef XML](https://www.crossref.org/schema/documentation/unixref1.1/unixref1.1.html) | crossref_xml | application/vnd.crossref.unixref+xml    | yes   | yes   |
| [Crossref](https://api.crossref.org)                                                     | crossref     | application/vnd.crossref+json           | yes   | n/a   |
| [DataCite](https://api.datacite.org/)                                                    | datacite     | application/vnd.datacite.datacite+json  | yes   | yes   |
| [Schema.org (in JSON-LD)](http://schema.org/)                                            | schema_org   | application/vnd.schemaorg.ld+json       | yes   | yes   |
| [RDF XML](http://www.w3.org/TR/rdf-syntax-grammar/)                                      | rdf_xml      | application/rdf+xml                     | no    | later |
| [RDF Turtle](http://www.w3.org/TeamSubmission/turtle/)                                   | turtle       | text/turtle                             | no    | later |
| [CSL-JSON](https://citationstyles.org/)                                                  | csl          | application/vnd.citationstyles.csl+json | yes   | yes   |
| [Formatted text citation](https://citationstyles.org/)                                   | citation     | text/x-bibliography                     | n/a   | yes   |
| [Codemeta](https://codemeta.github.io/)                                                  | codemeta     | application/vnd.codemeta.ld+json        | yes   | later |
| [Citation File Format (CFF)](https://citation-file-format.github.io/)                    | cff          | application/vnd.cff+yaml                | yes   | later |
| [JATS](https://jats.nlm.nih.gov/)                                                        | jats         | application/vnd.jats+xml                | later | later |
| [CSV](https://en.wikipedia.org/wiki/Comma-separated_values)                              | csv          | text/csv                                | no    | later |
| [BibTex](http://en.wikipedia.org/wiki/BibTeX)                                            | bibtex       | application/x-bibtex                    | yes   | yes   |
| [RIS](http://en.wikipedia.org/wiki/RIS_(file_format))                                    | ris          | application/x-research-info-systems     | yes   | yes   |
| [InvenioRDM](https://inveniordm.docs.cern.ch/reference/metadata/)                        | inveniordm   | application/vnd.inveniordm.v1+json      | yes   | yes   |
| [JSON Feed](https://www.jsonfeed.org/)                                                   | jsonfeed     | application/feed+json                   | yes   | later |
| [OpenAlex](https://www.openalex.org/)                                                    | openalex     | n/a                                     | yes   | no    |

_commonmeta_: the Commonmeta format is the native format for the library and used internally.
_Planned_: we plan to implement this format for the v1.0 public release.
_Later_: we plan to implement this format in a later release.

## Build & run

```sh
cargo build
cargo test
```

The `commonmeta` binary has four subcommands: `convert`, `encode`, `decode`, and `list`.

```sh
# Encode/decode a Crockford base32 identifier suffix
cargo run -- encode 10.5555
cargo run -- decode 10.5555/nwbyp-29t86

# Convert a single record between formats, fetching it by DOI
cargo run -- convert 10.5555/12345678 --from crossref --to csl

# Convert a local file and write the result to disk
cargo run -- convert record.json --from commonmeta --to csl --file out.json

# Render a formatted citation (CSL style + locale)
cargo run -- convert 10.5555/12345678 --from crossref --to citation --style apa --locale en-US

# Fetch a batch of records from an API and write them as a JSON array
cargo run -- list --from crossref --number 100 --type journal-article --file out.json

# Read VRAIX metadata from a local SQLite file
cargo run -- list crossref-2026-06-15.sqlite3 --from vraix --number 0 --to commonmeta --file out.json.gz

# Parquet output (.parquet file extension, --to commonmeta only): records are split into batches of 100,000, written in parallel, and zstd-compressed
cargo run --release -- list crossref-2026-06-15.sqlite3 --from vraix --number 0 --file out.parquet
```

Use `cargo run -- <subcommand> --help` for the full list of options for each subcommand.

## Documentation

Documentation (work in progress) for using the library is available at the [commonmeta-rs Documentation](https://rs.commonmeta.org/) website.

## Meta

Please note that this project is released with a [Contributor Code of Conduct](https://github.com/front-matter/commonmeta-rs/blob/main/CODE_OF_CONDUCT.md). By participating in this project you agree to abide by its terms.

License: [MIT](https://github.com/front-matter/commonmeta-rs/blob/main/LICENSE)
