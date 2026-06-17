# commonmeta-rs

A Rust implementation of [front-matter/commonmeta](https://github.com/front-matter/commonmeta). Converts scholarly metadata between formats.

## Supported Metadata Formats

Commonmeta-rs reads and/or writes these metadata formats:

| Format                                                                                           | Name          | Content Type                           | Read    | Write   |
| ------------------------------------------------------------------------------------------------ | ------------- | -------------------------------------- | ------- | ------- |
| Commonmeta  | commonmeta    | application/vnd.commonmeta+json        | yes     | yes     |
| [CrossRef XML](https://www.crossref.org/schema/documentation/unixref1.1/unixref1.1.html) | crossref_xml      | application/vnd.crossref.unixref+xml   | yes | yes |
| [Crossref](https://api.crossref.org)                                                             | crossref | application/vnd.crossref+json          | yes     | n/a     |
| [DataCite](https://api.datacite.org/)                                                            | datacite | application/vnd.datacite.datacite+json | yes     | yes  |
| [Schema.org (in JSON-LD)](http://schema.org/)                                                    | schema_org    | application/vnd.schemaorg.ld+json      | yes     | yes     |
| [RDF XML](http://www.w3.org/TR/rdf-syntax-grammar/)                                              | rdf_xml       | application/rdf+xml                    | no      | later   |
| [RDF Turtle](http://www.w3.org/TeamSubmission/turtle/)                                           | turtle        | text/turtle                            | no      | later   |
| [CSL-JSON](https://citationstyles.org/)                                                     | csl      | application/vnd.citationstyles.csl+json | yes | yes     |
| [Formatted text citation](https://citationstyles.org/)                                           | citation      | text/x-bibliography                    | n/a     | yes     |
| [Codemeta](https://codemeta.github.io/)                                                          | codemeta      | application/vnd.codemeta.ld+json       | yes | later |
| [Citation File Format (CFF)](https://citation-file-format.github.io/)                            | cff           | application/vnd.cff+yaml               | yes | later |
| [JATS](https://jats.nlm.nih.gov/)                                                                | jats          | application/vnd.jats+xml               | later   | later   |
| [CSV](ttps://en.wikipedia.org/wiki/Comma-separated_values)                                       | csv           | text/csv                               | no      | later   |
| [BibTex](http://en.wikipedia.org/wiki/BibTeX)                                                    | bibtex        | application/x-bibtex                   | yes | yes     |
| [RIS](http://en.wikipedia.org/wiki/RIS_(file_format))                                            | ris           | application/x-research-info-systems    | yes   | yes     |
| [InvenioRDM](https://inveniordm.docs.cern.ch/reference/metadata/)                                | inveniordm    | application/vnd.inveniordm.v1+json     | later   | later     |
| [JSON Feed](https://www.jsonfeed.org/)                                                           | jsonfeed     | application/feed+json    | yes | later     |
| [OpenAlex](https://www.openalex.org/)                                                           | openalex     |    | yes | no     |

_commonmeta_: the Commonmeta format is the native format for the library and used internally.
_Planned_: we plan to implement this format for the v1.0 public release.
_Later_: we plan to implement this format in a later release.

## Build & run

```sh
cargo build
cargo run -- encode 10.5555
cargo run -- decode 10.54900/d3ck1-skq19
cargo run -- convert 10.5555/12345678 --from crossref --to csl
cargo run -- convert record.json --from commonmeta --to csl --file out.json
cargo test
```

## Testing / conformance

`cargo test` runs unit tests plus the conformance harness in
`crates/commonmeta/tests/`:

- **`commonmeta_roundtrip`** reads every fixture in
  `tests/fixtures/commonmeta/*.json`, parses it into `Data`, serializes it back,
  and compares the trees with a semantic JSON diff (`tests/common/mod.rs`). The
  diff is omitempty-aware (empty ≈ absent) and numeric-aware (`52` == `52.0`), so
  the only thing it flags is real data loss, spurious output, or changed values —
  i.e. serde-tag / field-mapping drift. Failures name the exact JSON path.
- The shipped fixtures deliberately exercise the irregular tags
  (`contentHTML`, the snake_case `first_page`/`last_page` on references vs the
  camelCase `firstPage` on containers, `awardUri`, the geo fields, integer
  `size`). **To broaden coverage, drop real commonmeta-format files from the Go
  repo's `testdata/` into `tests/fixtures/commonmeta/`** — they're discovered
  automatically.
- **`crossref_to_commonmeta_golden`** reads each file under `tests/fixtures/crossref/`,
  converts it via the Crossref reader, and compares against the same-named file in
  `tests/fixtures/commonmeta/`. Add pairs there to broaden crossref coverage.

## Documentation

Documentation (work in progress) for using the library is available at the [commonmeta-rs Documentation](https://rs.commonmeta.org/) website.

## Meta

Please note that this project is released with a [Contributor Code of Conduct](https://github.com/front-matter/commonmeta-rs/blob/main/CODE_OF_CONDUCT.md). By participating in this project you agree to abide by its terms.

License: [MIT](https://github.com/front-matter/commonmeta-rs/blob/main/LICENSE)
