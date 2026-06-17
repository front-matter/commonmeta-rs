mod common;

use std::fs;

use common::{collect_json, diff, fixtures_dir};
use serde_json::Value;

fn remove_person_name(value: &mut Value) {
    if let Some(contributors) = value.get_mut("contributors").and_then(Value::as_array_mut) {
        for contributor in contributors {
            if let Some(obj) = contributor.as_object_mut()
                && obj.get("type").and_then(Value::as_str) == Some("Person") {
                    obj.remove("name");
                }
        }
    }
}

fn assert_reader_matrix(format: &str, fixture_subdir: &str) {
    let input_dir = fixtures_dir().join(fixture_subdir);
    let mut ran = 0usize;

    for input_path in collect_json(&input_dir) {
        let name = input_path.file_name().unwrap();
        let expected_path = fixtures_dir().join("commonmeta").join(name);
        if !expected_path.exists() {
            continue;
        }

        ran += 1;

        let input = fs::read_to_string(&input_path)
            .unwrap_or_else(|e| panic!("{}: failed to read input fixture: {e}", input_path.display()));
        let expected: Value = serde_json::from_str(
            &fs::read_to_string(&expected_path)
                .unwrap_or_else(|e| panic!("{}: failed to read expected fixture: {e}", expected_path.display())),
        )
        .unwrap_or_else(|e| panic!("{}: expected fixture is invalid JSON: {e}", expected_path.display()));

        let data = commonmeta::read(format, &input)
            .unwrap_or_else(|e| panic!("{}: read({format}) failed: {e}", input_path.display()));
        let mut actual: Value = serde_json::to_value(data)
            .unwrap_or_else(|e| panic!("{}: failed to serialize Data to JSON: {e}", input_path.display()));
        remove_person_name(&mut actual);

        let diffs = diff(&expected, &actual);
        assert!(
            diffs.is_empty(),
            "{} (format: {format}): {:#?}",
            input_path.display(),
            diffs
        );
    }

    assert!(
        ran > 0,
        "no fixture pairs found for format '{}' in {}",
        format,
        input_dir.display()
    );
}

#[test]
fn crossref_reader_fixture_matrix() {
    assert_reader_matrix("crossref", "crossref");
}

#[test]
fn datacite_reader_fixture_matrix() {
    assert_reader_matrix("datacite", "datacite_reader");
}

#[test]
fn schemaorg_reader_fixture_matrix() {
    assert_reader_matrix("schemaorg", "schemaorg");
}
