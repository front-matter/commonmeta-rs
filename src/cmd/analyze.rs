/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgMatches, Command};
use duckdb::{Connection, Row, types::ValueRef};

pub fn command() -> Command {
    Command::new("analyze")
        .about("Analyze Parquet batches with DuckDB")
        .long_about(
            "Analyze one or more Parquet files with DuckDB.\n\n\
            The input can be a single file path or a glob that DuckDB's \
            read_parquet() understands. The default query groups by \
            record type, but you can pick a different field with --group-by \
            or run a custom query with --sql.\n\n\
            Example usage:\n\n\
            commonmeta analyze 'batch-commonmeta-*.parquet.zst'\n\
            commonmeta analyze --group-by publisher 'batch-commonmeta-*.parquet.zst'\n\
            commonmeta analyze --sql 'SELECT record_type, COUNT(*) AS n FROM records GROUP BY record_type ORDER BY n DESC' 'batch-commonmeta-*.parquet.zst'",
        )
        .arg(
            Arg::new("sql")
                .long("sql")
                .value_name("QUERY")
                .help("Custom DuckDB query to run against the records view")
                .conflicts_with("group_by"),
        )
        .arg(
            Arg::new("group_by")
                .long("group-by")
                .value_name("COLUMN")
                .help("Column to group by for the default count query"),
        )
        .arg(
            Arg::new("parquet_glob")
                .help("Parquet file path or glob")
                .required(true)
                .index(1),
        )
}

fn escaped_parquet_glob(parquet_glob: &str) -> String {
    parquet_glob.replace('\'', "''")
}

fn validate_group_by(group_by: &str) -> Result<(), String> {
    if group_by.is_empty()
        || !group_by
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.'))
    {
        return Err(format!(
            "invalid --group-by value '{}': only ASCII letters, digits, '_' and '.' are allowed",
            group_by
        ));
    }

    Ok(())
}

fn default_query(parquet_glob: &str, group_by: &str) -> Result<String, String> {
    validate_group_by(group_by)?;
    let escaped = escaped_parquet_glob(parquet_glob);
    Ok(format!(
        "SELECT {group_by}, COUNT(*) AS count FROM read_parquet('{escaped}') GROUP BY {group_by} ORDER BY {group_by}"
    ))
}

fn value_ref_to_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_string(),
        ValueRef::Boolean(v) => v.to_string(),
        ValueRef::TinyInt(v) => v.to_string(),
        ValueRef::SmallInt(v) => v.to_string(),
        ValueRef::Int(v) => v.to_string(),
        ValueRef::BigInt(v) => v.to_string(),
        ValueRef::UTinyInt(v) => v.to_string(),
        ValueRef::USmallInt(v) => v.to_string(),
        ValueRef::UInt(v) => v.to_string(),
        ValueRef::UBigInt(v) => v.to_string(),
        ValueRef::HugeInt(v) => v.to_string(),
        ValueRef::Float(v) => v.to_string(),
        ValueRef::Double(v) => v.to_string(),
        ValueRef::Decimal(v) => v.to_string(),
        ValueRef::Text(v) => String::from_utf8_lossy(v).into_owned(),
        ValueRef::Blob(v) => format!(
            "0x{}",
            v.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ),
        ValueRef::Date32(v) => v.to_string(),
        ValueRef::Time64(unit, v) => format!("{:?} {v}", unit),
        ValueRef::Timestamp(unit, v) => format!("{:?} {v}", unit),
        ValueRef::Interval {
            months,
            days,
            nanos,
        } => {
            format!("months={months}, days={days}, nanos={nanos}")
        }
        ValueRef::List(_, _)
        | ValueRef::Enum(_, _)
        | ValueRef::Struct(_, _)
        | ValueRef::Map(_, _)
        | ValueRef::Array(_, _)
        | ValueRef::Union(_, _) => format!("{value:?}"),
    }
}

fn print_row(row: &Row<'_>, column_count: usize) -> duckdb::Result<()> {
    let mut values = Vec::with_capacity(column_count);
    for idx in 0..column_count {
        values.push(value_ref_to_string(row.get_ref(idx)?));
    }
    println!("{}", values.join("\t"));
    Ok(())
}

fn run_query(parquet_glob: &str, sql: &str) -> duckdb::Result<()> {
    let conn = Connection::open_in_memory()?;
    let view_sql = format!(
        "CREATE VIEW records AS SELECT * FROM read_parquet('{}')",
        escaped_parquet_glob(parquet_glob)
    );
    conn.execute_batch(&view_sql)?;

    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query([])?;
    let column_count = rows.as_ref().map(|s| s.column_count()).unwrap_or(0);
    while let Some(row) = rows.next()? {
        print_row(row, column_count)?;
    }
    Ok(())
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let parquet_glob = matches.get_one::<String>("parquet_glob").expect("required");
    let sql = if let Some(sql) = matches.get_one::<String>("sql") {
        sql.clone()
    } else {
        let group_by = matches
            .get_one::<String>("group_by")
            .map(String::as_str)
            .unwrap_or("record_type");
        default_query(parquet_glob, group_by)?
    };

    run_query(parquet_glob, &sql).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commonmeta::data::{Data, Publisher};

    fn write_test_parquet() -> String {
        let mut article = Data {
            id: "https://doi.org/10.1/a".to_string(),
            type_: "JournalArticle".to_string(),
            publisher: Publisher {
                name: "Front Matter Press".to_string(),
                ..Default::default()
            },
            ..Data::default()
        };
        article.title = "A".to_string();

        let mut dataset = Data {
            id: "https://doi.org/10.1/b".to_string(),
            type_: "Dataset".to_string(),
            publisher: Publisher {
                name: "Front Matter Press".to_string(),
                ..Default::default()
            },
            ..Data::default()
        };
        dataset.title = "B".to_string();

        let bytes = commonmeta::write_parquet(&[article, dataset]).unwrap();
        let path = std::env::temp_dir().join(format!(
            "commonmeta-analyze-{}-{}.parquet",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, bytes).unwrap();

        path.to_string_lossy().into_owned()
    }

    #[test]
    fn default_query_counts_record_types() {
        let path = write_test_parquet();

        let sql = default_query(&path, "record_type").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        let mut stmt = conn.prepare(&sql).unwrap();
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap();

        let mut counts = Vec::new();
        for row in rows {
            counts.push(row.unwrap());
        }

        std::fs::remove_file(&path).ok();

        assert_eq!(
            counts,
            vec![
                ("Dataset".to_string(), 1),
                ("JournalArticle".to_string(), 1)
            ]
        );
    }

    #[test]
    fn default_query_uses_custom_group_by() {
        let sql = default_query("batch.parquet", "publisher").unwrap();

        assert!(sql.contains("SELECT publisher, COUNT(*) AS count"));
        assert!(sql.contains("GROUP BY publisher"));
    }

    #[test]
    fn default_query_rejects_invalid_group_by() {
        let err = default_query("batch.parquet", "record_type; DROP TABLE x").unwrap_err();

        assert!(err.contains("invalid --group-by value"));
    }

    #[test]
    fn run_query_supports_custom_sql_against_records_view() {
        let path = write_test_parquet();

        let result = run_query(
            &path,
            "SELECT publisher, COUNT(*) AS count FROM records GROUP BY publisher ORDER BY publisher",
        );

        std::fs::remove_file(&path).ok();

        assert!(result.is_ok());
    }
}
