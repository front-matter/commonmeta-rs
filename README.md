# commonmeta-rs

commonmeta-rs is a Rust library to implement Commonmeta, the common Metadata Model for Scholarly Metadata. Use commonmeta to convert scholarly metadata in a variety of formats, listed below. Commonmeta-rs is work in progress, the first release was on June 17, 2026. Implementations in other languages are also available ([Go](https://github.com/front-matter/commonmeta), [Python](https://github.com/front-matter/commonmeta-py), [Ruby](https://github.com/front-matter/commonmeta-ruby)).

## Supported Metadata Formats

Commonmeta-rs reads and/or writes these metadata formats:

| Format                                                                                   | Name         | Content Type                            | Read  | Write |
| ---------------------------------------------------------------------------------------- | ------------ | --------------------------------------- | ----- | ----- |
| Commonmeta                                                                               | commonmeta   | application/vnd.commonmeta+json         | yes   | yes   |
| [CrossRef XML](https://www.crossref.org/schema/documentation/unixref1.1/unixref1.1.html) | crossref_xml | application/vnd.crossref.unixref+xml    | yes   | yes   |
| [Crossref](https://api.crossref.org)                                                     | crossref     | application/vnd.crossref+json           | yes   | yes   |
| [DataCite](https://api.datacite.org/)                                                    | datacite     | application/vnd.datacite.datacite+json  | yes   | yes   |
| [DataCite XML](https://api.datacite.org/)                                                            | datacite_xml | application/vnd.datacite.datacite+xml | yes     | yes |
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
_later_: we plan to implement this format in a later release.

## Build & run

```sh
cargo build
cargo test
```

The `commonmeta` binary has eight subcommands: `convert`, `encode`, `decode`, `import`, `list`, `push`, `put`, and `match`.

```sh
# Encode/decode a Crockford base32 identifier suffix given a DOI prefix
cargo run -- encode 10.5555
cargo run -- decode 10.5555/nwbyp-29t86

# Convert a single record between formats, fetching it by DOI
cargo run -- convert 10.5555/12345678 --from crossref --to csl

# Convert a local file and write the result to disk
cargo run -- convert record.json --from commonmeta --to csl --file out.json

# Render a formatted citation (CSL style + locale)
cargo run -- convert 10.5555/12345678 --from crossref --to citation --style apa --locale en-US

# Fetch a batch of records from an API and write them as a commonmeta JSON array
cargo run -- list --from crossref --number 100 --type journal-article --file out.json

# Read all records from a local VRAIX SQLite file and convert to another format
cargo run -- list crossref-2026-06-15.sqlite3 --number 0 --to commonmeta --file out.json.gz

# Parquet output (.parquet file extension, --to commonmeta only): records are split into batches of 100,000, written in parallel, and zstd-compressed
cargo run --release -- list crossref-2026-06-15.sqlite3 --number 0 --file out.parquet

# Import a single record by DOI into the local commonmeta database (source auto-detected)
cargo run -- import 10.7554/elife.01567

# Import all Crossref records for a ROR-identified institution (paginates through all results)
cargo run -- import --from crossref --ror 00pd74e08

# Import all DataCite records for an ORCID author (paginates through all results)
cargo run -- import --from datacite --orcid 0000-0003-1419-2405

# Import all records from a Crossref or DataCite VRAIX daily dump
cargo run -- import --from crossref --date 2026-06-15
cargo run -- import crossref-2026-06-15.sqlite3

# Import all records from the VRAIX pidbox dump
cargo run -- import --from pidbox

# Register records with a live InvenioRDM instance (creates/updates and publishes
# real records — registration is currently only supported with --to inveniordm)
cargo run -- push --from crossref --number 10 --to inveniordm --host rogue-scholar.org --token TOKEN

# Same as push, but for a single record (DOI, URL, or file path)
cargo run -- put 10.5555/12345678 --from crossref --to inveniordm --host rogue-scholar.org --token TOKEN

# Match a free-text affiliation string to a ROR organization (uses local DB when available)
cargo run -- match "Leibniz Universität Hannover"
cargo run -- match "Leibniz Universität Hannover" --to inveniordm

# Look up a ROR organization (uses local DB when available)
cargo run -- convert https://ror.org/02nr0ka47
cargo run -- convert https://ror.org/02nr0ka47 --to inveniordm

# Work fully offline — fails fast if a network call would be required
cargo run -- convert record.json --from commonmeta --to csl --no-network
cargo run -- list crossref-2026-06-15.sqlite3 --no-network --file out.json
cargo run -- import crossref-2026-06-15.sqlite3 --no-network
cargo run -- match "Leibniz Universität Hannover" --no-network
```

Use `cargo run -- <subcommand> --help` for the full list of options for each subcommand.

### `--no-network` flag

`convert`, `list`, `import`, and `match` all accept a `--no-network` flag. When set, any
operation that would make an outbound HTTP request is rejected immediately with a clear error
message. Operations on local files always succeed regardless of this flag. `push` and `put`
always require network access and do not expose this flag.

## Local database

The `import` command populates a local commonmeta SQLite database with scholarly metadata records. All imports upsert — existing records are updated rather than replaced. The database is also used by `match` and `convert` for offline lookups.

```sh
# Import a single record by DOI (source auto-detected from the DOI prefix)
commonmeta import 10.7554/elife.01567
commonmeta import https://doi.org/10.7554/elife.01567

# Import all Crossref records for an institution (ROR ID, paginates automatically)
commonmeta import --from crossref --ror 00pd74e08

# Import all DataCite records for an author (ORCID, paginates automatically)
commonmeta import --from datacite --orcid 0000-0003-1419-2405

# Import a full daily dump (downloads from metadata.vraix.org)
commonmeta import --from crossref --date 2026-06-15
commonmeta import --from datacite --date 2026-06-15

# Import from a locally downloaded VRAIX dump (source auto-detected from filename)
commonmeta import crossref-2026-06-15.sqlite3

# Import the full VRAIX pidbox dump
commonmeta import --from pidbox

# Import latest ROR organization data
commonmeta import --from ror
```

The database path is resolved in this order:

1. `COMMONMETA_DB` environment variable
2. Platform default:

| Platform | Default path                                                   |
| -------- | -------------------------------------------------------------- |
| macOS    | `~/Library/Application Support/commonmeta/commonmeta.sqlite3`  |
| Linux    | `/var/lib/commonmeta/commonmeta.sqlite3`                       |

```sh
# Use a custom path via environment variable
COMMONMETA_DB=/data/commonmeta.sqlite3 commonmeta import --from crossref --date 2026-06-15
```

## Documentation

Documentation (work in progress) for using the library is available at the [commonmeta-rs Documentation](https://rust.commonmeta.org/) website.

## Meta

Please note that this project is released with a [Contributor Code of Conduct](https://github.com/front-matter/commonmeta-rs/blob/main/CODE_OF_CONDUCT.md). By participating in this project you agree to abide by its terms.

License: [MIT](https://github.com/front-matter/commonmeta-rs/blob/main/LICENSE)
