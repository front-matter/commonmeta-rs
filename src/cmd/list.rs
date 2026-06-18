use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::json;
use url::Url;

use commonmeta::Data;
use commonmeta::file_utils;

/// Maximum number of records per output batch, used both for Parquet batch
/// files and for entries within a `.zip`/`.tgz` archive.
const BATCH_SIZE: usize = 100_000;

/// How long a downloaded VRAIX dump stays valid in the local cache before
/// it's re-downloaded. Dumps are daily snapshots that don't change once
/// published, so this is purely a disk-space/staleness bound, not a
/// correctness one.
const VRAIX_CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

pub fn command() -> Command {
    Command::new("list")
        .about("A list of scholarly metadata")
        .long_about(
            "A list of scholarly metadata retrieved via file or API.\n\n\
            Examples:\n\n\
            commonmeta list --number 10 --member 78 --type journal-article --from crossref\n\
            commonmeta list --number 10 --client cern.zenodo --type dataset --from datacite\n\
            commonmeta list --number 10 --from openalex --type journal-article\n\
            commonmeta list --from crossref --file out.json\n\
            commonmeta list --from crossref --number 1000 --file out.parquet\n\
            (a .parquet --file extension selects Parquet output and is only supported for\n\
            --to commonmeta, the default; output is always zstd-compressed, with records\n\
            split into batches of 100,000 written in parallel, e.g. out-00000.parquet.zst, ...)\n\
            commonmeta list --from crossref --number 1000 --file out.parquet.zip\n\
            (combining .parquet with .zip/.tgz packs the same zstd-compressed batches\n\
            into a single archive instead of writing them loose to disk)\n\
            commonmeta list batch-commonmeta-00000.parquet.zst --to csl\n\
            (a .parquet/.parquet.zst input is auto-detected as a commonmeta Parquet dump\n\
            written by --file *.parquet, regardless of --from, and read back into a list)\n\
            commonmeta list --from crossref --date 2026-06-14\n\
            commonmeta list --from datacite --date 2026-06-14\n\
            commonmeta list datacite-2026-06-14.sqlite3.zst --from datacite\n\
            (--date pairs with --from crossref or --from datacite to read a VRAIX daily\n\
            dump SQLite database; --from picks both the dump file, {from}-{date}.sqlite3.zst,\n\
            and how its rows are parsed. With no input path, the file is downloaded from\n\
            metadata.vraix.org and decompressed. A .sqlite3/.sqlite3.zst input path is\n\
            auto-detected the same way --file *.parquet is, so --date can be omitted when\n\
            pointing directly at an already-downloaded dump; it's only needed to name the\n\
            file to download when no input path is given)\n\
            commonmeta list --from crossref --date 2026-06-14 --number 0 --file out.zip\n\
            commonmeta list --from datacite --date 2026-06-14 --number 0 --file out.tgz\n\
            (a .zip/.tgz --file extension archives the records as multiple batched\n\
            entries of 100,000 records each instead of one in-memory buffer, useful with\n\
            --number 0, which fetches every row in a VRAIX dump)",
        )
        .arg(
            Arg::new("input")
                .help("Optional input file path (JSON/JSONL, Parquet, or SQLite with --date)")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Input source format")
                .default_value("commonmeta"),
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
        .arg(
            Arg::new("date")
                .long("date")
                .help(
                    "Date (YYYY-MM-DD) of a VRAIX daily dump, used with --from crossref or \
                     --from datacite; downloads {from}-{date}.sqlite3.zst from \
                     metadata.vraix.org unless an input file path is also given",
                ),
        )
        .arg(
            Arg::new("timers")
                .long("timers")
                .help("Print timing for download/parse/write phases to stderr")
                .action(ArgAction::SetTrue),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let from = matches
        .get_one::<String>("from")
        .map(String::as_str)
        .unwrap_or("commonmeta");
    let to = matches
        .get_one::<String>("to")
        .map(String::as_str)
        .unwrap_or("commonmeta");
    let out_file = matches.get_one::<String>("file");
    let date = matches.get_one::<String>("date").map(String::as_str);
    let timers = matches.get_flag("timers");

    if !matches!(from, "crossref" | "datacite" | "openalex" | "commonmeta") {
        return Err(format!(
            "list: --from {} is not implemented yet (supported: crossref, datacite, openalex, commonmeta)",
            from
        ));
    }
    if !is_supported_output_format(to) {
        return Err(format!("list: unsupported --to format: {}", to));
    }

    let input_path = matches.get_one::<String>("input").map(String::as_str);
    // A `.sqlite3`/`.sqlite3.zst` input is unambiguously a VRAIX dump (see
    // `write_parquet`'s sibling concept for `.parquet`), so detect it from
    // the extension rather than requiring `--date` to be passed just to
    // pick the loader — `--date` is only otherwise needed to name the file
    // to download when no local input path is given.
    let is_vraix_sqlite_input = input_path
        .map(|p| file_utils::get_extension(p, ".json").1 == ".sqlite3")
        .unwrap_or(false);

    let data = if date.is_some() || is_vraix_sqlite_input {
        if !matches!(from, "crossref" | "datacite") {
            return Err(
                "list: reading a VRAIX SQLite dump requires --from crossref or --from datacite"
                    .to_string(),
            );
        }
        load_vraix_list_for_date(date.unwrap_or(""), input_path, from, matches, timers)?
    } else if let Some(input_path) = input_path {
        load_list_from_file(input_path, from)?
    } else {
        if from == "commonmeta" {
            return Err("list: --from commonmeta requires an input Parquet file path".to_string());
        }
        fetch_list_from_api(matches, from)?
    };

    // Parquet is a storage format, not a metadata schema, so it is selected via
    // the --file extension (e.g. out.parquet) rather than --to. It is only
    // supported for the native commonmeta schema until other flattened
    // tabular representations (e.g. for datacite, csl) are added.
    if let Some(path) = out_file {
        let (_base, extension, compress) = file_utils::get_extension(path, ".json");
        if extension == ".parquet" {
            if to != "commonmeta" {
                return Err(format!(
                    "list: --file *.parquet output is only supported for --to commonmeta (got --to {}), until other flattened formats are added",
                    to
                ));
            }
            let write_start = Instant::now();
            // e.g. --file out.parquet.zip packs the zstd-compressed batch
            // files into a single zip/tgz archive instead of writing them
            // loose to disk.
            let result = if compress == "zip" || compress == "tgz" {
                write_parquet_archive(&data, path, &compress)
            } else {
                write_parquet_batches(&data, path)
            };
            if timers {
                eprintln!(
                    "list: write {} took {:.2?} ({} records)",
                    to,
                    write_start.elapsed(),
                    data.len()
                );
            }
            return result;
        }
        // A .zip/.tgz --file extension archives the records as multiple
        // batched entries (e.g. for a large --number 0 VRAIX dump) instead
        // of rendering everything into one in-memory buffer.
        if compress == "zip" || compress == "tgz" {
            let write_start = Instant::now();
            let result = write_archive_batches(&data, to, path, &compress);
            if timers {
                eprintln!(
                    "list: write {} took {:.2?} ({} records)",
                    to,
                    write_start.elapsed(),
                    data.len()
                );
            }
            return result;
        }
    }

    let write_start = Instant::now();
    let output = write_output(&data, to)?;
    if timers {
        eprintln!(
            "list: write {} took {:.2?} ({} records)",
            to,
            write_start.elapsed(),
            data.len()
        );
    }

    match out_file {
        Some(path) => {
            let (file, _extension, compress) = file_utils::get_extension(path, ".json");
            match compress.as_str() {
                "gz" => file_utils::write_gz_file(&file, &output)
                    .map_err(|e| format!("failed to write gzip '{}': {}", path, e)),
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

/// Write `data` as a single zstd-compressed Parquet file at `out_path` (with
/// a `.zst` suffix added). `commonmeta::write_parquet` internally
/// parallelizes the CPU-heavy flattening step across row groups, but always
/// returns one combined buffer — so this always produces one file,
/// regardless of how large `data` is.
fn write_parquet_batches(data: &[Data], out_path: &str) -> Result<(), String> {
    if data.is_empty() {
        return Err("list: no records to write".to_string());
    }

    let (base_path, _extension, _compress) = file_utils::get_extension(out_path, ".parquet");
    write_parquet_batch(data, &base_path)
}

fn write_parquet_batch(data: &[Data], base_path: &Path) -> Result<(), String> {
    let bytes = commonmeta::write_parquet(data).map_err(|e| e.to_string())?;
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(bytes), 0)
        .map_err(|e| format!("failed to zstd-compress parquet: {}", e))?;

    let path = parquet_batch_path(base_path);
    file_utils::write_file(&path, &compressed)
        .map_err(|e| format!("failed to write '{}': {}", path.display(), e))?;
    println!("wrote {} ({} records)", path.display(), data.len());
    Ok(())
}

/// Build the output path for the Parquet file: `{base}.zst`.
fn parquet_batch_path(base_path: &Path) -> PathBuf {
    let mut path = base_path.to_path_buf();
    let name = format!("{}.zst", path.file_name().unwrap_or_default().to_string_lossy());
    path.set_file_name(name);
    path
}

/// Write `data` as a single zstd-compressed Parquet file packed into a
/// `.zip` (`compress == "zip"`) or `.tgz` (`compress == "tgz"`) archive,
/// e.g. for `--file out.parquet.zip`.
fn write_parquet_archive(data: &[Data], out_path: &str, compress: &str) -> Result<(), String> {
    if data.is_empty() {
        return Err("list: no records to write".to_string());
    }

    let (base_path, _extension, _compress) = file_utils::get_extension(out_path, ".parquet");
    let base_name = base_path.file_name().unwrap_or_default().to_string_lossy().to_string();

    let entry = parquet_archive_entry(data, &base_name)?;

    match compress {
        "zip" => file_utils::write_zip_archive(out_path, std::slice::from_ref(&entry))
            .map_err(|e| format!("failed to write zip '{}': {}", out_path, e))?,
        "tgz" => file_utils::write_tar_gz_archive(out_path, std::slice::from_ref(&entry))
            .map_err(|e| format!("failed to write tgz '{}': {}", out_path, e))?,
        other => return Err(format!("list: unsupported archive compression: {}", other)),
    }
    println!("wrote {} ({} records)", out_path, data.len());
    Ok(())
}

/// Render `data` as a single zstd-compressed Parquet blob and name it as an
/// archive entry, reusing `parquet_batch_path`'s naming but dropping any
/// directory component since archive entries are flat names.
fn parquet_archive_entry(data: &[Data], base_name: &str) -> Result<(String, Vec<u8>), String> {
    let bytes = commonmeta::write_parquet(data).map_err(|e| e.to_string())?;
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(bytes), 0)
        .map_err(|e| format!("failed to zstd-compress parquet: {}", e))?;

    let entry_name = parquet_batch_path(Path::new(base_name))
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    Ok((entry_name, compressed))
}

/// Write `data` as multiple batched entries inside a single `.zip` (`compress
/// == "zip"`) or `.tgz` (`compress == "tgz"`) archive, rendered to `to`
/// format. Batching and entry naming are handled by `commonmeta::write_archive`;
/// this just picks the archive container format and persists it.
fn write_archive_batches(data: &[Data], to: &str, out_path: &str, compress: &str) -> Result<(), String> {
    let (base_path, inner_ext, _) = file_utils::get_extension(out_path, ".json");
    let base_path = if base_path.extension().is_none() {
        let inner_ext = if inner_ext.is_empty() { ".json" } else { &inner_ext };
        base_path.with_extension(inner_ext.trim_start_matches('.'))
    } else {
        base_path
    };
    let base_name = base_path.file_name().unwrap_or_default().to_string_lossy().to_string();

    let entries =
        commonmeta::write_archive(data, to, &base_name, BATCH_SIZE).map_err(|e| e.to_string())?;

    match compress {
        "zip" => file_utils::write_zip_archive(out_path, &entries)
            .map_err(|e| format!("failed to write zip '{}': {}", out_path, e))?,
        "tgz" => file_utils::write_tar_gz_archive(out_path, &entries)
            .map_err(|e| format!("failed to write tgz '{}': {}", out_path, e))?,
        other => return Err(format!("list: unsupported archive compression: {}", other)),
    }
    println!("wrote {} ({} records in {} batch(es))", out_path, data.len(), entries.len());
    Ok(())
}

fn write_output(data: &[Data], to: &str) -> Result<Vec<u8>, String> {
    commonmeta::write_list(data, to).map_err(|e| e.to_string())
}

pub(crate) fn fetch_list_from_api(matches: &ArgMatches, from: &str) -> Result<Vec<Data>, String> {
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

        if let Some(client_id) = matches.get_one::<String>("client")
            && !client_id.is_empty() {
                query.append_pair("client-id", client_id);
            }

        let mut search_terms: Vec<String> = Vec::new();
        if let Some(type_) = matches.get_one::<String>("type")
            && !type_.is_empty() {
                search_terms.push(format!("types.resourceTypeGeneral:{}", type_));
            }
        if let Some(year) = matches.get_one::<String>("year")
            && !year.is_empty() {
                search_terms.push(format!("publicationYear:{}", year));
            }
        if let Some(language) = matches.get_one::<String>("language")
            && !language.is_empty() {
                search_terms.push(format!("language:{}", language));
            }
        if let Some(orcid) = matches.get_one::<String>("orcid")
            && !orcid.is_empty() {
                search_terms.push(format!("creators.nameIdentifiers.nameIdentifier:{}", orcid));
            }
        if let Some(ror) = matches.get_one::<String>("ror")
            && !ror.is_empty() {
                search_terms.push(format!("creators.affiliation.affiliationIdentifier:{}", ror));
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
        if let Some(email) = matches.get_one::<String>("email")
            && !email.is_empty() {
                query.append_pair("mailto", email);
            }

        let mut filters: Vec<String> = Vec::new();
        if let Some(type_) = matches.get_one::<String>("type")
            && !type_.is_empty() {
                filters.push(format!("type_crossref:{}", type_));
            }
        if let Some(year) = matches.get_one::<String>("year")
            && !year.is_empty() {
                filters.push(format!("from_publication_date:{}-01-01", year));
                filters.push(format!("to_publication_date:{}-12-31", year));
            }
        if let Some(orcid) = matches.get_one::<String>("orcid")
            && !orcid.is_empty() {
                filters.push(format!("author.orcid:{}", orcid));
            }
        if let Some(ror) = matches.get_one::<String>("ror")
            && !ror.is_empty() {
                filters.push(format!("institutions.ror:{}", ror));
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

pub(crate) fn load_list_from_file(path: &str, from: &str) -> Result<Vec<Data>, String> {
    // A `.parquet`/`.parquet.zst` input is unambiguously a commonmeta dump
    // written by `--file *.parquet` (see `write_parquet_batches`), so detect
    // it from the extension rather than relying on `--from commonmeta`.
    let (_base, extension, _compress) = file_utils::get_extension(path, ".json");
    if extension == ".parquet" {
        return load_commonmeta_list_from_parquet(path);
    }

    match from {
        "crossref" => load_crossref_list_from_file(path),
        "datacite" => load_datacite_list_from_file(path),
        "openalex" => load_openalex_list_from_file(path),
        "commonmeta" => load_commonmeta_list_from_parquet(path),
        _ => Err(format!("unsupported source: {from}")),
    }
}

/// Read a commonmeta Parquet dump (optionally zstd-compressed, e.g.
/// `batch-commonmeta-00000.parquet.zst`) back into a list of records.
fn load_commonmeta_list_from_parquet(path: &str) -> Result<Vec<Data>, String> {
    let (_base, extension, compress) = file_utils::get_extension(path, ".parquet");
    if extension != ".parquet" {
        return Err(format!(
            "list: --from commonmeta expects a .parquet (optionally .zst/.zip/.tgz) input file, got '{}'",
            path
        ));
    }

    // .zip/.tgz inputs (from --file out.parquet.zip, see write_parquet_archive)
    // hold one or more entries, each itself zstd-compressed Parquet; decompress
    // every entry and concatenate the records they parse to.
    let compressed_batches: Vec<Vec<u8>> = match compress.as_str() {
        "zip" => {
            let raw = file_utils::read_file(path).map_err(|e| format!("failed to read '{}': {}", path, e))?;
            file_utils::read_zip_entries(&raw).map_err(|e| format!("failed to read zip '{}': {}", path, e))?
        }
        "tgz" => {
            let raw = file_utils::read_file(path).map_err(|e| format!("failed to read '{}': {}", path, e))?;
            file_utils::read_tar_gz_entries(&raw)
                .map_err(|e| format!("failed to read tgz '{}': {}", path, e))?
        }
        "zst" => vec![file_utils::read_zst_file(path)
            .map_err(|e| format!("failed to read zstd-compressed '{}': {}", path, e))?],
        _ => vec![file_utils::read_file(path).map_err(|e| format!("failed to read '{}': {}", path, e))?],
    };

    let parquet_batches: Vec<Vec<u8>> = if matches!(compress.as_str(), "zip" | "tgz") {
        compressed_batches
            .into_iter()
            .map(|bytes| {
                file_utils::unzst_content(&bytes)
                    .map_err(|e| format!("failed to decompress entry in '{}': {}", path, e))
            })
            .collect::<Result<Vec<_>, String>>()?
    } else {
        compressed_batches
    };

    let mut out = Vec::new();
    for bytes in parquet_batches {
        let records =
            commonmeta::read_parquet(&bytes).map_err(|e| format!("failed to parse Parquet '{}': {}", path, e))?;
        out.extend(records);
    }
    Ok(out)
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

/// Load a VRAIX daily dump for `--from crossref`/`--from datacite` combined
/// with `--date`. With `input_path`, the local SQLite file at that path is
/// read directly (e.g. an already-downloaded dump); otherwise
/// `{from}-{date}.sqlite3.zst` is downloaded from metadata.vraix.org and
/// decompressed into a temp file first. The dump's filename (and thus
/// `from`) determines the source for every row in the file — VRAIX does not
/// mix sources within one dump — so all rows are converted using `from`.
fn load_vraix_list_for_date(
    date: &str,
    input_path: Option<&str>,
    from: &str,
    matches: &ArgMatches,
    timers: bool,
) -> Result<Vec<Data>, String> {
    let number = *matches.get_one::<usize>("number").unwrap_or(&10);
    let page = *matches.get_one::<usize>("page").unwrap_or(&1);
    let offset = page.saturating_sub(1).saturating_mul(number);
    let limit = if number == 0 { None } else { Some(number) };

    if let Some(path) = input_path {
        let convert_start = Instant::now();
        let (_base, _extension, compress) = file_utils::get_extension(path, ".json");
        let data = if compress == "zst" {
            let compressed = file_utils::read_zst_file(path)
                .map_err(|e| format!("failed to read zstd-compressed '{}': {}", path, e))?;
            // read_zst_file already decompresses; despite the name it
            // returns the decompressed bytes, ready to write straight to
            // a temp sqlite file (sqlite needs a real file path, not bytes).
            let tmp_path = std::env::temp_dir()
                .join(format!("commonmeta-vraix-local-{}-{}.sqlite3", from, std::process::id()));
            file_utils::write_file(&tmp_path, &compressed)
                .map_err(|e| format!("failed to write temp file '{}': {}", tmp_path.display(), e))?;
            let result = commonmeta::read_vraix_sqlite(tmp_path.to_str().unwrap(), from, limit, offset);
            std::fs::remove_file(&tmp_path).ok();
            result.map_err(|e| e.to_string())?
        } else {
            commonmeta::read_vraix_sqlite(path, from, limit, offset).map_err(|e| e.to_string())?
        };
        if timers {
            eprintln!(
                "list: read to commonmeta took {:.2?} ({} records)",
                convert_start.elapsed(),
                data.len()
            );
        }
        return Ok(data);
    }

    let url = format!("https://metadata.vraix.org/{}-{}.sqlite3.zst", from, date);
    let cache_key = format!("{}-{}.sqlite3.zst", from, date);

    let download_start = Instant::now();
    let (compressed, from_cache) =
        file_utils::download_file_cached(&url, "vraix", &cache_key, VRAIX_CACHE_TTL)
            .map_err(|e| format!("failed to download '{}': {}", url, e))?;
    if timers {
        if from_cache {
            eprintln!(
                "list: download took {:.2?} ({} bytes, from local cache)",
                download_start.elapsed(),
                compressed.len()
            );
        } else {
            eprintln!(
                "list: download took {:.2?} ({} bytes)",
                download_start.elapsed(),
                compressed.len()
            );
        }
    }

    let convert_start = Instant::now();
    let decompressed = file_utils::unzst_content(&compressed)
        .map_err(|e| format!("failed to decompress '{}': {}", url, e))?;

    let tmp_path = std::env::temp_dir()
        .join(format!("commonmeta-vraix-{}-{}-{}.sqlite3", from, date, std::process::id()));
    file_utils::write_file(&tmp_path, &decompressed)
        .map_err(|e| format!("failed to write temp file '{}': {}", tmp_path.display(), e))?;

    let result = commonmeta::read_vraix_sqlite(tmp_path.to_str().unwrap(), from, limit, offset);
    std::fs::remove_file(&tmp_path).ok();
    let data = result.map_err(|e| e.to_string())?;
    if timers {
        eprintln!(
            "list: read to commonmeta took {:.2?} ({} records)",
            convert_start.elapsed(),
            data.len()
        );
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn sample_data(id: &str) -> Data {
        Data { id: id.to_string(), type_: "JournalArticle".to_string(), ..Data::default() }
    }

    #[test]
    fn test_parquet_batch_path_single() {
        let path = parquet_batch_path(Path::new("/tmp/out.parquet"));
        assert_eq!(path, PathBuf::from("/tmp/out.parquet.zst"));
    }

    #[test]
    fn test_parquet_batch_path_no_extension() {
        let path = parquet_batch_path(Path::new("/tmp/out"));
        assert_eq!(path, PathBuf::from("/tmp/out.zst"));
    }

    #[test]
    fn test_write_archive_batches_zip() {
        let dir = std::env::temp_dir().join("commonmeta_list_archive_zip");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.zip");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_archive_batches(&data, "commonmeta", out_path.to_str().unwrap(), "zip").unwrap();

        assert!(out_path.exists());
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&out_path).unwrap()).unwrap();
        assert_eq!(archive.len(), 1);
        assert_eq!(archive.by_index(0).unwrap().name(), "out.json");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_archive_batches_tgz_no_inner_extension() {
        let dir = std::env::temp_dir().join("commonmeta_list_archive_tgz");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.tgz");

        let data = vec![sample_data("https://doi.org/10.1/a")];
        write_archive_batches(&data, "commonmeta", out_path.to_str().unwrap(), "tgz").unwrap();

        assert!(out_path.exists());
        let decoder = flate2::read::GzDecoder::new(std::fs::File::open(&out_path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        // "out.tgz" has no inner extension, so it defaults to ".json".
        assert_eq!(entries, vec!["out.json"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_archive_batches_empty_data_errors() {
        let result = write_archive_batches(&[], "commonmeta", "/tmp/whatever.zip", "zip");
        assert!(result.is_err());
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
    fn test_write_parquet_batches_large_list_still_one_file() {
        let dir = std::env::temp_dir().join("commonmeta_list_parquet_large");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet");

        // Internally this would span multiple Parquet row groups if
        // ROW_GROUP_SIZE were small, but that's an implementation detail of
        // commonmeta::write_parquet now — the CLI always writes one file.
        let data: Vec<Data> = (0..5).map(|i| sample_data(&format!("https://doi.org/10.1/{i}"))).collect();
        write_parquet_batches(&data, out_path.to_str().unwrap()).unwrap();

        assert!(dir.join("out.parquet.zst").exists());
        assert!(!dir.join("out-00000.parquet.zst").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_parquet_archive_zip_single_batch() {
        let dir = std::env::temp_dir().join("commonmeta_list_parquet_archive_zip");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet.zip");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_archive(&data, out_path.to_str().unwrap(), "zip").unwrap();

        assert!(out_path.exists());
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&out_path).unwrap()).unwrap();
        assert_eq!(archive.len(), 1);
        assert_eq!(archive.by_index(0).unwrap().name(), "out.parquet.zst");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_parquet_archive_tgz_single_entry() {
        let (name, bytes) = parquet_archive_entry(&[sample_data("https://doi.org/10.1/a")], "out.parquet").unwrap();
        assert_eq!(name, "out.parquet.zst");
        assert!(!bytes.is_empty());

        let dir = std::env::temp_dir().join("commonmeta_list_parquet_archive_tgz");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet.tgz");
        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_archive(&data, out_path.to_str().unwrap(), "tgz").unwrap();

        let decoder = flate2::read::GzDecoder::new(std::fs::File::open(&out_path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(entries, vec!["out.parquet.zst"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_parquet_archive_empty_data_errors() {
        let result = write_parquet_archive(&[], "/tmp/whatever.parquet.zip", "zip");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_zst() {
        let dir = std::env::temp_dir().join("commonmeta_list_load_parquet_zst");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("batch-commonmeta.parquet");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_batches(&data, out_path.to_str().unwrap()).unwrap();

        let zst_path = dir.join("batch-commonmeta.parquet.zst");
        let loaded = load_commonmeta_list_from_parquet(zst_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "https://doi.org/10.1/a");
        assert_eq!(loaded[1].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_zip_round_trip() {
        let dir = std::env::temp_dir().join("commonmeta_list_load_parquet_zip");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet.zip");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_archive(&data, out_path.to_str().unwrap(), "zip").unwrap();

        let loaded = load_commonmeta_list_from_parquet(out_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "https://doi.org/10.1/a");
        assert_eq!(loaded[1].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_tgz_round_trip() {
        let dir = std::env::temp_dir().join("commonmeta_list_load_parquet_tgz");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet.tgz");

        let data = vec![sample_data("https://doi.org/10.1/a"), sample_data("https://doi.org/10.1/b")];
        write_parquet_archive(&data, out_path.to_str().unwrap(), "tgz").unwrap();

        let loaded = load_commonmeta_list_from_parquet(out_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "https://doi.org/10.1/a");
        assert_eq!(loaded[1].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_zip_multi_batch_round_trip() {
        let dir = std::env::temp_dir().join("commonmeta_list_load_parquet_zip_multi");
        std::fs::create_dir_all(&dir).unwrap();
        let out_path = dir.join("out.parquet.zip");

        // The writer always produces a single entry now, but the reader
        // still supports multiple (e.g. archives from an older version, or
        // hand-assembled ones) by concatenating every entry's records.
        let chunk_a = vec![sample_data("https://doi.org/10.1/a")];
        let chunk_b = vec![sample_data("https://doi.org/10.1/b")];
        let mut entry_a = parquet_archive_entry(&chunk_a, "out.parquet").unwrap();
        let entry_b = parquet_archive_entry(&chunk_b, "out.parquet").unwrap();
        entry_a.0 = "out-00000.parquet.zst".to_string();
        let entry_b = ("out-00001.parquet.zst".to_string(), entry_b.1);
        file_utils::write_zip_archive(&out_path, &[entry_a, entry_b]).unwrap();

        let loaded = load_commonmeta_list_from_parquet(out_path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "https://doi.org/10.1/a");
        assert_eq!(loaded[1].id, "https://doi.org/10.1/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_uncompressed() {
        let dir = std::env::temp_dir().join("commonmeta_list_load_parquet_plain");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.parquet");

        let bytes = commonmeta::write_parquet(&[sample_data("https://doi.org/10.1/a")]).unwrap();
        std::fs::write(&path, bytes).unwrap();

        let loaded = load_commonmeta_list_from_parquet(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "https://doi.org/10.1/a");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_commonmeta_list_from_parquet_wrong_extension() {
        let result = load_commonmeta_list_from_parquet("/tmp/whatever.json");
        assert!(result.is_err());
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

    /// VRAIX dumps don't mix sources within one file: the dump's filename
    /// (and thus `--from`) determines what every `raw_metadata` row is.
    fn write_vraix_sqlite(path: &Path, rows: &[(&str, &str)]) {
        std::fs::remove_file(path).ok();
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT);")
            .unwrap();
        for (pid, raw_metadata) in rows {
            connection
                .execute(
                    "INSERT INTO works (pid, source_id, raw_metadata) VALUES (?1, ?2, ?3)",
                    rusqlite::params![pid, 1i64, raw_metadata],
                )
                .unwrap();
        }
    }

    #[test]
    fn test_load_vraix_list_for_date_uses_local_input_path() {
        let dir = std::env::temp_dir().join("commonmeta_list_vraix_local_date");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("datacite.sqlite3");
        write_vraix_sqlite(
            &path,
            &[("10.5678/b", r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#)],
        );

        let matches = command().get_matches_from(vec!["list", "--number", "10", "--page", "1"]);
        let data = load_vraix_list_for_date(
            "2026-06-14",
            Some(path.to_str().unwrap()),
            "datacite",
            &matches,
            false,
        )
        .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.5678/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_date_requires_crossref_or_datacite_from() {
        let matches =
            command().get_matches_from(vec!["list", "--from", "openalex", "--date", "2026-06-14"]);
        let err = execute(&matches).unwrap_err();
        assert!(err.contains("requires --from crossref or --from datacite"));
    }

    #[test]
    fn test_load_vraix_list_for_date_decompresses_local_zst_input() {
        let dir = std::env::temp_dir().join("commonmeta_list_vraix_local_zst");
        std::fs::create_dir_all(&dir).unwrap();
        let sqlite_path = dir.join("datacite.sqlite3");
        write_vraix_sqlite(
            &sqlite_path,
            &[("10.5678/b", r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#)],
        );

        let raw = std::fs::read(&sqlite_path).unwrap();
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(raw), 0).unwrap();
        let zst_path = dir.join("datacite.sqlite3.zst");
        std::fs::write(&zst_path, compressed).unwrap();

        let matches = command().get_matches_from(vec!["list", "--number", "10", "--page", "1"]);
        let data = load_vraix_list_for_date(
            "2026-06-14",
            Some(zst_path.to_str().unwrap()),
            "datacite",
            &matches,
            false,
        )
        .unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].id, "https://doi.org/10.5678/b");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_auto_detects_sqlite_input_without_date() {
        let dir = std::env::temp_dir().join("commonmeta_list_execute_sqlite_no_date");
        std::fs::create_dir_all(&dir).unwrap();
        let sqlite_path = dir.join("datacite.sqlite3");
        write_vraix_sqlite(
            &sqlite_path,
            &[("10.5678/b", r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#)],
        );

        // No --date here: the .sqlite3 input extension alone must be enough
        // to route into the VRAIX loader instead of the plain JSON loaders
        // (which would otherwise try `read_to_string` on this binary file).
        let matches = command()
            .get_matches_from(vec!["list", sqlite_path.to_str().unwrap(), "--from", "datacite"]);
        let result = execute(&matches);
        assert!(result.is_ok(), "expected success, got: {:?}", result);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_execute_auto_detects_sqlite_zst_input_without_date() {
        let dir = std::env::temp_dir().join("commonmeta_list_execute_sqlite_zst_no_date");
        std::fs::create_dir_all(&dir).unwrap();
        let sqlite_path = dir.join("datacite.sqlite3");
        write_vraix_sqlite(
            &sqlite_path,
            &[("10.5678/b", r#"{"data":{"id":"10.5678/b","attributes":{"doi":"10.5678/b"}}}"#)],
        );
        let raw = std::fs::read(&sqlite_path).unwrap();
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(raw), 0).unwrap();
        let zst_path = dir.join("datacite.sqlite3.zst");
        std::fs::write(&zst_path, compressed).unwrap();

        let matches =
            command().get_matches_from(vec!["list", zst_path.to_str().unwrap(), "--from", "datacite"]);
        let result = execute(&matches);
        assert!(result.is_ok(), "expected success, got: {:?}", result);

        std::fs::remove_dir_all(&dir).ok();
    }
}
