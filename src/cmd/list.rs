use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command};
use rusqlite::Connection;
use serde_json::json;
use url::Url;

use commonmeta::Data;
use crate::file_utils;

/// Maximum number of records per Parquet batch file.
const PARQUET_BATCH_SIZE: usize = 100_000;

pub fn command() -> Command {
    Command::new("list")
        .about("A list of scholarly metadata")
        .long_about(
            "A list of scholarly metadata retrieved via file or API.\n\n\
            Examples:\n\n\
            commonmeta list --number 10 --member 78 --type journal-article --from crossref\n\
            commonmeta list --number 10 --client cern.zenodo --type dataset --from datacite\n\
            commonmeta list --number 10 --from openalex --type journal-article\n\
            commonmeta list crossref-2026-06-15.sqlite3 --from vraix --number 1000 --page 1 --to commonmeta --file out.json.gz\n\
            commonmeta list --from crossref --file out.json\n\
            commonmeta list --from crossref --number 1000 --file out.parquet\n\
            (a .parquet --file extension selects Parquet output and is only supported for\n\
            --to commonmeta, the default; output is always zstd-compressed, with records\n\
            split into batches of 100,000 written in parallel, e.g. out-00000.parquet.zst, ...)",
        )
        .arg(
            Arg::new("input")
                .help("Optional input file path (JSON/JSONL, or SQLite for --from vraix)")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input source format")
                .default_value("crossref"),
        )
        .arg(
            Arg::new("to")
                .long("to")
                .short('t')
                .help("Output format")
                .default_value("commonmeta"),
        )
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
        .arg(
            Arg::new("file")
                .long("file")
                .help("Write output to file instead of stdout"),
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
        .unwrap_or("commonmeta");
    let out_file = matches.get_one::<String>("file");

    if !matches!(from, "crossref" | "datacite" | "openalex" | "vraix") {
        return Err(format!(
            "list: --from {} is not implemented yet (supported: crossref, datacite, openalex, vraix)",
            from
        ));
    }
    if !is_supported_output_format(to) {
        return Err(format!("list: unsupported --to format: {}", to));
    }

    let data = if let Some(input_path) = matches.get_one::<String>("input") {
        load_list_from_file(input_path, from, matches)?
    } else {
        if from == "vraix" {
            return Err("list: --from vraix requires an input SQLite file path".to_string());
        }
        fetch_list_from_api(matches, from)?
    };

    // Parquet is a storage format, not a metadata schema, so it is selected via
    // the --file extension (e.g. out.parquet) rather than --to. It is only
    // supported for the native commonmeta schema until other flattened
    // tabular representations (e.g. for datacite, csl) are added.
    if let Some(path) = out_file {
        let (_base, extension, _compress) = file_utils::get_extension(path, ".json");
        if extension == ".parquet" {
            if to != "commonmeta" {
                return Err(format!(
                    "list: --file *.parquet output is only supported for --to commonmeta (got --to {}), until other flattened formats are added",
                    to
                ));
            }
            return write_parquet_batches(&data, path);
        }
    }

    let output = write_output(&data, to)?;

    match out_file {
        Some(path) => {
            let (file, _extension, compress) = file_utils::get_extension(path, ".json");
            match compress.as_str() {
                "gz" => file_utils::write_gz_file(&file, &output)
                    .map_err(|e| format!("failed to write gzip '{}': {}", path, e)),
                "zip" => file_utils::write_zip_file(&file, &output)
                    .map_err(|e| format!("failed to write zip '{}': {}", path, e)),
                "zst" => file_utils::write_zst_file(&file, &output)
                    .map_err(|e| format!("failed to write zst '{}': {}", path, e)),
                _ => file_utils::write_file(&file, &output)
                    .map_err(|e| format!("failed to write '{}': {}", path, e)),
            }
        }
        None => {
            println!("{}", String::from_utf8_lossy(&output));
            Ok(())
        }
    }
}

fn is_supported_output_format(to: &str) -> bool {
    matches!(
        to,
        "commonmeta"
            | "csl"
            | "datacite"
            | "inveniordm"
            | "schemaorg"
            | "ror"
            | "bibtex"
            | "ris"
            | "crossref_xml"
    )
}

/// Write `data` as one or more zstd-compressed Parquet files, splitting into
/// batches of `PARQUET_BATCH_SIZE` records and writing the batches in parallel.
/// When there is only one batch, the output is written to `out_path` (with a
/// `.zst` suffix); otherwise each batch gets a numbered suffix.
fn write_parquet_batches(data: &[Data], out_path: &str) -> Result<(), String> {
    if data.is_empty() {
        return Err("list: no records to write".to_string());
    }

    let (base_path, _extension, _compress) = file_utils::get_extension(out_path, ".parquet");
    let chunks: Vec<&[Data]> = data.chunks(PARQUET_BATCH_SIZE).collect();
    let multi = chunks.len() > 1;

    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .enumerate()
            .map(|(idx, chunk)| {
                let base_path = base_path.clone();
                scope.spawn(move || {
                    write_parquet_batch(chunk, &base_path, if multi { Some(idx) } else { None })
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("list: parquet batch thread panicked".to_string()))
            })
            .collect()
    });

    results.into_iter().collect::<Result<Vec<()>, String>>()?;
    Ok(())
}

