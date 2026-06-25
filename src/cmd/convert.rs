/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgMatches, Command};
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

    let input = if Path::new(input_arg).exists() {
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
    let to_arg = matches.get_one::<String>("to").expect("has default");
    let to = if from == "ror" && to_arg == "commonmeta" {
        "ror"
    } else {
        to_arg.as_str()
    };

    let style = matches.get_one::<String>("style").map(String::as_str);
    let locale = matches.get_one::<String>("locale").map(String::as_str);

    // ── ROR input path ────────────────────────────────────────────────────────
    if from == "ror" {
        // Normalize the input to a full ROR URL for the SQLite lookup.
        let ror_id = commonmeta::utils::normalize_ror(&input);
        if ror_id.is_empty() {
            return Err(format!("'{}' is not a valid ROR identifier", input));
        }

        // Prefer the local SQLite database (COMMONMETA_DB > platform default).
        let db_path_str = resolve_db_path(None);
        let db_path = Path::new(&db_path_str);
        let data = if db_path.exists() {
            commonmeta::fetch_ror_sqlite(&ror_id, db_path).map_err(|e| e.to_string())?
        } else {
            commonmeta::fetch_ror(&ror_id).map_err(|e| e.to_string())?
        };

        let output = match to {
            "inveniordm" => {
                commonmeta::write("ror", &data).map_err(|e| e.to_string())?
            }
            "ror" | _ => {
                commonmeta::write_ror_json(&data).map_err(|e| e.to_string())?
            }
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
