/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use std::path::Path;
use std::time::Instant;

use clap::{Arg, ArgAction, ArgMatches, Command};

use commonmeta::{self, file_utils};

use crate::cmd::{resolve_db_path, PIDBOX_CACHE_KEY, PIDBOX_URL, VRAIX_CACHE_TTL};
use crate::cmd::convert::detect_format;
use crate::cmd::list::{fetch_list_from_api, fmt_wrote_sqlite};

pub fn command() -> Command {
    Command::new("import")
        .about("Import scholarly metadata into the local commonmeta database")
        .long_about(
            "Download and import scholarly metadata into the local commonmeta SQLite \
            database (always upserts — existing records are updated, not replaced).\n\n\
            The output path defaults to the COMMONMETA_DB environment variable or \
            the platform default (~/Library/Application Support/commonmeta/commonmeta.sqlite3 \
            on macOS, /var/lib/commonmeta/commonmeta.sqlite3 on Linux).\n\n\
            Examples:\n\n\
            commonmeta import 10.7554/elife.01561\n\
            commonmeta import https://doi.org/10.7554/elife.01561\n\
            commonmeta import 10.7554/elife.01561 --from crossref\n\
            commonmeta import --from pidbox\n\
            commonmeta import --from crossref --date 2026-06-15\n\
            commonmeta import --from datacite --date 2026-06-15\n\
            commonmeta import crossref-2026-06-15.sqlite3\n\
            commonmeta import --from crossref --number 100 --member 78\n\
            commonmeta import --from datacite --number 100 --client cern.zenodo\n\
            commonmeta import --from openalex --number 100 --type journal-article\n\
            commonmeta import --from ror",
        )
        .arg(
            Arg::new("input")
                .help("DOI, URL, or VRAIX SQLite file path (auto-detected)")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::new("from")
                .long("from")
                .short('f')
                .help("Source format: crossref, datacite, openalex, pidbox, ror")
                .default_value("commonmeta"),
        )
        .arg(
            Arg::new("number")
                .long("number")
                .help("Number of records to fetch via API (file and date inputs always import all)")
                .value_parser(clap::value_parser!(usize))
                .default_value("0"),
        )
        .arg(
            Arg::new("page")
                .long("page")
                .help("Page number for API fetches (1-based)")
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
        .arg(Arg::new("affiliation").long("affiliation").help("Affiliation name filter"))
        .arg(Arg::new("country").long("country").help("Country code filter"))
        .arg(Arg::new("date-updated").long("date-updated").help("Filter by date updated (YYYY-MM-DD)"))
        .arg(Arg::new("from-host").long("from-host").help("InvenioRDM source host"))
        .arg(Arg::new("from-token").long("from-token").help("InvenioRDM source API token"))
        .arg(Arg::new("community").long("community").help("InvenioRDM community slug"))
        .arg(Arg::new("subject").long("subject").help("Subject area filter"))
        .arg(Arg::new("depositor").long("depositor").help("Crossref depositor name"))
        .arg(Arg::new("registrant").long("registrant").help("Crossref registrant name"))
        .arg(
            Arg::new("email")
                .long("email")
                .help("Email for OpenAlex mailto parameter"),
        )
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
            Arg::new("is-archived")
                .long("is-archived")
                .help("Filter for archived records")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("vocabulary")
                .long("vocabulary")
                .help("Output as vocabulary (e.g. InvenioRDM affiliations YAML)")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("match")
                .long("match")
                .help("Enable ROR affiliation matching when reading crossref and datacite records")
                .default_value("true")
                .value_parser(clap::value_parser!(bool)),
        )
        .arg(Arg::new("date").long("date").help(
            "Date (YYYY-MM-DD) of a VRAIX daily dump; downloads \
            {from}-{date}.sqlite3.zst from metadata.vraix.org when no input \
            file path is given",
        ))
        .arg(
            Arg::new("no-network")
                .long("no-network")
                .help("Disable all outbound network requests; only local .sqlite3 file imports are allowed")
                .action(ArgAction::SetTrue),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let input_path = matches.get_one::<String>("input").map(String::as_str);
    let date = matches.get_one::<String>("date").map(String::as_str);

    // Auto-detect source from VRAIX filename pattern ({source}-{date}.sqlite3).
    let is_sqlite_input = input_path
        .map(|p| file_utils::get_extension(p, ".json").1 == ".sqlite3")
        .unwrap_or(false);
    let filename_source: Option<&'static str> = if is_sqlite_input {
        input_path
            .and_then(|p| std::path::Path::new(p).file_stem()?.to_str())
            .and_then(|stem| {
                if stem.starts_with("crossref-") { Some("crossref") }
                else if stem.starts_with("datacite-") { Some("datacite") }
                else { None }
            })
    } else {
        None
    };
    let from_explicit = matches.get_one::<String>("from").map(String::as_str).unwrap_or("commonmeta");
    let from_flag: &str = filename_source.unwrap_or(from_explicit);

    // When a source-specific flag is provided but --from is not set, auto-select
    // the matching source (e.g. `import --member 78` implies --from crossref).
    let from_flag = if from_flag == "commonmeta" && input_path.is_none() {
        let has_member = matches.get_one::<String>("member").map(|s| !s.is_empty()).unwrap_or(false);
        let has_client = matches.get_one::<String>("client").map(|s| !s.is_empty()).unwrap_or(false);
        if has_member { "crossref" }
        else if has_client { "datacite" }
        else { from_flag }
    } else {
        from_flag
    };

    // Positional shorthand: `import ror` / `import pidbox` / `import crossref` etc.
    // is treated as `import --from <source>` when --from was not explicitly given
    // and the positional arg is a known source name rather than a DOI or file path.
    let (from, input_path) = match input_path {
        Some(s)
            if from_flag == "commonmeta"
                && !is_sqlite_input
                && matches!(s, "crossref" | "datacite" | "openalex" | "pidbox" | "ror") =>
        {
            (s, None)
        }
        _ => (from_flag, input_path),
    };

    if !matches!(from, "crossref" | "datacite" | "openalex" | "pidbox" | "ror" | "commonmeta") {
        return Err(format!(
            "import: unsupported --from value '{}' (supported: crossref, datacite, openalex, pidbox, ror)",
            from
        ));
    }

    // When --no-network is set, only a local VRAIX .sqlite3 file is accepted.
    // Everything else (DOI lookups, API fetches, date downloads, ror/pidbox installs)
    // requires outbound network access.
    let no_network = matches.get_flag("no-network");
    if no_network && !(is_sqlite_input && input_path.is_some()) {
        return Err(
            "--no-network requires a local .sqlite3 input file; \
            provide a VRAIX dump path or remove --no-network"
                .to_string(),
        );
    }

    // ROR is a vocabulary install, not a metadata records import.
    if from == "ror" {
        let out_path = resolve_db_path(None);
        return install_ror(&out_path);
    }

    // pidbox is a full VRAIX dump installed directly into commonmeta.sqlite3.
    if from == "pidbox" {
        let out_path = resolve_db_path(None);
        return install_pidbox(&out_path);
    }

    let out_path = resolve_db_path(None);
    let is_vraix_sqlite = is_sqlite_input && matches!(from, "crossref" | "datacite");
    let is_date_download = date.is_some() && input_path.is_none() && matches!(from, "crossref" | "datacite");

    // Fast path: stream VRAIX SQLite → commonmeta SQLite without loading all
    // records into RAM. Always imports every row (limit=0). Always upserts.
    if is_vraix_sqlite || is_date_download {
        return import_vraix_fast(from, input_path, date, &out_path);
    }

    // Single-record path: DOI, URL, or any identifier that isn't a file path.
    // Auto-detect the source format from the identifier when --from is not given.
    if let Some(identifier) = input_path {
        if !is_sqlite_input {
            let effective_from = if from_explicit == "commonmeta" {
                detect_format(identifier)
            } else {
                from_explicit.to_string()
            };
            return import_single(identifier, &effective_from, &out_path);
        }
    }

    // API fetch path: fetch records, then upsert into commonmeta SQLite.
    if from == "commonmeta" {
        return Err(
            "import: --from commonmeta requires an input .sqlite3 file path".to_string()
        );
    }
    let fetch_start = Instant::now();
    let data = fetch_list_from_api(matches, from)?;
    eprintln!(
        "import: fetch took {:.2?} ({} records)",
        fetch_start.elapsed(),
        data.len()
    );

    let out_sqlite = Path::new(&out_path);
    let write_start = Instant::now();
    commonmeta::upsert_sqlite(&data, out_sqlite).map_err(|e| e.to_string())?;
    let total = commonmeta::count_sqlite_works(out_sqlite).ok();
    eprintln!(
        "import: upsert took {:.2?} ({} records)",
        write_start.elapsed(),
        data.len()
    );
    println!("{}", fmt_wrote_sqlite(&out_path, data.len(), total));
    Ok(())
}

