/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgMatches, Command};
use std::path::Path;
use std::time::Instant;

use crate::cmd::{resolve_db_path, PIDBOX_CACHE_KEY, PIDBOX_URL, VRAIX_CACHE_TTL};

pub fn command() -> Command {
    Command::new("install")
        .about("Install a vocabulary or dataset as a local SQLite database")
        .long_about(
            "Download and install a controlled vocabulary or dataset for offline use.\n\n\
            'ror' — fetches the latest ROR release from Zenodo and stores all \
            organizations in a local SQLite database with a full-text index. \
            Used by 'commonmeta match' and 'commonmeta convert'.\n\n\
            'pidbox' — downloads the full VRAIX pidbox dump from metadata.vraix.org, \
            decompresses it, and converts it to a commonmeta SQLite database. \
            The download is cached for 30 days.\n\n\
            If the latest release is already installed (ror), or a fresh cached copy \
            is available (pidbox), the heavy work is skipped.\n\n\
            Supported vocabularies: ror, pidbox\n\n\
            Examples:\n\n\
            commonmeta install ror\n\
            commonmeta install ror --file /data/ror.sqlite3\n\
            commonmeta install ror --timers\n\
            commonmeta install pidbox\n\
            commonmeta install pidbox --file /var/lib/dragoman/commonmeta.sqlite3\n\
            commonmeta install pidbox --timers",
        )
        .arg(
            Arg::new("vocabulary")
                .help("Vocabulary to install: ror, pidbox")
                .required(true)
                .index(1),
        )
        .arg(
            Arg::new("file")
                .long("file")
                .value_name("FILE")
                .help(
                    "Output SQLite file path. Overrides COMMONMETA_DB and the \
                    platform default.",
                ),
        )
        .arg(
            Arg::new("timers")
                .long("timers")
                .action(clap::ArgAction::SetTrue)
                .help("Print elapsed time for each installation step"),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let vocabulary = matches.get_one::<String>("vocabulary").expect("required");
    let out_path = resolve_db_path(matches.get_one::<String>("file"));
    let timers = matches.get_flag("timers");

    match vocabulary.as_str() {
        "ror" => install_ror(&out_path, timers),
        "pidbox" => install_pidbox(&out_path, timers),
        other => {
            eprintln!("Unsupported vocabulary '{}'. Supported: ror, pidbox", other);
            Ok(())
        }
    }
}

fn install_ror(out_path: &str, timers: bool) -> Result<(), String> {
    let total = Instant::now();

    // Step 1: fetch release metadata from Zenodo (fast, no download yet).
    eprintln!("Fetching latest ROR release metadata from Zenodo...");
    let t = Instant::now();
    let release = commonmeta::fetch_latest_ror_release().map_err(|e| e.to_string())?;
    if timers {
        eprintln!("  metadata fetched in {:.2}s", t.elapsed().as_secs_f64());
    }

    // Skip the download if the same version is already installed.
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

    // Step 2: download (or load from 30-day cache) and parse the zip.
    let t = Instant::now();
    let (list, from_cache) =
        commonmeta::download_ror_release(&release).map_err(|e| e.to_string())?;
    if from_cache {
        eprintln!(
            "ROR {} ({}) — using cached {}.",
            release.version, release.date, release.filename,
        );
    } else {
        eprintln!(
            "ROR {} ({}) — downloaded {}.",
            release.version, release.date, release.filename,
        );
    }
    if timers {
        eprintln!(
            "  {} and parsed {} organizations in {:.2}s",
            if from_cache { "loaded" } else { "downloaded" },
            list.len(),
            t.elapsed().as_secs_f64()
        );
    } else {
        eprintln!("Parsed {} organizations.", list.len());
    }

    // Step 3: write to SQLite with FTS index for offline match/convert.
    eprintln!("Writing to {}...", out_path);
    let t = Instant::now();
    commonmeta::write_ror_sqlite(
        &list,
        db_path,
        Some(&release.version),
        Some(&release.date),
    )
    .map_err(|e| e.to_string())?;
    if timers {
        eprintln!("  SQLite written in {:.2}s", t.elapsed().as_secs_f64());
        eprintln!("  total: {:.2}s", total.elapsed().as_secs_f64());
    }

    println!(
        "Installed ROR {} ({}) → {} ({} organizations)",
        release.version,
        release.date,
        out_path,
        list.len(),
    );
    Ok(())
}

fn install_pidbox(out_path: &str, timers: bool) -> Result<(), String> {
    let total = Instant::now();

    // Step 1: download (or serve from 30-day cache) the compressed pidbox dump.
    // Uses file-to-file streaming so the multi-GB download never lives in RAM.
    eprintln!("Downloading pidbox from {}...", PIDBOX_URL);
    let t = Instant::now();
    let (cache_path, from_cache) =
        commonmeta::file_utils::ensure_cached_path(PIDBOX_URL, "vraix", PIDBOX_CACHE_KEY, VRAIX_CACHE_TTL)
            .map_err(|e| format!("failed to download pidbox: {}", e))?;
    if from_cache {
        eprintln!("  pidbox download skipped (cached at {})", cache_path.display());
    } else if timers {
        eprintln!("  downloaded in {:.2}s", t.elapsed().as_secs_f64());
    }

    // Step 2: stream-decompress and convert in one pass — the decompressed
    // SQLite can be 1-3 TB and may exceed available disk space, so we read
    // the zstd file page-by-page and write commonmeta records directly to the
    // output without writing the intermediate SQLite to disk.
    let out = Path::new(out_path);
    eprintln!("Converting (streaming decompress + convert) → {}...", out_path);
    let t = Instant::now();
    let n = commonmeta::stream_zst_pidbox_to_sqlite(&cache_path, out, 0)
        .map_err(|e| format!("failed to convert pidbox: {}", e))?;
    if timers {
        eprintln!("  converted and wrote {} records in {:.2}s", n, t.elapsed().as_secs_f64());
        eprintln!("  total: {:.2}s", total.elapsed().as_secs_f64());
    }

    let date = commonmeta::fetch_installed_vraix_date(out)
        .ok()
        .flatten()
        .map(|d| format!(", vraix_date: {d}"))
        .unwrap_or_default();
    println!("Installed pidbox → {} ({} records{})", out_path, n, date);
    Ok(())
}
