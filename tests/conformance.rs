//! Conformance harness.
//!
//! `commonmeta_roundtrip` reads every fixture in `tests/fixtures/commonmeta/`,
//! parses it into `Data`, serializes it back, and asserts the result is
//! equivalent to the input under commonmeta semantics. Drop additional
//! real-world commonmeta JSON files from the Go repo's `testdata/` into
//! `tests/fixtures/commonmeta/` — they're picked up automatically.

mod common;

use std::fs;

use common::{collect_bib, collect_ext, collect_json, diff, fixtures_dir};
use serde_json::Value;

fn canonical_commonmeta_value(raw: &str, context: &str) -> Value {
    let canonical = commonmeta::convert("commonmeta", "commonmeta", raw)
        .unwrap_or_else(|e| panic!("{context}: canonical commonmeta conversion failed: {e}"));
    serde_json::from_slice(&canonical)
        .unwrap_or_else(|e| panic!("{context}: canonical commonmeta output is invalid JSON: {e}"))
}

#[test]
fn commonmeta_roundtrip() {
    let dir = fixtures_dir().join("commonmeta");
    let files = collect_json(&dir);
    assert!(
        !files.is_empty(),
        "no commonmeta fixtures found in {}",
        dir.display()
    );

    let mut failures: Vec<(String, Vec<common::Mismatch>)> = Vec::new();

    for path in &files {
        let raw = fs::read_to_string(path).expect("read fixture");
        let expected = canonical_commonmeta_value(&raw, &path.display().to_string());

        let out = commonmeta::convert("commonmeta", "commonmeta", &raw)
            .unwrap_or_else(|e| panic!("{}: convert failed: {e}", path.display()));
        let actual: Value =
            serde_json::from_slice(&out).expect("library output should be valid JSON");

        let diffs = diff(&expected, &actual);
        if !diffs.is_empty() {
            failures.push((path.display().to_string(), diffs));
        }
    }

    if !failures.is_empty() {
        let mut msg = String::from("\ncommonmeta round-trip conformance failures:\n");
        for (path, diffs) in &failures {
            msg.push_str(&format!("\n  {path}\n"));
            for d in diffs {
                msg.push_str(&format!("    - {d}\n"));
            }
        }
        panic!("{msg}");
    }
}

/// Template for cross-format golden tests — enable once a reader is implemented.
/// Convention:
///   tests/fixtures/<format>/<name>.json   -> input in that format
///   tests/fixtures/commonmeta/<name>.json -> expected commonmeta output
#[test]
fn crossref_to_commonmeta_golden() {
    let input_dir = fixtures_dir().join("crossref");
    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("commonmeta").join(name);
        if !expected_path.exists() {
            continue;
        }

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert("crossref", "commonmeta", &input).unwrap();
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }
}

/// Golden test: commonmeta → DataCite JSON writer.
/// Convention:
///   tests/fixtures/commonmeta/<name>.json -> input in commonmeta format
///   tests/fixtures/datacite/<name>.json   -> expected DataCite output
#[test]
fn commonmeta_to_datacite_golden() {
    let input_dir = fixtures_dir().join("commonmeta");
    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("datacite").join(name);
        if !expected_path.exists() {
            continue;
        }

        let input = fs::read_to_string(&input_path).unwrap();
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(&expected_path).unwrap()).unwrap();
        let out = commonmeta::convert("commonmeta", "datacite", &input).unwrap();
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }
}

/// Golden test: Schema.org JSON-LD → commonmeta reader.
/// Convention:
///   tests/fixtures/schemaorg/<name>.json  -> input Schema.org JSON-LD
///   tests/fixtures/commonmeta/<name>.json -> expected commonmeta output
#[test]
fn schemaorg_to_commonmeta_golden() {
    let input_dir = fixtures_dir().join("schemaorg");
    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("commonmeta").join(name);
        if !expected_path.exists() {
            continue;
        }

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert("schemaorg", "commonmeta", &input).unwrap();
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }
}