/// Fetch a single record by DOI, URL, or other identifier and upsert it.
fn import_single(identifier: &str, from: &str, out_path: &str) -> Result<(), String> {
    let fetch_start = Instant::now();
    let data = commonmeta::read(from, identifier).map_err(|e| e.to_string())?;
    eprintln!("import: fetch took {:.2?}", fetch_start.elapsed());

    let out_sqlite = Path::new(out_path);
    let write_start = Instant::now();
    commonmeta::upsert_sqlite(std::slice::from_ref(&data), out_sqlite)
        .map_err(|e| e.to_string())?;
    let total = commonmeta::count_sqlite_works(out_sqlite).ok();
    eprintln!("import: upsert took {:.2?}", write_start.elapsed());
    println!("{}", fmt_wrote_sqlite(out_path, 1, total));
    Ok(())
}

/// Stream a VRAIX SQLite dump (local file or downloaded daily dump) directly
/// into the commonmeta database with upsert semantics. Always imports all rows.
fn import_vraix_fast(
    from: &str,
    input_path: Option<&str>,
    date: Option<&str>,
    out_path: &str,
) -> Result<(), String> {
    let total_start = Instant::now();
    let out_sqlite = std::path::PathBuf::from(out_path);

    // Resolve the VRAIX input to a local .sqlite3 path, downloading and
    // decompressing on demand when only --date was given.
    let (in_sqlite, tmp_to_clean) = if date.is_some() && input_path.is_none() {
        let date = date.unwrap();
        let url = format!("https://metadata.vraix.org/{}-{}.sqlite3.zst", from, date);
        let cache_key = format!("{}-{}.sqlite3.zst", from, date);
        let dl_start = Instant::now();
        let (cache_path, from_cache) =
            file_utils::ensure_cached_path(&url, "vraix", &cache_key, VRAIX_CACHE_TTL)
                .map_err(|e| format!("failed to download '{}': {}", url, e))?;
        let size = cache_path.metadata().map(|m| m.len()).unwrap_or(0);
        eprintln!(
            "import: download took {:.2?} ({} bytes{})",
            dl_start.elapsed(),
            size,
            if from_cache { ", from cache" } else { "" }
        );
        let dc_start = Instant::now();
        let tmp = out_sqlite.with_extension(format!("sqlite3.vraix-{}.tmp", std::process::id()));
        let dc_bytes = file_utils::decompress_zst_file(&cache_path, &tmp)
            .map_err(|e| format!("failed to decompress '{}': {}", url, e))?;
        eprintln!(
            "import: decompress took {:.2?} ({} bytes)",
            dc_start.elapsed(),
            dc_bytes
        );
        (tmp.clone(), Some(tmp))
    } else {
        (std::path::PathBuf::from(input_path.unwrap()), None)
    };

    let convert_start = Instant::now();
    let result = commonmeta::stream_vraix_to_sqlite(&in_sqlite, from, &out_sqlite, 0, true)
        .map_err(|e| e.to_string());
    if let Some(tmp) = tmp_to_clean {
        std::fs::remove_file(&tmp).ok();
    }
    let n = result?;
    let total = commonmeta::count_sqlite_works(&out_sqlite).ok();
    eprintln!(
        "import: convert+write took {:.2?} ({} records)",
        convert_start.elapsed(),
        n
    );
    eprintln!("import: total took {:.2?}", total_start.elapsed());
    println!("{}", fmt_wrote_sqlite(out_path, n, total));
    Ok(())
}

