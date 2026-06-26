use clap::{Arg, ArgAction, ArgMatches, Command};
use std::time::Instant;

use commonmeta::Data;

use crate::cmd::list::load_list_from_file;

pub fn command() -> Command {
    Command::new("push")
        .about("Push scholarly metadata into a service")
        .long_about(
            "Convert scholarly metadata between formats and register with a service.\n\
            Registration is currently only supported with InvenioRDM.\n\n\
            This performs real, network-visible writes: a live record is created or\n\
            updated and published on --host using --token for authentication.\n\n\
            Examples:\n\n\
            commonmeta push records.json --from commonmeta --to inveniordm --host rogue-scholar.org --token TOKEN\n\
            commonmeta push records.parquet --from commonmeta --to inveniordm --host my.invenio.host --token TOKEN",
        )
        .arg(
            Arg::new("input")
                .help("Input file path (JSON/JSONL array, or Parquet for --from commonmeta)")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input source format (crossref, datacite, openalex, commonmeta)")
                .default_value("commonmeta"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .short('t')
                .help("Target service to register with")
                .default_value("inveniordm"),
        )
        .arg(
            Arg::new("host")
                .long("host")
                .help("InvenioRDM host (e.g. rogue-scholar.org)"),
        )
        .arg(Arg::new("token").long("token").help("InvenioRDM API token"))
        .arg(Arg::new("prefix").long("prefix").help("DOI prefix"))
        .arg(
            Arg::new("depositor")
                .long("depositor")
                .help("Depositor name (used for Crossref XML registration)"),
        )
        .arg(
            Arg::new("email")
                .long("email")
                .help("Depositor email (used for Crossref XML registration)"),
        )
        .arg(
            Arg::new("registrant")
                .long("registrant")
                .help("Registrant name (used for Crossref XML registration)"),
        )
        .arg(
            Arg::new("login-id")
                .long("login-id")
                .help("Login ID for Crossref XML deposit"),
        )
        .arg(
            Arg::new("login-passwd")
                .long("login-passwd")
                .help("Login password for Crossref XML deposit"),
        )
        .arg(
            Arg::new("test-mode")
                .long("test-mode")
                .help("Use test mode for Crossref XML deposit")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("legacy-conn")
                .long("legacy-conn")
                .help("Legacy connection string"),
        )
        .arg(
            Arg::new("show-errors")
                .long("show-errors")
                .help("Print validation errors")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("show-timer")
                .long("show-timer")
                .help("Print runtime after completion")
                .action(ArgAction::SetTrue),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let timer = Instant::now();

    let via = matches
        .get_one::<String>("from")
        .map(String::as_str)
        .unwrap_or("commonmeta");
    let to = matches
        .get_one::<String>("to")
        .map(String::as_str)
        .unwrap_or("inveniordm");
    let show_timer = matches.get_flag("show-timer");

    let data = if let Some(input_path) = matches.get_one::<String>("input") {
        if via == "commonmeta" {
            load_commonmeta_file(input_path)?
        } else {
            load_list_from_file(input_path, via, None, 0)?
        }
    } else {
        return Err("push: an input file path is required".to_string());
    };

    let result = match to {
        "inveniordm" => push_to_inveniordm(&data, matches),
        "crossref_xml" | "datacite" => Err(format!(
            "push: --to {} is not yet implemented (registration is currently only supported with --to inveniordm)",
            to
        )),
        other => Err(format!("push: unsupported --to target: {}", other)),
    };

    if show_timer {
        eprintln!("Runtime: {:.2} seconds", timer.elapsed().as_secs_f64());
    }

    result
}

fn push_to_inveniordm(data: &[Data], matches: &ArgMatches) -> Result<(), String> {
    let host = matches
        .get_one::<String>("host")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "push: --to inveniordm requires --host <host>".to_string())?;
    let token = matches
        .get_one::<String>("token")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "push: --to inveniordm requires --token <token>".to_string())?;

    let results = commonmeta::push_inveniordm(data, host, token);
    let output = serde_json::to_string_pretty(&results).map_err(|e| e.to_string())?;
    println!("{}", output);
    Ok(())
}

/// Load commonmeta records from a local JSON file (single record or array)
/// or a Parquet dump written by `list --file *.parquet`.
fn load_commonmeta_file(path: &str) -> Result<Vec<Data>, String> {
    if path.ends_with(".parquet") || path.ends_with(".parquet.zst") {
        return load_list_from_file(path, "commonmeta", None, 0);
    }

    let content =
        std::fs::read_to_string(path).map_err(|e| format!("failed to read '{}': {}", path, e))?;
    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON in '{}': {}", path, e))?;

    if let Some(items) = value.as_array() {
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let data: Data = serde_json::from_value(item.clone())
                .map_err(|e| format!("failed to parse commonmeta record: {}", e))?;
            out.push(data);
        }
        return Ok(out);
    }

    let data: Data = serde_json::from_value(value)
        .map_err(|e| format!("failed to parse commonmeta record: {}", e))?;
    Ok(vec![data])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_commonmeta_file_single_record() {
        let dir = std::env::temp_dir().join("commonmeta_push_test_single");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let data = load_commonmeta_file(path.to_str().unwrap()).unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.1/a");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_file_array() {
        let dir = std::env::temp_dir().join("commonmeta_push_test_array");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("records.json");
        std::fs::write(
            &path,
            r#"[{"id":"https://doi.org/10.1/a","type":"JournalArticle"},{"id":"https://doi.org/10.1/b","type":"JournalArticle"}]"#,
        )
        .unwrap();

        let data = load_commonmeta_file(path.to_str().unwrap()).unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[1].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_requires_input_file() {
        let matches = command().get_matches_from(vec!["push", "--from", "commonmeta"]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("input file path is required"));
    }

    #[test]
    fn test_execute_rejects_unimplemented_to() {
        let dir = std::env::temp_dir().join("commonmeta_push_test_to");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let matches = command().get_matches_from(vec![
            "push",
            path.to_str().unwrap(),
            "--from",
            "commonmeta",
            "--to",
            "datacite",
        ]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("not yet implemented"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_requires_host_for_inveniordm() {
        let dir = std::env::temp_dir().join("commonmeta_push_test_host");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let matches = command().get_matches_from(vec![
            "push",
            path.to_str().unwrap(),
            "--from",
            "commonmeta",
        ]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("requires --host"));

        std::fs::remove_dir_all(&dir).ok();
    }
}