fn write_parquet_batch(chunk: &[Data], base_path: &Path, idx: Option<usize>) -> Result<(), String> {
    let bytes = commonmeta::write_parquet(chunk).map_err(|e| e.to_string())?;
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(bytes), 0)
        .map_err(|e| format!("failed to zstd-compress parquet batch: {}", e))?;

    let path = parquet_batch_path(base_path, idx);
    file_utils::write_file(&path, &compressed)
        .map_err(|e| format!("failed to write '{}': {}", path.display(), e))?;
    println!("wrote {} ({} records)", path.display(), chunk.len());
    Ok(())
}

/// Build the output path for a Parquet batch: `{base}.zst` when `idx` is
/// `None`, or `{stem}-{idx:05}.{ext}.zst` for numbered batches.
fn parquet_batch_path(base_path: &Path, idx: Option<usize>) -> PathBuf {
    match idx {
        None => {
            let mut path = base_path.to_path_buf();
            let name = format!("{}.zst", path.file_name().unwrap_or_default().to_string_lossy());
            path.set_file_name(name);
            path
        }
        Some(i) => {
            let stem = base_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let ext = base_path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            let parent = base_path.parent().unwrap_or_else(|| Path::new(""));
            let filename = if ext.is_empty() {
                format!("{}-{:05}.zst", stem, i)
            } else {
                format!("{}-{:05}.{}.zst", stem, i, ext)
            };
            parent.join(filename)
        }
    }
}

fn write_output(data: &[Data], to: &str) -> Result<Vec<u8>, String> {
    if matches!(to, "commonmeta" | "csl" | "datacite" | "inveniordm" | "schemaorg" | "ror") {
        let mut items: Vec<serde_json::Value> = Vec::with_capacity(data.len());
        for item in data {
            let rendered = render_single(item, to)?;
            let value: serde_json::Value = serde_json::from_slice(&rendered)
                .map_err(|e| format!("failed to parse {} output as JSON: {}", to, e))?;
            items.push(value);
        }
        return serde_json::to_vec_pretty(&items).map_err(|e| e.to_string());
    }

    let mut output = String::new();
    for (idx, item) in data.iter().enumerate() {
        let rendered = render_single(item, to)?;
        if idx > 0 {
            output.push_str("\n");
        }
        output.push_str(&String::from_utf8_lossy(&rendered));
    }
    Ok(output.into_bytes())
}

fn render_single(data: &Data, to: &str) -> Result<Vec<u8>, String> {
    let input = serde_json::to_string(data).map_err(|e| e.to_string())?;
    commonmeta::convert("commonmeta", to, &input).map_err(|e| e.to_string())
}

fn fetch_list_from_api(matches: &ArgMatches, from: &str) -> Result<Vec<Data>, String> {
    let number = *matches.get_one::<usize>("number").unwrap_or(&10);
    let page = *matches.get_one::<usize>("page").unwrap_or(&1);

    match from {
        "crossref" => commonmeta::crossref::fetch_all(
            number,
            page,
            matches
                .get_one::<String>("member")
                .map(String::as_str)
                .unwrap_or(""),
            matches
                .get_one::<String>("type")
                .map(String::as_str)
                .unwrap_or(""),
            matches.get_flag("sample"),
            matches
                .get_one::<String>("year")
                .map(String::as_str)
                .unwrap_or(""),
            matches
                .get_one::<String>("ror")
                .map(String::as_str)
                .unwrap_or(""),
            matches
                .get_one::<String>("orcid")
                .map(String::as_str)
                .unwrap_or(""),
            matches.get_flag("has-orcid"),
            matches.get_flag("has-ror-id"),
            matches.get_flag("has-references"),
            matches.get_flag("has-relation"),
            matches.get_flag("has-abstract"),
            matches.get_flag("has-award"),
            matches.get_flag("has-license"),
            matches.get_flag("has-archive"),
        )
        .map_err(|e| e.to_string()),
        "datacite" => fetch_datacite_list(matches, number, page),
        "openalex" => fetch_openalex_list(matches, number, page),
        _ => Err(format!("unsupported source: {from}")),
    }
}