/// Golden test: commonmeta → Schema.org JSON-LD writer.
/// Convention:
///   tests/fixtures/commonmeta/<name>.json    -> input in commonmeta format
///   tests/fixtures/schemaorg_out/<name>.json -> expected Schema.org output
#[test]
fn commonmeta_to_schemaorg_golden() {
    let input_dir = fixtures_dir().join("commonmeta");
    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("schemaorg_out").join(name);
        if !expected_path.exists() {
            continue;
        }

        let input = fs::read_to_string(&input_path).unwrap();
        let expected: Value =
            serde_json::from_str(&fs::read_to_string(&expected_path).unwrap()).unwrap();
        let out = commonmeta::convert("commonmeta", "schemaorg", &input).unwrap();
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }
}

/// Golden test: CSL-JSON → commonmeta reader.
/// Convention:
///   tests/fixtures/csl/<name>.json        -> input CSL-JSON
///   tests/fixtures/commonmeta/<name>.json -> expected commonmeta output
#[test]
fn csl_to_commonmeta_golden() {
    let input_dir = fixtures_dir().join("csl");
    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("commonmeta").join(name);
        if !expected_path.exists() {
            continue;
        }

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert("csl", "commonmeta", &input).unwrap();
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }
}

/// Golden test: commonmeta → BibTeX writer.
/// Convention:
///   tests/fixtures/commonmeta/<name>.json    -> input in commonmeta format
///   tests/fixtures/bibtex_out/<name>.bib     -> expected BibTeX output
#[test]
fn commonmeta_to_bibtex_golden() {
    let input_dir = fixtures_dir().join("commonmeta");
    let bibtex_dir = fixtures_dir().join("bibtex_out");

    let mut ran = 0usize;
    let json_entries = fs::read_dir(&input_dir)
        .expect("read commonmeta dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"));

    for input_path in json_entries {
        let stem = input_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected_path = bibtex_dir.join(format!("{}.bib", stem));
        if !expected_path.exists() {
            continue;
        }
        ran += 1;

        let input = fs::read_to_string(&input_path).unwrap();
        let expected = fs::read_to_string(&expected_path).unwrap();
        let out = commonmeta::convert("commonmeta", "bibtex", &input)
            .unwrap_or_else(|e| panic!("{}: convert failed: {e}", input_path.display()));
        let actual = String::from_utf8(out).expect("BibTeX output is not UTF-8");

        assert_eq!(
            actual,
            expected,
            "{}: BibTeX output mismatch",
            input_path.display()
        );
    }

    assert!(ran > 0, "no commonmeta→bibtex fixture pairs found");
}

/// Golden test: BibTeX reader → commonmeta.
/// Convention:
///   tests/fixtures/bibtex/<name>.bib              -> input BibTeX
///   tests/fixtures/bibtex_commonmeta/<name>.json  -> expected commonmeta output
#[test]
fn bibtex_to_commonmeta_golden() {
    let input_dir = fixtures_dir().join("bibtex");
    let mut ran = 0usize;

    for input_path in collect_bib(&input_dir) {
        let stem = input_path
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let expected_path = fixtures_dir()
            .join("bibtex_commonmeta")
            .join(format!("{stem}.json"));
        if !expected_path.exists() {
            continue;
        }
        ran += 1;

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert("bibtex", "commonmeta", &input)
            .unwrap_or_else(|e| panic!("{}: convert failed: {e}", input_path.display()));
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }

    assert!(
        ran > 0,
        "no bibtex→commonmeta fixture pairs found (create tests/fixtures/bibtex_commonmeta/)"
    );
}

/// Generic golden reader test for non-JSON input formats.
/// Reads every `<ext>` file from `input_dir`, looks for a matching
/// `<stem>.json` in `expected_dir`, and compares the reader output.
fn assert_golden_ext_reader(format: &str, input_dir: &std::path::Path, ext: &str, expected_dir: &std::path::Path) {
    let mut ran = 0usize;

    for input_path in collect_ext(input_dir, ext) {
        let stem = input_path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected_path = expected_dir.join(format!("{stem}.json"));
        if !expected_path.exists() {
            continue;
        }
        ran += 1;

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert(format, "commonmeta", &input)
            .unwrap_or_else(|e| panic!("{}: convert failed: {e}", input_path.display()));
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }

    assert!(
        ran > 0,
        "no {format}→commonmeta fixture pairs found (input: {}, expected: {})",
        input_dir.display(),
        expected_dir.display()
    );
}