pub(crate) fn install_ror(out_path: &str) -> Result<(), String> {
    let total = Instant::now();

    eprintln!("Fetching latest ROR release metadata from Zenodo...");
    let t = Instant::now();
    let release = commonmeta::fetch_latest_ror_release().map_err(|e| e.to_string())?;
    eprintln!("  metadata fetched in {:.2}s", t.elapsed().as_secs_f64());

    let db_path = Path::new(out_path);
    match commonmeta::fetch_installed_ror_version(db_path).map_err(|e| e.to_string())? {
        Some(ref installed) if installed == &release.version => {
            println!(
                "ROR {} ({}) is already installed at {}",
                release.version, release.date, out_path
            );
            return Ok(());
        }
        Some(ref installed) => {
            eprintln!("Upgrading ROR {} → {}...", installed, release.version);
        }
        None => {}
    }

    let t = Instant::now();
    let (list, from_cache) =
        commonmeta::download_ror_release(&release).map_err(|e| e.to_string())?;
    eprintln!(
        "  {} and parsed {} organizations in {:.2}s",
        if from_cache { "loaded" } else { "downloaded" },
        list.len(),
        t.elapsed().as_secs_f64()
    );

    eprintln!("Writing to {}...", out_path);
    let t = Instant::now();
    commonmeta::write_ror_sqlite(&list, db_path, Some(&release.version), Some(&release.date))
        .map_err(|e| e.to_string())?;
    eprintln!("  SQLite written in {:.2}s", t.elapsed().as_secs_f64());
    eprintln!("  total: {:.2}s", total.elapsed().as_secs_f64());

    println!(
        "Installed ROR {} ({}) → {} ({} organizations)",
        release.version,
        release.date,
        out_path,
        list.len(),
    );
    Ok(())
}