fn fetch_datacite_list(matches: &ArgMatches, number: usize, page: usize) -> Result<Vec<Data>, String> {
    let mut url =
        Url::parse("https://api.datacite.org/dois").map_err(|e| format!("invalid URL: {}", e))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("page[size]", &number.clamp(1, 1000).to_string());
        query.append_pair("page[number]", &page.max(1).to_string());
        query.append_pair("affiliation", "true");

        if let Some(client_id) = matches.get_one::<String>("client") {
            if !client_id.is_empty() {
                query.append_pair("client-id", client_id);
            }
        }

        let mut search_terms: Vec<String> = Vec::new();
        if let Some(type_) = matches.get_one::<String>("type") {
            if !type_.is_empty() {
                search_terms.push(format!("types.resourceTypeGeneral:{}", type_));
            }
        }
        if let Some(year) = matches.get_one::<String>("year") {
            if !year.is_empty() {
                search_terms.push(format!("publicationYear:{}", year));
            }
        }
        if let Some(language) = matches.get_one::<String>("language") {
            if !language.is_empty() {
                search_terms.push(format!("language:{}", language));
            }
        }
        if let Some(orcid) = matches.get_one::<String>("orcid") {
            if !orcid.is_empty() {
                search_terms.push(format!("creators.nameIdentifiers.nameIdentifier:{}", orcid));
            }
        }
        if let Some(ror) = matches.get_one::<String>("ror") {
            if !ror.is_empty() {
                search_terms.push(format!("creators.affiliation.affiliationIdentifier:{}", ror));
            }
        }
        if !search_terms.is_empty() {
            query.append_pair("query", &search_terms.join(" "));
        }
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| format!("http client build failed: {}", e))?;

    let text = client
        .get(url.as_str())
        .send()
        .map_err(|e| format!("http request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("http status error: {}", e))?
        .text()
        .map_err(|e| format!("failed to read response: {}", e))?;

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid DataCite response: {}", e))?;
    let items = value
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "DataCite response missing data array".to_string())?;

    let mut out: Vec<Data> = Vec::with_capacity(items.len());
    for item in items {
        out.push(convert_datacite_item(item)?);
    }
    Ok(out)
}

