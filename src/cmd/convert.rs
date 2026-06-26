/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgAction, ArgMatches, Command};
use std::path::Path;

use crate::cmd::resolve_db_path;

pub fn command() -> Command {
    Command::new("convert")
        .about("Convert scholarly metadata between formats")
        .long_about(
            "Convert scholarly metadata between formats.\n\n\
            The input is a file path, a DOI/URL, or a ROR organization ID. \
            When --from is omitted the format is auto-detected: DOIs are \
            resolved via the DOI RA API; ROR URLs are detected by pattern; \
            JSON files are inspected for schema markers.\n\n\
            For ROR input, a local 'commonmeta.sqlite3' in the current \
            directory (produced by 'commonmeta list --to ror --file \
            commonmeta.sqlite3') is queried first — faster and offline. \
            The ROR API is used as a fallback when no local database exists.\n\n\
            Supported input formats:  crossref, commonmeta, ror\n\
            Supported output formats: commonmeta, csl, ror, inveniordm\n\n\
            Examples:\n\n\
            commonmeta convert 10.5555/12345678\n\
            commonmeta convert https://doi.org/10.59350/gj8re-sca95 --to csl\n\
            commonmeta convert https://ror.org/02nr0ka47\n\
            commonmeta convert https://ror.org/02nr0ka47 --to inveniordm\n\
            commonmeta convert record.json --from commonmeta --to csl --file out.json",
        )
        .arg(
            Arg::new("input")
                .help("File path, DOI, URL, or ROR ID")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input format (crossref, commonmeta, ror); auto-detected if omitted"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .short('t')
                .help("Output format (commonmeta, csl, ror, inveniordm)")
                .default_value("commonmeta"),
        )
        .arg(
            Arg::new("style")
                .long("style")
                .short('s')
                .help("CSL style name for citation output (default: apa)"),
        )
        .arg(
            Arg::new("locale")
                .long("locale")
                .short('l')
                .help("BCP 47 locale for citation output (e.g. de-DE)"),
        )
        .arg(
            Arg::new("file")
                .long("file")
                .help("Write output to this file instead of stdout"),
        )
        .arg(
            Arg::new("no-network")
                .long("no-network")
                .help("Disable all outbound network requests; fails if the operation would require network access")
                .action(ArgAction::SetTrue),
        )
}

// ─── Format detection ─────────────────────────────────────────────────────────

fn doi_prefix(s: &str) -> Option<String> {
    let doi = s
        .trim_start_matches("https://doi.org/")
        .trim_start_matches("http://doi.org/")
        .trim_start_matches("https://dx.doi.org/")
        .trim_start_matches("http://dx.doi.org/");
    if doi.starts_with("10.") {
        doi.find('/').map(|i| doi[..i].to_string())
    } else {
        None
    }
}

fn ra_for_prefix(prefix: &str) -> Option<String> {
    let url = format!("https://doi.org/ra/{prefix}");
    let resp = reqwest::blocking::get(&url).ok()?;
    let json: serde_json::Value = resp.json().ok()?;
    json.as_array()?
        .first()?
        .get("RA")?
        .as_str()
        .map(|s| s.to_lowercase())
}

pub(crate) fn detect_format(input: &str) -> String {
    // ROR URL or bare ROR ID
    if commonmeta::utils::validate_ror(input).is_some() {
        return "ror".to_string();
    }
    // DOI URL or bare DOI → look up registration agency
    if let Some(prefix) = doi_prefix(input) {
        return ra_for_prefix(&prefix).unwrap_or_else(|| "crossref".to_string());
    }
    // JSON content → inspect schema markers
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
        if v.get("schema_version")
            .and_then(|s| s.as_str())
            .map(|s| s.starts_with("https://commonmeta.org"))
            .unwrap_or(false)
        {
            return "commonmeta".to_string();
        }
        // Crossref API envelope
        if v.get("message-type").is_some() {
            return "crossref".to_string();
        }
        // ROR record
        if v.get("id")
            .and_then(|s| s.as_str())
            .map(|s| s.starts_with("https://ror.org/"))
            .unwrap_or(false)
            && v.get("names").is_some()
        {
            return "ror".to_string();
        }
    }
    "commonmeta".to_string()
}

