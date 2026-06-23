mod common;

use std::fs;

use common::fixtures_dir;

#[test]
fn commonmeta_schema_validation_fixture_matrix() {
    let dir = fixtures_dir().join("commonmeta");
    let cases = [("journal_article.json", true), ("blog_post_1.json", true)];

    for (name, should_validate) in cases {
        let path = dir.join(name);
        let doc =
            fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let result = commonmeta::schema_utils::json_schema_errors(&doc, None);

        if should_validate {
            assert!(
                result.is_ok(),
                "schema validation unexpectedly failed for {}: {result:?}",
                path.display()
            );
        } else {
            assert!(
                result.is_err(),
                "schema validation unexpectedly passed for {}",
                path.display()
            );
        }
    }
}