fn fetch_openalex_list(matches: &ArgMatches, number: usize, page: usize) -> Result<Vec<Data>, String> {
    let mut url =
        Url::parse("https://api.openalex.org/works").map_err(|e| format!("invalid URL: {}", e))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("per-page", &number.clamp(1, 200).to_string());
        query.append_pair("page", &page.max(1).to_string());
        if let Some(email) = matches.get_one::<String>("email") {
            if !email.is_empty() {
                query.append_pair("mailto", email);
            }
        }

        let mut filters: Vec<String> = Vec::new();
        if let Some(type_) = matches.get_one::<String>("type") {
            if !type_.is_empty() {
                filters.push(format!("type_crossref:{}", type_));
            }
        }
        if let Some(year) = matches.get_one::<String>("year") {
            if !year.is_empty() {
                filters.push(format!("from_publication_date:{}-01-01", year));
                filters.push(format!("to_publication_date:{}-12-31", year));
            }
        }
        if let Some(orcid) = matches.get_one::<String>("orcid") {
            if !orcid.is_empty() {
                filters.push(format!("author.orcid:{}", orcid));
            }
        }
        if let Some(ror) = matches.get_one::<String>("ror") {
            if !ror.is_empty() {
                filters.push(format!("institutions.ror:{}", ror));
            }
        }
        if matches.get_flag("has-abstract") {
            filters.push("has_abstract:true".to_string());
        }
        if matches.get_flag("has-references") {
            filters.push("referenced_works_count:>0".to_string());
        }
        if !filters.is_empty() {
            query.append_pair("filter", &filters.join(","));
        }
    }

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!(
            "commonmeta-rs/{} (https://github.com/front-matter/commonmeta-rs; mailto:info@front-matter.de)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| format!("http client build failed: {}", e))?;

    let text = client
        .get(url.as_str())
        .send()
        .map_err(|e| format!("http request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("http status error: {}", e))?
        .text()
        .map_err(|e| format!("failed to read response: {}", e))?;

    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid OpenAlex response: {}", e))?;
    let items = value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "OpenAlex response missing results array".to_string())?;

    let mut out: Vec<Data> = Vec::with_capacity(items.len());
    for item in items {
        out.push(convert_openalex_item(item)?);
    }
    Ok(out)
}

fn load_list_from_file(path: &str, from: &str, matches: &ArgMatches) -> Result<Vec<Data>, String> {
    match from {
        "crossref" => load_crossref_list_from_file(path),
        "datacite" => load_datacite_list_from_file(path),
        "openalex" => load_openalex_list_from_file(path),
        "vraix" => load_vraix_list_from_sqlite(path, matches),
        _ => Err(format!("unsupported source: {from}")),
    }
}

fn load_crossref_list_from_file(path: &str) -> Result<Vec<Data>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path, e))?;

    if path.ends_with(".jsonl") || path.ends_with(".jsonlines") {
        return parse_crossref_jsonlines(&content);
    }

    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON in '{}': {}", path, e))?;

    let mut out: Vec<Data> = Vec::new();
    if let Some(items) = value
        .get("items")
        .and_then(serde_json::Value::as_array)
        .or_else(|| {
            value
                .get("message")
                .and_then(|m| m.get("items"))
                .and_then(serde_json::Value::as_array)
        })
    {
        for item in items {
            out.push(convert_crossref_item(item)?);
        }
        return Ok(out);
    }

    if let Some(items) = value.as_array() {
        for item in items {
            out.push(convert_crossref_item(item)?);
        }
        return Ok(out);
    }

    Err("unsupported Crossref list file format; expected JSON array, {items:[...]}, or JSON Lines".to_string())
}

fn parse_crossref_jsonlines(content: &str) -> Result<Vec<Data>, String> {
    let mut out: Vec<Data> = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| format!("invalid JSON at line {}: {}", index + 1, e))?;
        out.push(convert_crossref_item(&value)?);
    }
    Ok(out)
}

fn convert_crossref_item(item: &serde_json::Value) -> Result<Data, String> {
    let envelope = json!({ "message": item });
    let input = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    let bytes = commonmeta::convert("crossref", "commonmeta", &input)
        .map_err(|e| format!("crossref conversion failed: {}", e))?;
    serde_json::from_slice::<Data>(&bytes).map_err(|e| format!("failed to parse output JSON: {}", e))
}

fn load_datacite_list_from_file(path: &str) -> Result<Vec<Data>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path, e))?;

    if path.ends_with(".jsonl") || path.ends_with(".jsonlines") {
        let mut out: Vec<Data> = Vec::new();
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| format!("invalid JSON at line {}: {}", index + 1, e))?;
            out.push(convert_datacite_item(&value)?);
        }
        return Ok(out);
    }

    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON in '{}': {}", path, e))?;

    let mut out: Vec<Data> = Vec::new();
    if let Some(items) = value.get("data").and_then(serde_json::Value::as_array) {
        for item in items {
            out.push(convert_datacite_item(item)?);
        }
        return Ok(out);
    }
    if let Some(items) = value.as_array() {
        for item in items {
            out.push(convert_datacite_item(item)?);
        }
        return Ok(out);
    }

    Err("unsupported DataCite list file format; expected JSON array, {data:[...]}, or JSON Lines".to_string())
}

fn convert_datacite_item(item: &serde_json::Value) -> Result<Data, String> {
    let envelope = if item.get("data").is_some() {
        item.clone()
    } else {
        json!({ "data": item })
    };
    let input = serde_json::to_string(&envelope).map_err(|e| e.to_string())?;
    let bytes = commonmeta::convert("datacite", "commonmeta", &input)
        .map_err(|e| format!("datacite conversion failed: {}", e))?;
    serde_json::from_slice::<Data>(&bytes).map_err(|e| format!("failed to parse output JSON: {}", e))
}