pub(crate) fn install_pidbox(out_path: &str) -> Result<(), String> {
    let total = Instant::now();

    eprintln!("Downloading pidbox from {}...", PIDBOX_URL);
    let t = Instant::now();
    let (cache_path, from_cache) =
        file_utils::ensure_cached_path(PIDBOX_URL, "vraix", PIDBOX_CACHE_KEY, VRAIX_CACHE_TTL)
            .map_err(|e| format!("failed to download pidbox: {}", e))?;
    if from_cache {
        eprintln!("  pidbox download skipped (cached at {})", cache_path.display());
    } else {
        eprintln!("  downloaded in {:.2}s", t.elapsed().as_secs_f64());
    }

    // The pidbox SQLite database is not VACUUM'd, so overflow pages for large
    // records appear in reverse page-number order.  stream_zst_pidbox_to_sqlite
    // uses a sliding window buffer (default 32 GiB RAM + 500 GiB disk) to
    // resolve backward chain links without extra full-file scans.
    // Tune with COMMONMETA_SCAN_WINDOW_GIB and COMMONMETA_SCAN_DISK_GIB.
    let out = Path::new(out_path);
    eprintln!("Converting (streaming decompress + convert) → {}…", out_path);
    let t = Instant::now();
    let n = commonmeta::stream_zst_pidbox_to_sqlite(&cache_path, out, 0)
        .map_err(|e| format!("failed to convert pidbox: {}", e))?;
    eprintln!("  converted and wrote {} records in {:.0}s", n, t.elapsed().as_secs_f64());
    eprintln!("  total: {:.0}s", total.elapsed().as_secs_f64());

    let date = commonmeta::fetch_installed_vraix_date(out)
        .ok()
        .flatten()
        .map(|d| format!(", vraix_date: {d}"))
        .unwrap_or_default();
    println!("Installed pidbox → {} ({} records{})", out_path, n, date);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> clap::ArgMatches {
        command().try_get_matches_from(args).expect("arg parse failed")
    }

    #[test]
    fn test_no_network_with_doi_errors() {
        let m = parse_args(&["import", "--no-network", "10.7554/elife.01567"]);
        let err = execute(&m).unwrap_err();
        assert!(
            err.contains("--no-network"),
            "expected --no-network in error, got: {err}"
        );
    }

    #[test]
    fn test_no_network_with_api_fetch_errors() {
        let m = parse_args(&["import", "--no-network", "--from", "crossref", "--ror", "00pd74e08"]);
        let err = execute(&m).unwrap_err();
        assert!(
            err.contains("--no-network"),
            "expected --no-network in error, got: {err}"
        );
    }

    #[test]
    fn test_no_network_with_local_sqlite_passes_guard() {
        // Use a generic .sqlite3 name (not the crossref-/datacite- VRAIX pattern)
        // so the guard passes and the command fails at the "from commonmeta requires
        // a .sqlite3 path" check rather than entering the slow streaming path.
        let m = parse_args(&["import", "--no-network", "local.sqlite3"]);
        let err = execute(&m).unwrap_err();
        assert!(
            !err.contains("--no-network"),
            "should not fail at network guard for local sqlite, got: {err}"
        );
    }
}
