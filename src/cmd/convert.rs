/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgMatches, Command};

pub fn command() -> Command {
    Command::new("convert")
        .about("Convert scholarly metadata between formats")
        .long_about(
            "Convert scholarly metadata between formats.\n\n\
            The input is a file path or a DOI/URL. When --from is omitted the\n\
            format is auto-detected: DOIs are resolved via the DOI RA API;\n\
            JSON files are inspected for schema markers.\n\n\
            Supported input formats:  crossref, commonmeta\n\
            Supported output formats: commonmeta, csl\n\n\
            Examples:\n\n\
            commonmeta convert 10.5555/12345678\n\
            commonmeta convert https://doi.org/10.59350/gj8re-sca95 --to csl\n\
            commonmeta convert record.json --from commonmeta --to csl --file out.json",
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
                .help("Input format (crossref, commonmeta); auto-detected if omitted"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .short('t')
                .help("Output format (commonmeta, csl)")
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
    json.as_array()?.first()?.get("RA")?.as_str().map(|s| s.to_lowercase())
}

fn detect_format(input: &str) -> String {
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
    }
    "commonmeta".to_string()
}

// ─── Execute ─────────────────────────────────────────────────────────────────

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input_arg = matches.get_one::<String>("input").expect("required");
    let to = matches.get_one::<String>("to").expect("has default");
    let out_file = matches.get_one::<String>("file");

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

    let style = matches.get_one::<String>("style").map(String::as_str);
    let locale = matches.get_one::<String>("locale").map(String::as_str);

    let output = if to == "citation" {
        commonmeta::convert_citation(&from, &input, style, locale).map_err(|e| e.to_string())?
    } else {
        commonmeta::convert(&from, to, &input).map_err(|e| e.to_string())?
    };

    // Pretty-print JSON output.
    let pretty: Vec<u8> = serde_json::from_slice::<serde_json::Value>(&output)
        .ok()
        .and_then(|v| serde_json::to_vec_pretty(&v).ok())
        .unwrap_or(output);

    match out_file {
        Some(path) => std::fs::write(path, &pretty)
            .map_err(|e| format!("failed to write '{}': {}", path, e)),
        None => {
            println!("{}", String::from_utf8_lossy(&pretty));
            Ok(())
        }
    }
}
