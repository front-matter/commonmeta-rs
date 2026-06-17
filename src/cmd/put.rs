use clap::{Arg, ArgMatches, Command};

use commonmeta::Data;

use crate::cmd::convert::detect_format;

pub fn command() -> Command {
    Command::new("put")
        .about("Put scholarly metadata into a service")
        .long_about(
            "Convert scholarly metadata between formats and register a single record\n\
            with a service. Registration is currently only supported with InvenioRDM.\n\n\
            The input is a file path, DOI, or URL. When --from is omitted the format\n\
            is auto-detected: DOIs are resolved via the DOI RA API; JSON files are\n\
            inspected for schema markers.\n\n\
            This performs a real, network-visible write: a live record is created or\n\
            updated and published on --host using --token for authentication.\n\n\
            Examples:\n\n\
            commonmeta put 10.5555/12345678 --from crossref --to inveniordm --host rogue-scholar.org --token TOKEN\n\
            commonmeta put record.json --from commonmeta --to inveniordm --host my.invenio.host --token TOKEN",
        )
        .arg(
            Arg::new("input")
                .help("File path, DOI, or URL")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input format; auto-detected if omitted"),
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
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input_arg = matches.get_one::<String>("input").expect("required");
    let to = matches
        .get_one::<String>("to")
        .map(String::as_str)
        .unwrap_or("inveniordm");

    let input = if std::path::Path::new(input_arg).exists() {
        std::fs::read_to_string(input_arg)
            .map_err(|e| format!("failed to read '{}': {}", input_arg, e))?
    } else {
        input_arg.clone()
    };

    let from = match matches.get_one::<String>("from") {
        Some(f) => f.clone(),
        None => detect_format(&input),
    };

    let data = commonmeta::read(&from, &input).map_err(|e| e.to_string())?;

    match to {
        "inveniordm" => put_to_inveniordm(&data, matches),
        "crossref_xml" | "datacite" => Err(format!(
            "put: --to {} is not yet implemented (registration is currently only supported with --to inveniordm)",
            to
        )),
        other => Err(format!("put: unsupported --to target: {}", other)),
    }
}

fn put_to_inveniordm(data: &Data, matches: &ArgMatches) -> Result<(), String> {
    let host = matches
        .get_one::<String>("host")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "put: --to inveniordm requires --host <host>".to_string())?;
    let token = matches
        .get_one::<String>("token")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "put: --to inveniordm requires --token <token>".to_string())?;

    let result = commonmeta::put_inveniordm(data, host, token);
    let output = serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?;
    println!("{}", output);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_requires_host_for_inveniordm() {
        let dir = std::env::temp_dir().join("commonmeta_put_test_host");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let matches = command().get_matches_from(vec![
            "put",
            path.to_str().unwrap(),
            "--from",
            "commonmeta",
        ]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("requires --host"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_rejects_unimplemented_to() {
        let dir = std::env::temp_dir().join("commonmeta_put_test_to");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let matches = command().get_matches_from(vec![
            "put",
            path.to_str().unwrap(),
            "--from",
            "commonmeta",
            "--to",
            "crossref_xml",
        ]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("not yet implemented"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_rejects_missing_token() {
        let dir = std::env::temp_dir().join("commonmeta_put_test_token");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("record.json");
        std::fs::write(
            &path,
            r#"{"id":"https://doi.org/10.1/a","type":"JournalArticle"}"#,
        )
        .unwrap();

        let matches = command().get_matches_from(vec![
            "put",
            path.to_str().unwrap(),
            "--from",
            "commonmeta",
            "--host",
            "example.invenio.host",
        ]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("requires --token"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_file_not_found_reads_as_literal_input() {
        // A nonexistent path is treated as a literal identifier string (e.g.
        // a DOI), not a file-read error, matching convert.rs's behavior.
        let matches =
            command().get_matches_from(vec!["put", "not-a-real-path-or-doi", "--from", "commonmeta"]);
        // commonmeta::read with from="commonmeta" will fail to parse the
        // literal string as JSON, surfacing a parse error rather than a
        // missing-host error — confirms we got past the file-read branch.
        let err = execute(&matches).unwrap_err();
        assert!(!err.contains("failed to read"));
    }
}