fn load_openalex_list_from_file(path: &str) -> Result<Vec<Data>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read '{}': {}", path, e))?;

    if path.ends_with(".jsonl") || path.ends_with(".jsonlines") {
        let mut out: Vec<Data> = Vec::new();
        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(trimmed)
                .map_err(|e| format!("invalid JSON at line {}: {}", index + 1, e))?;
            out.push(convert_openalex_item(&value)?);
        }
        return Ok(out);
    }

    let value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid JSON in '{}': {}", path, e))?;

    let mut out: Vec<Data> = Vec::new();
    if let Some(items) = value
        .get("results")
        .and_then(serde_json::Value::as_array)
        .or_else(|| value.as_array())
    {
        for item in items {
            out.push(convert_openalex_item(item)?);
        }
        return Ok(out);
    }

    Err("unsupported OpenAlex list file format; expected JSON array, {results:[...]}, or JSON Lines".to_string())
}

fn convert_openalex_item(item: &serde_json::Value) -> Result<Data, String> {
    let input = serde_json::to_string(item).map_err(|e| e.to_string())?;
    let bytes = commonmeta::convert("openalex", "commonmeta", &input)
        .map_err(|e| format!("openalex conversion failed: {}", e))?;
    serde_json::from_slice::<Data>(&bytes).map_err(|e| format!("failed to parse output JSON: {}", e))
}

fn load_vraix_list_from_sqlite(path: &str, matches: &ArgMatches) -> Result<Vec<Data>, String> {
    let number = *matches.get_one::<usize>("number").unwrap_or(&10);
    let page = *matches.get_one::<usize>("page").unwrap_or(&1);
    let offset = page.saturating_sub(1).saturating_mul(number);

    let connection =
        Connection::open(path).map_err(|e| format!("failed to open SQLite '{}': {}", path, e))?;
    let table = find_vraix_table(&connection)?
        .ok_or_else(|| "no VRAIX table with pid/source_id/raw_metadata found".to_string())?;

    let query = if number == 0 {
        format!(
            "SELECT source_id, raw_metadata FROM {}",
            quote_identifier(&table)
        )
    } else {
        format!(
            "SELECT source_id, raw_metadata FROM {} LIMIT ?1 OFFSET ?2",
            quote_identifier(&table)
        )
    };

    let mut statement = connection
        .prepare(&query)
        .map_err(|e| format!("failed to prepare query: {}", e))?;

    let mut out: Vec<Data> = Vec::new();
    if number == 0 {
        let rows = statement
            .query_map([], |row| {
                let source_id: i64 = row.get(0)?;
                let raw_metadata: String = row.get(1)?;
                Ok((source_id, raw_metadata))
            })
            .map_err(|e| format!("failed to query rows: {}", e))?;

        for row in rows {
            let (source_id, raw_metadata) = row.map_err(|e| e.to_string())?;
            out.push(convert_vraix_row(source_id, &raw_metadata)?);
        }
        return Ok(out);
    }

    let rows = statement
        .query_map([number as i64, offset as i64], |row| {
            let source_id: i64 = row.get(0)?;
            let raw_metadata: String = row.get(1)?;
            Ok((source_id, raw_metadata))
        })
        .map_err(|e| format!("failed to query rows: {}", e))?;

    for row in rows {
        let (source_id, raw_metadata) = row.map_err(|e| e.to_string())?;
        out.push(convert_vraix_row(source_id, &raw_metadata)?);
    }

    Ok(out)
}

fn find_vraix_table(connection: &Connection) -> Result<Option<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|e| e.to_string())?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?;

    for name in names {
        let table = name.map_err(|e| e.to_string())?;
        if table_has_vraix_columns(connection, &table)? {
            return Ok(Some(table));
        }
    }

    Ok(None)
}