// ─── Execute ─────────────────────────────────────────────────────────────────

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input_arg = matches.get_one::<String>("input").expect("required");
    let out_file = matches.get_one::<String>("file");
    let no_network = matches.get_flag("no-network");
    let style = matches.get_one::<String>("style").map(String::as_str);
    let locale = matches.get_one::<String>("locale").map(String::as_str);
    let to_arg = matches.get_one::<String>("to").expect("has default");

    let is_local_file = Path::new(input_arg).exists();

    // When --no-network is set and the input is a DOI/URL, look it up in the
    // local commonmeta database instead of fetching from the API.
    if no_network && !is_local_file && doi_prefix(input_arg).is_some() {
        let bare = input_arg
            .trim_start_matches("https://doi.org/")
            .trim_start_matches("http://doi.org/")
            .trim_start_matches("https://dx.doi.org/")
            .trim_start_matches("http://dx.doi.org/");
        let doi_url = format!("https://doi.org/{}", bare);
        let db_path_str = resolve_db_path(None);
        let db_path = Path::new(&db_path_str);
        if !db_path.exists() {
            return Err(format!(
                "local database not found at '{}'; \
                run 'commonmeta import {}' or remove --no-network",
                db_path_str, input_arg
            ));
        }
        let data = commonmeta::read_sqlite_by_id(&doi_url, db_path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!(
                "'{}' not found in local database '{}'; \
                run 'commonmeta import {}' or remove --no-network",
                input_arg, db_path_str, input_arg
            ))?;
        let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
        let to_arg = to_arg.as_str();
        let output = if to_arg == "citation" {
            commonmeta::convert_citation("commonmeta", &json, style, locale)
                .map_err(|e| e.to_string())?
        } else {
            commonmeta::convert("commonmeta", to_arg, &json).map_err(|e| e.to_string())?
        };
        return write_output(&output, to_arg, out_file);
    }

    let input = if is_local_file {
        std::fs::read_to_string(input_arg)
            .map_err(|e| format!("failed to read '{}': {}", input_arg, e))?
    } else {
        input_arg.clone()
    };

    let from = match matches.get_one::<String>("from") {
        Some(f) => f.clone(),
        None => detect_format(&input),
    };

    // For ROR input the natural default output is "ror", not "commonmeta".
    let to_arg = to_arg.as_str();
    let to = if from == "ror" && to_arg == "commonmeta" {
        "ror"
    } else {
        to_arg
    };

    // ── ROR input path ────────────────────────────────────────────────────────
    if from == "ror" {
        // Normalize the input to a full ROR URL for the SQLite lookup.
        let ror_id = commonmeta::utils::normalize_ror(&input);
        if ror_id.is_empty() {
            return Err(format!("'{}' is not a valid ROR identifier", input));
        }

        // Prefer the local SQLite database (COMMONMETA_DB > platform default);
        // fall back to the ROR API unless --no-network was requested.
        let db_path_str = resolve_db_path(None);
        let db_path = Path::new(&db_path_str);
        let data = if db_path.exists() {
            commonmeta::fetch_ror_sqlite(&ror_id, db_path).map_err(|e| e.to_string())?
        } else if no_network {
            return Err(format!(
                "ROR lookup requires network access (local database not found at '{}'); \
                run 'commonmeta import --from ror' or remove --no-network",
                db_path_str
            ));
        } else {
            commonmeta::fetch_ror(&ror_id).map_err(|e| e.to_string())?
        };

        let output = match to {
            "inveniordm" => commonmeta::write("ror", &data).map_err(|e| e.to_string())?,
            _ => commonmeta::write_ror_json(&data).map_err(|e| e.to_string())?,
        };

        return write_output(&output, to, out_file);
    }

    // ── Scholarly-work input path ─────────────────────────────────────────────
    let output = if to == "citation" {
        commonmeta::convert_citation(&from, &input, style, locale).map_err(|e| e.to_string())?
    } else {
        commonmeta::convert(&from, to, &input).map_err(|e| e.to_string())?
    };

    write_output(&output, to, out_file)
}

fn write_output(output: &[u8], to: &str, out_file: Option<&String>) -> Result<(), String> {
    // JSON formats get pretty-printed; XML/YAML stay as-is.
    let formatted: Vec<u8> = if matches!(to, "inveniordm") {
        output.to_vec()
    } else {
        serde_json::from_slice::<serde_json::Value>(output)
            .ok()
            .and_then(|v| serde_json::to_vec_pretty(&v).ok())
            .unwrap_or_else(|| output.to_vec())
    };

    match out_file {
        Some(path) => std::fs::write(path, &formatted)
            .map_err(|e| format!("failed to write '{}': {}", path, e)),
        None => {
            println!("{}", String::from_utf8_lossy(&formatted));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> clap::ArgMatches {
        command().try_get_matches_from(args).expect("arg parse failed")
    }

    #[test]
    fn test_no_network_with_doi_uses_local_db() {
        // With --no-network and a DOI, convert must look up the local database
        // instead of calling the API. Either the record is found and returned
        // (Ok), or the DB/record is absent and a "not found" error is returned.
        // In neither case should the error be an API-fetch refusal.
        let m = parse_args(&["convert", "--no-network", "10.7554/elife.01567"]);
        match execute(&m) {
            Ok(()) => {}
            Err(e) => assert!(
                e.contains("not found") || e.contains("--no-network"),
                "expected local-db error, got: {e}"
            ),
        }
    }

    #[test]
    fn test_no_network_with_doi_url_uses_local_db() {
        let m = parse_args(&["convert", "--no-network", "https://doi.org/10.7554/elife.01567"]);
        match execute(&m) {
            Ok(()) => {}
            Err(e) => assert!(
                e.contains("not found") || e.contains("--no-network"),
                "expected local-db error, got: {e}"
            ),
        }
    }

    #[test]
    fn test_no_network_with_local_json_passes_guard() {
        // Non-DOI non-file input (inline JSON) is not blocked by the network guard.
        // Fails at parse time because the JSON is not valid commonmeta — but NOT
        // with a --no-network error.
        let m = parse_args(&["convert", "--no-network", r#"{"type":"JournalArticle"}"#]);
        let err = execute(&m).unwrap_err();
        assert!(
            !err.contains("--no-network"),
            "should not fail at network guard for inline JSON, got: {err}"
        );
    }
}
