/*
 * Copyright © 2026 Front Matter <info@front-matter.de>
 */

use clap::{Arg, ArgMatches, Command};
use commonmeta::file_utils;
use std::time::Instant;

/// Default Parquet row-group size — 10 000 rows per group keeps memory
/// pressure manageable for large VRAIX daily dumps (millions of rows).
const DEFAULT_BATCH_SIZE: usize = 10_000;

pub fn command() -> Command {
    Command::new("package")
        .about("Write a VRAIX SQLite dump as a Parquet file")
        .long_about(
            "Read a VRAIX transport table from a local SQLite3 file and write \
            its raw columns (pid, source_id, raw_metadata, …) to a Parquet \
            file. The output uses the VRAIX Arrow schema and is suitable for \
            analytics with DuckDB, Polars, or DataFusion.\n\n\
            The source_id column is stored as a string dictionary \
            ('crossref', 'datacite', 'ror') so downstream queries can filter \
            by name without joining a lookup table.\n\n\
            Append a compression extension to --file to compress the output:\n\
              .parquet       plain Parquet\n\
              .parquet.zst   zstd-compressed Parquet\n\
              .parquet.zip   ZIP archive containing the Parquet file\n\
              .parquet.tgz   tar+gzip archive containing the Parquet file\n\n\
            Examples:\n\n\
            commonmeta package crossref-2026-06-15.sqlite3\n\
            commonmeta package crossref-2026-06-15.sqlite3 --file crossref-2026-06-15.parquet\n\
            commonmeta package crossref-2026-06-15.sqlite3 --file crossref-2026-06-15.parquet.zst\n\
            commonmeta package crossref-2026-06-15.sqlite3 --file crossref-2026-06-15.parquet.zip\n\
            commonmeta package crossref-2026-06-15.sqlite3 --batch-size 50000 --timer",
        )
        .arg(
            Arg::new("input")
                .help("Path to the VRAIX SQLite3 file")
                .required(true)
                .index(1),
        )
        .arg(Arg::new("file").long("file").value_name("FILE").help(
            "Output file path. Defaults to the input path with .sqlite3 \
                    replaced by .parquet. Append .zst/.zip/.tgz to compress.",
        ))
        .arg(
            Arg::new("batch_size")
                .long("batch-size")
                .value_name("N")
                .help("Rows per Parquet row group (default: 10000)")
                .value_parser(clap::value_parser!(usize)),
        )
        .arg(
            Arg::new("timer")
                .long("timer")
                .help("Print total packaging duration to stderr")
                .action(clap::ArgAction::SetTrue),
        )
}

pub fn execute(matches: &ArgMatches) -> Result<(), String> {
    let timer = matches.get_flag("timer");
    let started = Instant::now();
    let input = matches.get_one::<String>("input").expect("required");
    let batch_size = matches
        .get_one::<usize>("batch_size")
        .copied()
        .unwrap_or(DEFAULT_BATCH_SIZE);

    let out_path = match matches.get_one::<String>("file") {
        Some(p) => p.clone(),
        None => default_output_path(input),
    };

    let bytes =
        commonmeta::write_vraix_table_parquet(input, batch_size).map_err(|e| e.to_string())?;

    write_output(&bytes, &out_path)?;

    eprintln!("wrote {} rows → {}", parquet_row_count(&bytes), out_path);
    if timer {
        eprintln!("package timer: {:?}", started.elapsed());
    }

    Ok(())
}

/// Write `bytes` to `out_path`, compressing if the path ends with a known
/// compression extension (.zst, .zip, .tgz). Uses the same `file_utils`
/// helpers as `cmd::list`.
fn write_output(bytes: &[u8], out_path: &str) -> Result<(), String> {
    let (base_path, _inner_ext, compress) = file_utils::get_extension(out_path, ".parquet");

    match compress.as_str() {
        "zst" => {
            let compressed =
                zstd::encode_all(bytes, 0).map_err(|e| format!("zstd encoding: {e}"))?;
            file_utils::write_file(out_path, &compressed)
                .map_err(|e| format!("writing {out_path}: {e}"))
        }
        "zip" => {
            let entry_name = base_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            file_utils::write_zip_archive(out_path, &[(entry_name, bytes.to_vec())])
                .map_err(|e| format!("writing {out_path}: {e}"))
        }
        "tgz" => {
            let entry_name = base_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            file_utils::write_tar_gz_archive(out_path, &[(entry_name, bytes.to_vec())])
                .map_err(|e| format!("writing {out_path}: {e}"))
        }
        _ => {
            file_utils::write_file(out_path, bytes).map_err(|e| format!("writing {out_path}: {e}"))
        }
    }
}

fn default_output_path(input: &str) -> String {
    // Strip any trailing .zst, then replace .sqlite3 with .parquet.
    let base = input.strip_suffix(".zst").unwrap_or(input);
    if let Some(stem) = base.strip_suffix(".sqlite3") {
        format!("{stem}.parquet")
    } else {
        format!("{base}.parquet")
    }
}

/// Return the row count stored in the Parquet file metadata without
/// re-reading all the data — used only for the progress message.
fn parquet_row_count(bytes: &[u8]) -> i64 {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    SerializedFileReader::new(::bytes::Bytes::copy_from_slice(bytes))
        .map(|r| r.metadata().file_metadata().num_rows())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_output_path_replaces_sqlite3_extension() {
        assert_eq!(
            default_output_path("crossref-2026-06-15.sqlite3"),
            "crossref-2026-06-15.parquet"
        );
    }

    #[test]
    fn default_output_path_strips_zst_then_replaces_sqlite3() {
        assert_eq!(
            default_output_path("crossref-2026-06-15.sqlite3.zst"),
            "crossref-2026-06-15.parquet"
        );
    }

    #[test]
    fn default_output_path_unknown_extension() {
        assert_eq!(default_output_path("dump.db"), "dump.db.parquet");
    }

    #[test]
    fn write_output_plain_roundtrip() {
        let dir = std::env::temp_dir().join(format!("commonmeta-dump-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.parquet").to_string_lossy().into_owned();
        let data = b"PAR1fake";

        write_output(data, &path).unwrap();

        let read_back = std::fs::read(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(read_back, data);
    }

    #[test]
    fn write_output_zst_roundtrip() {
        let dir = std::env::temp_dir().join(format!("commonmeta-dump-zst-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.parquet.zst").to_string_lossy().into_owned();
        let data = b"PAR1fake";

        write_output(data, &path).unwrap();

        let decompressed = file_utils::read_zst_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(decompressed, data);
    }
}