fn table_has_vraix_columns(connection: &Connection, table_name: &str) -> Result<bool, String> {
    let pragma = format!("PRAGMA table_info({})", quote_identifier(table_name));
    let mut statement = connection.prepare(&pragma).map_err(|e| e.to_string())?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;

    let mut has_pid = false;
    let mut has_source_id = false;
    let mut has_raw_metadata = false;
    for column in columns {
        let col = column.map_err(|e| e.to_string())?;
        if col.eq_ignore_ascii_case("pid") {
            has_pid = true;
        }
        if col.eq_ignore_ascii_case("source_id") {
            has_source_id = true;
        }
        if col.eq_ignore_ascii_case("raw_metadata") {
            has_raw_metadata = true;
        }
    }

    Ok(has_pid && has_source_id && has_raw_metadata)
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn convert_vraix_row(source_id: i64, raw_metadata: &str) -> Result<Data, String> {
    match source_id {
        1 => {
            let value: serde_json::Value = serde_json::from_str(raw_metadata)
                .map_err(|e| format!("invalid Crossref raw_metadata: {}", e))?;
            let payload = if value.get("message").is_some() {
                value
            } else {
                json!({ "message": value })
            };
            let input = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
            let bytes = commonmeta::convert("crossref", "commonmeta", &input)
                .map_err(|e| format!("crossref conversion failed: {}", e))?;
            serde_json::from_slice::<Data>(&bytes)
                .map_err(|e| format!("failed to parse commonmeta JSON: {}", e))
        }
        2 => {
            let value: serde_json::Value = serde_json::from_str(raw_metadata)
                .map_err(|e| format!("invalid DataCite raw_metadata: {}", e))?;
            let payload = if value.get("data").is_some() {
                value
            } else {
                json!({ "data": value })
            };
            let input = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
            let bytes = commonmeta::convert("datacite", "commonmeta", &input)
                .map_err(|e| format!("datacite conversion failed: {}", e))?;
            serde_json::from_slice::<Data>(&bytes)
                .map_err(|e| format!("failed to parse commonmeta JSON: {}", e))
        }
        3 => {
            let bytes = commonmeta::convert("ror", "commonmeta", raw_metadata)
                .map_err(|e| format!("ror conversion failed: {}", e))?;
            serde_json::from_slice::<Data>(&bytes)
                .map_err(|e| format!("failed to parse commonmeta JSON: {}", e))
        }
        other => Err(format!("unsupported VRAIX source_id: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_data(id: &str) -> Data {
        Data { id: id.to_string(), type_: "JournalArticle".to_string(), ..Data::default() }
    }

    #[test]
    fn test_parquet_batch_path_single() {
        let path = parquet_batch_path(Path::new("/tmp/out.parquet"), None);
        assert_eq!(path, PathBuf::from("/tmp/out.parquet.zst"));
    }

    #[test]
    fn test_parquet_batch_path_numbered() {
        let path = parquet_batch_path(Path::new("/tmp/out.parquet"), Some(0));
        assert_eq!(path, PathBuf::from("/tmp/out-00000.parquet.zst"));

        let path = parquet_batch_path(Path::new("/tmp/out.parquet"), Some(12));
        assert_eq!(path, PathBuf::from("/tmp/out-00012.parquet.zst"));
    }

    #[test]
    fn test_parquet_batch_path_no_extension() {
        let path = parquet_batch_path(Path::new("/tmp/out"), Some(3));
        assert_eq!(path, PathBuf::from("/tmp/out-00003.zst"));
    }

    #[test]
    fn test_write_parquet_batches_single_batch() {
        let dir = std::env::temp_dir().join("commonmeta_list_parquet_single");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_batches(&data, out_path.to_str().unwrap()).unwrap();

        let zst_path = dir.join("out.parquet.zst");
        assert!(zst_path.exists());
        // no numbered batch file should exist for a single batch
        assert!(!dir.join("out-00000.parquet.zst").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_parquet_batches_multi_batch() {
        let dir = std::env::temp_dir().join("commonmeta_list_parquet_multi");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet");

        // Force multiple chunks by calling the chunking logic directly with a
        // tiny effective batch size via two manual chunks instead of relying
        // on PARQUET_BATCH_SIZE (kept at 100_000 for production use).
        let chunk_a = vec![sample_data("https://doi.org/10.1/a")];
        let chunk_b = vec![sample_data("https://doi.org/10.1/b")];

        write_parquet_batch(&chunk_a, &out_path, Some(0)).unwrap();
        write_parquet_batch(&chunk_b, &out_path, Some(1)).unwrap();

        assert!(dir.join("out-00000.parquet.zst").exists());
        assert!(dir.join("out-00001.parquet.zst").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_parquet_batches_empty_data_errors() {
        let result = write_parquet_batches(&[], "/tmp/whatever.parquet");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_parquet_batch_roundtrip_readable() {
        use parquet::file::reader::{FileReader, SerializedFileReader};

        let dir = std::env::temp_dir().join("commonmeta_list_parquet_readable");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet");

        let data = vec![sample_data("https://doi.org/10.1/a")];
        write_parquet_batches(&data, out_path.to_str().unwrap()).unwrap();

        let zst_path = dir.join("out.parquet.zst");
        let compressed = std::fs::read(&zst_path).unwrap();
        let decompressed = zstd::stream::decode_all(std::io::Cursor::new(compressed)).unwrap();

        let reader = SerializedFileReader::new(bytes::Bytes::from(decompressed)).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }
}
