use clap::{Arg, ArgAction, ArgMatches, Command};

use commonmeta::Data;

use crate::cmd::list::{fetch_list_from_api, load_list_from_file};

pub fn command() -> Command {
    Command::new("push")
        .about("Push scholarly metadata into a service")
        .long_about(
            "Convert scholarly metadata between formats and register with a service.\n\
            Registration is currently only supported with InvenioRDM.\n\n\
            This performs real, network-visible writes: a live record is created or\n\
            updated and published on --host using --token for authentication.\n\n\
            Examples:\n\n\
            commonmeta push --from crossref --number 10 --to inveniordm --host rogue-scholar.org --token TOKEN\n\
            commonmeta push records.json --from commonmeta --to inveniordm --host my.invenio.host --token TOKEN",
        )
        .arg(
            Arg::new("input")
                .help("Optional input file path (JSON/JSONL, or Parquet for --from commonmeta)")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input source format (crossref, datacite, openalex, commonmeta)")
                .default_value("crossref"),
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
        .arg(
            Arg::new("number")
                .long("number")
                .help("Number of records to fetch")
                .value_parser(clap::value_parser!(usize))
                .default_value("10"),
        )
        .arg(
            Arg::new("page")
                .long("page")
                .help("Page number (1-based)")
                .value_parser(clap::value_parser!(usize))
                .default_value("1"),
        )
        .arg(Arg::new("member").long("member").help("Crossref member ID"))
        .arg(Arg::new("client").long("client").help("DataCite client ID"))
        .arg(Arg::new("type").long("type").help("Work type filter"))
        .arg(Arg::new("year").long("year").help("Publication year"))
        .arg(Arg::new("language").long("language").help("Language filter"))
        .arg(Arg::new("orcid").long("orcid").help("Filter by ORCID"))
        .arg(Arg::new("ror").long("ror").help("Filter by ROR"))
        .arg(Arg::new("email").long("email").help("Email for OpenAlex mailto parameter"))
        .arg(
            Arg::new("sample")
                .long("sample")
                .help("Use Crossref sample mode")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-orcid")
                .long("has-orcid")
                .help("Filter for records with ORCID")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-ror-id")
                .long("has-ror-id")
                .help("Filter for records with ROR")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-references")
                .long("has-references")
                .help("Filter for records with references")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-relation")
                .long("has-relation")
                .help("Filter for records with relation")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-abstract")
                .long("has-abstract")
                .help("Filter for records with abstract")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-award")
                .long("has-award")
                .help("Filter for records with award")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-license")
                .long("has-license")
                .help("Filter for records with license")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("has-archive")
                .long("has-archive")
                .help("Filter for records with archive")
                .action(ArgAction::SetTrue),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let from = matches
        .get_one::<String>("from")
        .map(String::as_str)
        .unwrap_or("crossref");
    let to = matches
        .get_one::<String>("to")
        .map(String::as_str)
        .unwrap_or("inveniordm");

    if !matches!(from, "crossref" | "datacite" | "openalex" | "commonmeta") {
        return Err(format!(
            "push: --from {} is not implemented yet (supported: crossref, datacite, openalex, commonmeta)",
            from
        ));
    }

    let data = if let Some(input_path) = matches.get_one::<String>("input") {
        if from == "commonmeta" {
            load_commonmeta_file(input_path)?
        } else {
            load_list_from_file(input_path, from)?
        }
    } else {
        if from == "commonmeta" {
            return Err(format!("push: --from {} requires an input file path", from));
        }
        fetch_list_from_api(matches, from)?
    };

    match to {
        "inveniordm" => push_to_inveniordm(&data, matches),
        "crossref_xml" | "datacite" => Err(format!(
            "push: --to {} is not yet implemented (registration is currently only supported with --to inveniordm)",
            to
        )),
        other => Err(format!("push: unsupported --to target: {}", other)),
    }
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

/// Load commonmeta records directly from a local JSON file: either a single
/// record or a JSON array of records (as written by `list --to commonmeta`).
fn load_commonmeta_file(path: &str) -> Result<Vec<Data>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path, e))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("invalid JSON in '{}': {}", path, e))?;

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
    fn test_execute_requires_input_file_for_commonmeta() {
        let matches = command().get_matches_from(vec!["push", "--from", "commonmeta"]);
        // No input file and from=commonmeta should fail before reaching the
        // host/token check, since commonmeta requires a file path.
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("requires an input file path"));
    }

    #[test]
    fn test_execute_rejects_unimplemented_to() {
        let dir = std::env::temp_dir().join("commonmeta_push_test_to");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(&path, r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#)
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
        std::fs::write(&path, r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#)
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