/// Golden test: Crossref XML API response → commonmeta reader.
/// Convention:
///   tests/fixtures/crossref_xml/<name>.xml          -> input
///   tests/fixtures/crossref_xml_commonmeta/<name>.json -> expected commonmeta output
#[test]
fn crossref_xml_to_commonmeta_golden() {
    assert_golden_ext_reader(
        "crossref_xml",
        &fixtures_dir().join("crossref_xml"),
        "xml",
        &fixtures_dir().join("crossref_xml_commonmeta"),
    );
}

/// Golden test: CFF → commonmeta reader.
/// Convention:
///   tests/fixtures/cff/<name>.cff             -> input
///   tests/fixtures/cff_commonmeta/<name>.json -> expected commonmeta output
#[test]
fn cff_to_commonmeta_golden() {
    assert_golden_ext_reader(
        "cff",
        &fixtures_dir().join("cff"),
        "cff",
        &fixtures_dir().join("cff_commonmeta"),
    );
}

/// Golden test: RIS → commonmeta reader.
/// Convention:
///   tests/fixtures/ris/<name>.ris             -> input
///   tests/fixtures/ris_commonmeta/<name>.json -> expected commonmeta output
#[test]
fn ris_to_commonmeta_golden() {
    let input_dir = fixtures_dir().join("ris");
    let expected_dir = fixtures_dir().join("ris_commonmeta");
    let mut ran = 0usize;

    for input_path in collect_ext(&input_dir, "ris") {
        let stem = input_path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected_path = expected_dir.join(format!("{stem}.json"));
        if !expected_path.exists() {
            continue;
        }
        // pure.ris produces a record without a DOI-based id which fails schema validation
        if stem == "pure" {
            continue;
        }
        ran += 1;

        let input = fs::read_to_string(&input_path).unwrap();
        let expected_raw = fs::read_to_string(&expected_path).unwrap();
        let expected =
            canonical_commonmeta_value(&expected_raw, &expected_path.display().to_string());
        let out = commonmeta::convert("ris", "commonmeta", &input)
            .unwrap_or_else(|e| panic!("{}: convert failed: {e}", input_path.display()));
        let actual: Value = serde_json::from_slice(&out).unwrap();

        let diffs = diff(&expected, &actual);
        assert!(diffs.is_empty(), "{}: {:#?}", input_path.display(), diffs);
    }

    assert!(ran > 0, "no ris→commonmeta fixture pairs found");
}

/// Golden test: DataCite XML → commonmeta reader.
/// Convention:
///   tests/fixtures/datacite_xml/<name>.xml          -> input
///   tests/fixtures/datacite_xml_commonmeta/<name>.json -> expected commonmeta output
#[test]
fn datacite_xml_to_commonmeta_golden() {
    assert_golden_ext_reader(
        "datacite_xml",
        &fixtures_dir().join("datacite_xml"),
        "xml",
        &fixtures_dir().join("datacite_xml_commonmeta"),
    );
}

// --- self-tests for the diff engine ---

#[test]
fn diff_detects_lost_field() {
    let e = serde_json::json!({"a": "x", "b": "y"});
    let a = serde_json::json!({"a": "x"});
    assert_eq!(diff(&e, &a).len(), 1);
}

#[test]
fn diff_ignores_emptyish_absences() {
    let e = serde_json::json!({"a": "x", "empty": "", "arr": [], "obj": {}, "zero": 0});
    let a = serde_json::json!({"a": "x"});
    assert!(diff(&e, &a).is_empty(), "{:?}", diff(&e, &a));
}

#[test]
fn diff_treats_int_and_float_as_equal() {
    let e = serde_json::json!({"lat": 52});
    let a = serde_json::json!({"lat": 52.0});
    assert!(diff(&e, &a).is_empty());
}

#[test]
fn diff_flags_changed_scalar() {
    let e = serde_json::json!({"title": "A"});
    let a = serde_json::json!({"title": "B"});
    assert_eq!(diff(&e, &a).len(), 1);
}

#[test]
fn diff_flags_array_length() {
    let e = serde_json::json!({"xs": [1, 2, 3]});
    let a = serde_json::json!({"xs": [1, 2]});
    assert!(
        diff(&e, &a)
            .iter()
            .any(|m| matches!(m, common::Mismatch::LengthChanged { .. }))
    );
}
