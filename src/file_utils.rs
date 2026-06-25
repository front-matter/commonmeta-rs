use flate2::Compression;
use flate2::write::GzEncoder;
use reqwest::blocking::Client;
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;

// ---------- error handling ----------

#[derive(Error, Debug)]
pub enum FileError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Status code error: {status} {text}")]
    StatusCode { status: u16, text: String },

    #[error("download of '{url}' failed: {message}")]
    Download { url: String, message: String },
}

pub type Result<T> = std::result::Result<T, FileError>;

// ---------- read functions ----------

/// Read the content of a file into a byte vector.
pub fn read_file<P: AsRef<Path>>(filename: P) -> Result<Vec<u8>> {
    let mut file = File::open(filename)?;
    let metadata = file.metadata()?;
    let mut output = Vec::with_capacity(metadata.len() as usize);
    file.read_to_end(&mut output)?;
    Ok(output)
}

// ---------- ZIP-related functions ----------

/// Extract the content of a ZIP archive into a byte vector.
/// If a filename is provided, only that file is extracted.
pub fn unzip_content(input: &[u8], filename: &str) -> Result<Vec<u8>> {
    let reader = io::Cursor::new(input);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut output = Vec::new();

    // Extract the files from the zip archive
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if !filename.is_empty() && file.name() != filename {
            continue;
        }

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        output.extend(buffer);
    }

    Ok(output)
}

/// Extract the first file ending in `.json` from a zip archive in memory,
/// skipping directory entries. Useful when the JSON filename inside a zip is
/// derived from the version string and not known ahead of time.
pub fn unzip_first_json(input: &[u8]) -> Result<Vec<u8>> {
    let reader = io::Cursor::new(input);
    let mut archive = zip::ZipArchive::new(reader)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if !file.is_dir() && file.name().ends_with(".json") {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            return Ok(buf);
        }
    }
    Err(FileError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        "no .json file found in zip archive",
    )))
}

/// Opens a ZIP file and extracts the content of a specific file.
pub fn read_zip_file<P: AsRef<Path>>(filename: P, name: &str) -> Result<Vec<u8>> {
    let input = read_file(filename)?;
    let output = unzip_content(&input, name)?;
    Ok(output)
}

/// Saves the content to a ZIP file.
pub fn write_zip_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    let path = Path::new(filename.as_ref());
    let mut zip_path = PathBuf::from(path);
    zip_path.set_extension(format!(
        "{}zip",
        path.extension()
            .map(|ext| format!("{}.", ext.to_string_lossy()))
            .unwrap_or_default()
    ));

    let zipfile = File::create(zip_path)?;
    let mut zip_writer = zip::ZipWriter::new(zipfile);

    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755)
        .last_modified_time(zip::DateTime::default_for_write());

    // Add file to the zip archive
    let basename = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid filename"))?
        .to_string_lossy();

    zip_writer.start_file(basename.to_string(), options)?;
    zip_writer.write_all(output)?;
    zip_writer.finish()?;

    Ok(())
}

/// Read each entry of a ZIP archive separately (in archive order), as
/// opposed to `unzip_content`, which concatenates every entry's bytes
/// together — not useful when entries are independently-encoded blobs (e.g.
/// each a separate zstd-compressed Parquet batch) rather than plain text.
pub fn read_zip_entries(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let reader = io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut entries = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        entries.push(buffer);
    }
    Ok(entries)
}

/// Read each entry of a gzip-compressed tar (`.tgz`) archive separately, in
/// archive order.
pub fn read_tar_gz_entries(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let decoder = flate2::read::GzDecoder::new(io::Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let mut entries = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let mut buffer = Vec::new();
        entry.read_to_end(&mut buffer)?;
        entries.push(buffer);
    }
    Ok(entries)
}

/// Saves multiple named entries into a single ZIP archive.
pub fn write_zip_archive<P: AsRef<Path>>(filename: P, entries: &[(String, Vec<u8>)]) -> Result<()> {
    let file = File::create(filename)?;
    let mut zip_writer = zip::ZipWriter::new(file);

    let options = zip::write::FileOptions::<()>::default()
        .compression_method(zip::CompressionMethod::Stored)
        .unix_permissions(0o755)
        .last_modified_time(zip::DateTime::default_for_write());

    for (name, content) in entries {
        zip_writer.start_file(name, options)?;
        zip_writer.write_all(content)?;
    }
    zip_writer.finish()?;

    Ok(())
}

/// Saves multiple named entries into a single gzip-compressed tar (.tgz) archive.
pub fn write_tar_gz_archive<P: AsRef<Path>>(
    filename: P,
    entries: &[(String, Vec<u8>)],
) -> Result<()> {
    let file = File::create(filename)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);

    for (name, content) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, content.as_slice())?;
    }

    let encoder = builder.into_inner()?;
    encoder.finish()?;

    Ok(())
}

/// Saves the content to a GZIP-compressed file.
pub fn write_gz_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    let path = Path::new(filename.as_ref());
    let mut gz_path = PathBuf::from(path);
    gz_path.set_extension(format!(
        "{}gz",
        path.extension()
            .map(|ext| format!("{}.", ext.to_string_lossy()))
            .unwrap_or_default()
    ));

    let file = File::create(gz_path)?;
    let mut encoder = GzEncoder::new(file, Compression::default());
    encoder.write_all(output)?;
    encoder.finish()?;

    Ok(())
}

// ---------- ZSTD-related functions ----------

/// Decompress a Zstandard-compressed byte buffer.
pub fn unzst_content(input: &[u8]) -> Result<Vec<u8>> {
    let output = zstd::stream::decode_all(io::Cursor::new(input))?;
    Ok(output)
}

/// Opens a ZSTD-compressed file and returns its decompressed content.
pub fn read_zst_file<P: AsRef<Path>>(filename: P) -> Result<Vec<u8>> {
    let input = read_file(filename)?;
    let output = unzst_content(&input)?;
    Ok(output)
}

/// Saves the content to a Zstandard-compressed file.
pub fn write_zst_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    let path = Path::new(filename.as_ref());
    let mut zst_path = PathBuf::from(path);
    zst_path.set_extension(format!(
        "{}zst",
        path.extension()
            .map(|ext| format!("{}.", ext.to_string_lossy()))
            .unwrap_or_default()
    ));

    let file = File::create(zst_path)?;
    let mut encoder = zstd::stream::Encoder::new(file, 0)?;
    encoder.write_all(output)?;
    encoder.finish()?;

    Ok(())
}

// ---------- network functions ----------

/// Download the content of a URL.
///
/// Data dumps can be multiple GB (e.g. the daily Crossref dump is ~2GB), so
/// the request timeout has to cover the entire download, not just a typical
/// API response: 60s was enough to fail on any connection slower than
/// ~35MB/s. `connect_timeout` stays short (server unreachable should fail
/// fast); `timeout` covers connect-through-body and is generous enough for
/// large, slow downloads while still bounding a truly stuck connection.
pub fn download_file(url: &str) -> Result<Vec<u8>> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(30 * 60))
        .build()
        .map_err(FileError::Http)?;

    let mut resp = client.get(url).send().map_err(|e| FileError::Download {
        url: url.to_string(),
        message: describe_reqwest_error(&e),
    })?;

    if !resp.status().is_success() {
        return Err(FileError::StatusCode {
            status: resp.status().as_u16(),
            text: resp.status().to_string(),
        });
    }

    let total_bytes = resp.content_length().unwrap_or(0);
    let bar = crate::progress::bytes_bar("downloading", total_bytes);

    let mut buffer = Vec::new();
    let mut writer = ProgressWriter {
        buffer: &mut buffer,
        bar: &bar,
    };
    let copy_result = resp.copy_to(&mut writer);
    bar.finish_and_clear();

    if let Err(e) = copy_result {
        return Err(FileError::Download {
            url: url.to_string(),
            message: format!(
                "{} (received {} bytes before the error)",
                describe_reqwest_error(&e),
                buffer.len()
            ),
        });
    }

    Ok(buffer)
}

/// Local cache directory for downloaded files, e.g.
/// `~/Library/Caches/commonmeta/{namespace}` on macOS,
/// `~/.cache/commonmeta/{namespace}` on Linux. Falls back to the system temp
/// dir if no cache dir is available.
pub fn cache_dir(namespace: &str) -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("commonmeta")
        .join(namespace)
}

/// Like [`download_file`], but checks a local cache first and populates it
/// on a miss. `namespace`/`cache_key` locate the cached file under
/// [`cache_dir`]; `ttl` is how long a cached copy stays valid before being
/// treated as a miss and re-downloaded. Returns `(bytes, true)` on a cache
/// hit, `(bytes, false)` after a fresh download.
///
/// Cache writes and the staleness sweep are best-effort: a read-only
/// filesystem or full disk degrades to always-download rather than failing
/// the caller, since the network request already succeeded by that point.
pub fn download_file_cached(
    url: &str,
    namespace: &str,
    cache_key: &str,
    ttl: Duration,
) -> Result<(Vec<u8>, bool)> {
    let path = cache_dir(namespace).join(cache_key);
    prune_cache(namespace, ttl);

    if let Some(bytes) = read_cache(&path, ttl) {
        return Ok((bytes, true));
    }

    let bytes = download_file(url)?;
    write_cache(&path, &bytes);
    Ok((bytes, false))
}

/// Return the cached bytes at `path` if it exists and is younger than `ttl`.
fn read_cache(path: &Path, ttl: Duration) -> Option<Vec<u8>> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    if SystemTime::now().duration_since(modified).ok()? > ttl {
        return None;
    }
    fs::read(path).ok()
}

fn write_cache(path: &Path, bytes: &[u8]) {
    // Write to a sibling .tmp file then rename atomically so a crash or kill
    // between create and write_all never leaves a partial file that looks valid.
    let Some(parent) = path.parent() else { return };
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("warning: failed to create cache dir '{}': {}", parent.display(), e);
        return;
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = fs::write(&tmp, bytes) {
        eprintln!("warning: failed to write cache '{}': {}", tmp.display(), e);
        fs::remove_file(&tmp).ok();
        return;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        eprintln!("warning: failed to rename cache '{}': {}", tmp.display(), e);
        fs::remove_file(&tmp).ok();
    }
}

/// Remove cached files older than `ttl` from `cache_dir(namespace)`.
/// Skips `.part` files — those belong to in-progress downloads and must not
/// be pruned independently of their final destination.
fn prune_cache(namespace: &str, ttl: Duration) {
    let Ok(entries) = fs::read_dir(cache_dir(namespace)) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().extension().is_some_and(|e| e == "part") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            > ttl
        {
            fs::remove_file(entry.path()).ok();
        }
    }
}

/// Forwards writes into `buffer` while incrementing `bar`, so
/// `Response::copy_to` (which already classifies errors via
/// `reqwest::Error`, unlike a manual `Read::read` loop) can report progress.
struct ProgressWriter<'a> {
    buffer: &'a mut Vec<u8>,
    bar: &'a indicatif::ProgressBar,
}

impl Write for ProgressWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        self.bar.inc(buf.len() as u64);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Stream-download `url` into a file at `path` with HTTP Range resumption and
/// automatic retry-on-failure. Suitable for files that are too large to hold
/// in RAM. Parent directories are created if needed.
///
/// The download is broken into 128 MiB chunks, each fetched as a separate
/// `Range: bytes=START-END` request. This bounds the per-request lifetime so
/// stalls are detected within the per-chunk timeout. TCP keepalive (30 s probe
/// interval) detects dead connections at the OS level; they surface as read
/// errors that trigger a retry from the current byte offset.
///
/// A `.part` sibling file stores in-progress bytes so a killed process can
/// resume from where it left off. Progress is written to stderr every 30 s
/// with wall-clock timestamps — suitable for tmux/screen/nohup logs.
///
/// Returns the total bytes written (including any bytes already in .part on
/// a resumed run).
pub fn download_file_to_path(url: &str, path: &Path) -> Result<u64> {
    download_to_path_resumable(url, path)
}

/// Like [`download_file_to_path`] but the cached copy is a file on disk rather
/// than a `Vec<u8>` in memory, making it suitable for very large downloads.
/// Returns `(path, was_cache_hit)`. The file at `path` is always valid on `Ok`.
///
/// A `.part` sibling preserves in-progress state across process restarts.
/// On a TTL miss both the stale file and any `.part` are deleted so the next
/// download starts fresh (rather than resuming a possibly-stale partial file).
pub fn ensure_cached_path(
    url: &str,
    namespace: &str,
    cache_key: &str,
    ttl: Duration,
) -> Result<(PathBuf, bool)> {
    let path = cache_dir(namespace).join(cache_key);
    prune_cache(namespace, ttl);

    if let Ok(meta) = fs::metadata(&path) {
        if let Ok(modified) = meta.modified() {
            if SystemTime::now()
                .duration_since(modified)
                .unwrap_or(ttl + Duration::from_secs(1))
                <= ttl
            {
                return Ok((path, true));
            }
        }
        // File exists but is stale: remove it and any in-progress .part so the
        // next download starts clean rather than resuming outdated bytes.
        fs::remove_file(&path).ok();
        fs::remove_file(&part_path(&path)).ok();
    }

    download_to_path_resumable(url, &path)?;
    Ok((path, false))
}

// ---------- resumable downloader ----------

const CHUNK: u64 = 128 * 1024 * 1024; // 128 MiB per HTTP Range request
const PARALLEL_TRANSFERS: usize = 4; // rclone --transfers
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60); // rclone --contimeout
const CHUNK_TIMEOUT: Duration = Duration::from_secs(5 * 60); // rclone --timeout (idle/stall)
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30); // TCP keepalive probe interval
const KEEPALIVE_RETRIES: u32 = 5; // probes before the OS declares the connection dead
const PROGRESS_INTERVAL: Duration = Duration::from_secs(30); // rclone --stats
const MAX_RETRIES: u32 = 20; // rclone --low-level-retries
const READ_BUF: usize = 256 * 1024; // 256 KiB read buffer

fn download_to_path_resumable(url: &str, dest: &Path) -> Result<u64> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let part = part_path(dest);

    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .tcp_keepalive(KEEPALIVE_INTERVAL)
        .tcp_keepalive_interval(KEEPALIVE_INTERVAL)
        .tcp_keepalive_retries(KEEPALIVE_RETRIES)
        .build()
        .map_err(FileError::Http)?;

    let (total, supports_range) = head_content_length(&client, url);

    // Fast path: server supports Range + known Content-Length → parallel multi-connection.
    // Any previous .part is discarded because we can't verify which parallel chunks
    // completed; starting fresh is always correct.
    if supports_range {
        if let Some(t) = total {
            fs::remove_file(&part).ok();
            return download_parallel(&client, url, dest, &part, t);
        }
    }

    // Sequential fallback: no Range support or unknown Content-Length.
    let mut offset: u64 = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);

    if !supports_range {
        eprintln!("download: server does not support Range requests — streaming without resume");
        offset = 0;
        fs::remove_file(&part).ok();
    }

    // If the .part file already covers the full content (interrupted rename),
    // just finish the rename.
    if let Some(t) = total {
        if offset >= t && offset > 0 {
            fs::rename(&part, dest)?;
            return Ok(offset);
        }
    }

    if offset > 0 {
        let of = total
            .map(|t| format!(" / {} ({:.1}%)", fmt_bytes(t), offset as f64 / t as f64 * 100.0))
            .unwrap_or_default();
        eprintln!("download: resuming at {}{}", fmt_bytes(offset), of);
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .open(&part)?;
    file.seek(io::SeekFrom::Start(offset))?;

    let overall_start = Instant::now();
    let mut progress_mark = Instant::now();
    let mut progress_bytes: u64 = 0;
    let mut retries: u32 = 0;

    'outer: loop {
        if let Some(t) = total {
            if offset >= t {
                break;
            }
        }

        let mut req = client.get(url).timeout(CHUNK_TIMEOUT);
        if supports_range {
            let end = match total {
                Some(t) => (offset + CHUNK - 1).min(t - 1),
                None => offset + CHUNK - 1,
            };
            req = req.header("Range", format!("bytes={offset}-{end}"));
        }
        let resp = req.send();

        let mut resp = match resp {
            Ok(r) => r,
            Err(e) => {
                retries += 1;
                if retries > MAX_RETRIES {
                    return Err(FileError::Download {
                        url: url.to_string(),
                        message: format!(
                            "too many errors after {}; last: {}",
                            fmt_bytes(offset),
                            describe_reqwest_error(&e)
                        ),
                    });
                }
                let wait = retry_backoff(retries - 1);
                eprintln!(
                    "download: connect failed ({}) — retry {}/{} in {}",
                    describe_reqwest_error(&e),
                    retries,
                    MAX_RETRIES,
                    fmt_duration_short(wait)
                );
                std::thread::sleep(wait);
                continue;
            }
        };

        let status = resp.status();

        if status.as_u16() == 416 {
            // Range Not Satisfiable — we are already at or past EOF.
            break;
        }

        if !status.is_success() {
            return Err(FileError::StatusCode {
                status: status.as_u16(),
                text: status.to_string(),
            });
        }

        // When we sent a Range header but got 200, the server ignored it.
        // We can't use the partial .part bytes — restart from 0 and remember
        // that this server doesn't support Range for future chunks.
        if supports_range && status.as_u16() == 200 && offset > 0 {
            eprintln!("download: server ignores Range header, restarting from 0");
            offset = 0;
            file.seek(io::SeekFrom::Start(0))?;
            file.set_len(0)?;
        }

        // For non-Range requests the full body is one chunk; use content-length
        // as the bound (or u64::MAX to stream until EOF).
        let mut chunk_remaining: u64 = if supports_range {
            let end = match total {
                Some(t) => (offset + CHUNK - 1).min(t - 1),
                None => offset + CHUNK - 1,
            };
            end - offset + 1
        } else {
            total.unwrap_or(u64::MAX)
        };
        let mut buf = vec![0u8; READ_BUF];

        loop {
            let n = match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    retries += 1;
                    let detail = error_chain(&e);
                    if retries > MAX_RETRIES {
                        return Err(FileError::Download {
                            url: url.to_string(),
                            message: format!(
                                "read error after {}: {}",
                                fmt_bytes(offset),
                                detail
                            ),
                        });
                    }
                    let wait = retry_backoff(retries - 1);
                    eprintln!(
                        "download: read error at {} ({}) — retry {}/{} in {}",
                        fmt_bytes(offset),
                        detail,
                        retries,
                        MAX_RETRIES,
                        fmt_duration_short(wait)
                    );
                    std::thread::sleep(wait);
                    continue 'outer;
                }
            };

            file.write_all(&buf[..n])?;
            let n64 = n as u64;
            offset += n64;
            progress_bytes += n64;
            chunk_remaining = chunk_remaining.saturating_sub(n64);
            retries = 0;

            if progress_mark.elapsed() >= PROGRESS_INTERVAL {
                let speed = progress_bytes as f64 / progress_mark.elapsed().as_secs_f64();
                let of_total = total
                    .map(|t| format!(" / {}", fmt_bytes(t)))
                    .unwrap_or_default();
                let pct = total
                    .filter(|&t| t > 0)
                    .map(|t| format!(" ({:.1}%)", offset as f64 / t as f64 * 100.0))
                    .unwrap_or_default();
                let eta = total
                    .filter(|&t| t > offset && speed > 0.0)
                    .map(|t| {
                        let secs = (t - offset) as f64 / speed;
                        format!(", ETA {}", fmt_duration_short(Duration::from_secs_f64(secs)))
                    })
                    .unwrap_or_default();
                let ts = chrono::Local::now().format("%H:%M:%S");
                eprintln!(
                    "[{ts}] download: {}{}{} @ {}/s elapsed {}{}",
                    fmt_bytes(offset),
                    of_total,
                    pct,
                    fmt_bytes(speed as u64),
                    fmt_duration_short(overall_start.elapsed()),
                    eta,
                );
                progress_mark = Instant::now();
                progress_bytes = 0;
            }

            if chunk_remaining == 0 {
                break; // fetch next chunk
            }
        }
    }

    file.flush()?;
    drop(file);

    let ts = chrono::Local::now().format("%H:%M:%S");
    let final_bytes = total.unwrap_or(offset);
    eprintln!(
        "[{ts}] download: complete — {} in {}",
        fmt_bytes(final_bytes),
        fmt_duration_short(overall_start.elapsed())
    );

    fs::rename(&part, dest)?;
    Ok(final_bytes)
}

/// Parallel multi-connection download. Pre-allocates `dest.part`, splits the
/// file into 128 MiB chunks, and fetches them with PARALLEL_TRANSFERS workers.
/// Each worker has its own `Client` clone (shared pool) and `File` handle;
/// writes to non-overlapping byte ranges are safe without locking.
/// A failed chunk is retried from its start offset — writes are idempotent since
/// the target region in the pre-allocated file is simply overwritten.
fn download_parallel(
    client: &Client,
    url: &str,
    dest: &Path,
    part: &Path,
    total: u64,
) -> Result<u64> {
    // Pre-allocate so every worker can seek-and-write without extending the file.
    {
        let f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(part)?;
        f.set_len(total)?;
    }

    // Build the chunk work queue.
    let mut queue: VecDeque<(u64, u64)> = VecDeque::new(); // (start, end) inclusive
    let mut off = 0u64;
    while off < total {
        let end = (off + CHUNK - 1).min(total - 1);
        queue.push_back((off, end));
        off = end + 1;
    }
    let n_chunks = queue.len();
    let n_workers = PARALLEL_TRANSFERS.min(n_chunks);

    eprintln!(
        "download: {} / {} chunks, {} parallel connections",
        n_chunks,
        fmt_bytes(total),
        n_workers,
    );

    let queue = Arc::new(Mutex::new(queue));
    let abort = Arc::new(AtomicBool::new(false));
    let (prog_tx, prog_rx) = mpsc::channel::<u64>(); // bytes written per write call

    let url_arc = Arc::new(url.to_string());
    let part_arc = Arc::new(part.to_path_buf());

    let mut handles: Vec<std::thread::JoinHandle<Result<()>>> =
        Vec::with_capacity(n_workers);

    for _ in 0..n_workers {
        let queue = Arc::clone(&queue);
        let abort = Arc::clone(&abort);
        let prog_tx = prog_tx.clone();
        let url = Arc::clone(&url_arc);
        let part = Arc::clone(&part_arc);
        let client = client.clone(); // shares the connection pool

        handles.push(std::thread::spawn(move || -> Result<()> {
            let mut file = fs::OpenOptions::new().write(true).open(part.as_ref())?;
            let mut buf = vec![0u8; READ_BUF];

            loop {
                if abort.load(Ordering::Relaxed) {
                    break;
                }
                let chunk = { queue.lock().unwrap().pop_front() };
                let (start, end) = match chunk {
                    None => break,
                    Some(c) => c,
                };

                let mut retries = 0u32;
                'chunk: loop {
                    if abort.load(Ordering::Relaxed) {
                        return Ok(());
                    }

                    let resp = client
                        .get(url.as_str())
                        .timeout(CHUNK_TIMEOUT)
                        .header("Range", format!("bytes={start}-{end}"))
                        .send();

                    let mut resp = match resp {
                        Ok(r) if r.status().as_u16() == 206 => r,
                        Ok(r) => {
                            abort.store(true, Ordering::Relaxed);
                            return Err(FileError::StatusCode {
                                status: r.status().as_u16(),
                                text: r.status().to_string(),
                            });
                        }
                        Err(e) => {
                            retries += 1;
                            if retries > MAX_RETRIES {
                                abort.store(true, Ordering::Relaxed);
                                return Err(FileError::Download {
                                    url: url.to_string(),
                                    message: format!(
                                        "chunk @{}: too many connect errors: {}",
                                        fmt_bytes(start),
                                        error_chain(&e)
                                    ),
                                });
                            }
                            let wait = retry_backoff(retries - 1);
                            eprintln!(
                                "download: chunk @{} connect error ({}) — retry {}/{} in {}",
                                fmt_bytes(start),
                                error_chain(&e),
                                retries,
                                MAX_RETRIES,
                                fmt_duration_short(wait),
                            );
                            std::thread::sleep(wait);
                            continue 'chunk;
                        }
                    };

                    file.seek(io::SeekFrom::Start(start))?;

                    loop {
                        match resp.read(&mut buf) {
                            Ok(0) => break 'chunk, // chunk complete, move to next
                            Ok(n) => {
                                file.write_all(&buf[..n])?;
                                prog_tx.send(n as u64).ok();
                            }
                            Err(e) => {
                                retries += 1;
                                let detail = error_chain(&e);
                                if retries > MAX_RETRIES {
                                    abort.store(true, Ordering::Relaxed);
                                    return Err(FileError::Download {
                                        url: url.to_string(),
                                        message: format!(
                                            "chunk @{}: too many read errors: {}",
                                            fmt_bytes(start),
                                            detail
                                        ),
                                    });
                                }
                                let wait = retry_backoff(retries - 1);
                                eprintln!(
                                    "download: read error in chunk @{} ({}) — retry {}/{} in {}",
                                    fmt_bytes(start),
                                    detail,
                                    retries,
                                    MAX_RETRIES,
                                    fmt_duration_short(wait),
                                );
                                std::thread::sleep(wait);
                                continue 'chunk; // re-request entire chunk from start
                            }
                        }
                    }
                }
            }
            Ok(())
        }));
    }
    drop(prog_tx); // close our sender so the channel closes when all workers finish

    // Aggregate progress on the main thread while workers run.
    let overall_start = Instant::now();
    let mut progress_mark = Instant::now();
    let mut progress_bytes = 0u64;
    let mut total_written = 0u64;

    for bytes in &prog_rx {
        total_written += bytes;
        progress_bytes += bytes;

        if progress_mark.elapsed() >= PROGRESS_INTERVAL {
            let speed = progress_bytes as f64 / progress_mark.elapsed().as_secs_f64();
            let pct = total_written as f64 / total as f64 * 100.0;
            let eta = if speed > 0.0 && total_written < total {
                format!(
                    ", ETA {}",
                    fmt_duration_short(Duration::from_secs_f64(
                        (total - total_written) as f64 / speed
                    ))
                )
            } else {
                String::new()
            };
            let ts = chrono::Local::now().format("%H:%M:%S");
            eprintln!(
                "[{ts}] download: {} / {} ({:.1}%) @ {}/s elapsed {}{}",
                fmt_bytes(total_written),
                fmt_bytes(total),
                pct,
                fmt_bytes(speed as u64),
                fmt_duration_short(overall_start.elapsed()),
                eta,
            );
            progress_mark = Instant::now();
            progress_bytes = 0;
        }
    }

    // Join all workers and collect the first error (if any).
    let mut first_err: Option<FileError> = None;
    for h in handles {
        match h.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(_) => {
                if first_err.is_none() {
                    first_err = Some(FileError::Download {
                        url: url.to_string(),
                        message: "worker thread panicked".to_string(),
                    });
                }
            }
        }
    }

    if let Some(e) = first_err {
        fs::remove_file(part).ok();
        return Err(e);
    }

    let ts = chrono::Local::now().format("%H:%M:%S");
    eprintln!(
        "[{ts}] download: complete — {} in {} ({} parallel connections)",
        fmt_bytes(total),
        fmt_duration_short(overall_start.elapsed()),
        n_workers,
    );

    fs::rename(part, dest)?;
    Ok(total)
}

/// `dest` + `.part` suffix, used as the in-progress scratch file.
fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// GET Content-Length via HEAD without downloading the body.
/// Returns `(content_length, supports_range)`.
/// `supports_range` is true when the server sends `Accept-Ranges: bytes`.
/// When false, chunked Range requests won't work — stream as a single request.
fn head_content_length(client: &Client, url: &str) -> (Option<u64>, bool) {
    let resp = match client
        .head(url)
        .timeout(Duration::from_secs(30))
        .send()
        .ok()
    {
        Some(r) => r,
        None => return (None, true), // assume range support; will retry on failure
    };
    let length = resp.content_length();
    let supports_range = resp
        .headers()
        .get("accept-ranges")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);
    (length, supports_range)
}

/// Human-readable byte count: "304.25 GiB", "45.00 MiB", etc.
fn fmt_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = n as f64;
    let mut unit = 0usize;
    while val >= 1024.0 && unit + 1 < UNITS.len() {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{val:.2} {}", UNITS[unit])
    }
}

/// Human-readable duration: "3h 47m", "12m 30s", "45s".
fn fmt_duration_short(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

/// Walk the `std::error::Error::source()` chain and return a string that
/// includes every level, colon-separated. Reqwest wraps the root OS error
/// (e.g. "connection reset by peer (os error 104)") several levels deep
/// behind a generic "request or response body error" Display — this makes
/// the actual cause visible in log output.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut msg = e.to_string();
    let mut src = e.source();
    while let Some(cause) = src {
        let cause_str = cause.to_string();
        if cause_str != msg {
            msg.push_str(": ");
            msg.push_str(&cause_str);
        }
        src = cause.source();
    }
    msg
}

/// Exponential back-off with 300 s cap: 10 s → 30 s → 90 s → 270 s → 300 s.
fn retry_backoff(attempt: u32) -> Duration {
    Duration::from_secs(10u64.saturating_mul(3u64.pow(attempt.min(4))).min(300))
}

/// Stream-decompress the zstd file at `src` into `dest`, creating `dest`
/// (and its parents) if needed. Returns the number of decompressed bytes.
pub fn decompress_zst_file(src: &Path, dest: &Path) -> Result<u64> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let src_file = File::open(src)?;
    let mut decoder = zstd::Decoder::new(src_file)?;
    let mut dest_file = File::create(dest)?;
    let bytes = io::copy(&mut decoder, &mut dest_file)?;
    Ok(bytes)
}

/// Classify a `reqwest::Error` into a more actionable message than its
/// `Display` impl alone, which for body/decode failures is just the opaque
/// "error decoding response body" regardless of root cause.
fn describe_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("the request timed out: {e}")
    } else if e.is_connect() {
        format!("could not connect to the server: {e}")
    } else if e.is_body() || e.is_decode() {
        format!("the connection was interrupted before the download finished: {e}")
    } else {
        e.to_string()
    }
}

// ---------- write functions ----------

/// Saves the content to a file.
pub fn write_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    // Create parent directories if they don't exist
    if let Some(parent) = filename.as_ref().parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
    }

    let mut file = File::create(filename)?;
    file.write_all(output)?;
    Ok(())
}

// ---------- helper functions ----------

pub fn get_extension<P: AsRef<Path>>(filename: P, ext: &str) -> (PathBuf, String, String) {
    let path = PathBuf::from(filename.as_ref());

    if path != PathBuf::new() {
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_default();

        let compress = if extension == "zip"
            || extension == "gz"
            || extension == "zst"
            || extension == "tgz"
        {
            // Remove trailing compression extension (".zip"/".gz"/".zst"/".tgz") from filename.
            let stem = path.file_stem().unwrap_or_default();
            let parent = path.parent().unwrap_or_else(|| Path::new(""));
            let new_path = parent.join(stem);

            let new_extension = new_path
                .extension()
                .map(|ext| ext.to_string_lossy().to_string())
                .unwrap_or_default();

            let formatted_ext = if new_extension.is_empty() {
                "".to_string()
            } else {
                format!(".{}", new_extension)
            };

            (new_path, formatted_ext, extension)
        } else {
            let formatted_ext = if extension.is_empty() {
                "".to_string()
            } else {
                format!(".{}", extension)
            };

            (path, formatted_ext, String::new())
        };

        return compress;
    }

    let extension = if ext.is_empty() {
        ".json".to_string()
    } else if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{}", ext)
    };

    (path, extension, String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_file_connect_error_is_classified() {
        // Nothing listens on port 1, so this fails fast with a connection
        // error rather than hanging for the full 30-minute request timeout.
        let err = download_file("http://127.0.0.1:1/").unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("could not connect to the server"),
            "unexpected message: {message}"
        );
    }

    /// Read (and discard) an incoming HTTP request up through the blank
    /// line ending its headers. If the client's bytes are still unread in
    /// the kernel's receive buffer when we close our end of the socket, the
    /// OS can send a TCP RST instead of a clean FIN, which reqwest surfaces
    /// as a body/decode error indistinguishable from a real interrupted
    /// download — draining the request first avoids that race.
    fn drain_http_request(stream: &std::net::TcpStream) {
        use std::io::{BufRead, BufReader};
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) if line == "\r\n" || line == "\n" => break,
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    fn respond_once(
        listener: std::net::TcpListener,
        body: &'static [u8],
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            use std::io::Write;
            if let Ok((mut stream, _)) = listener.accept() {
                drain_http_request(&stream);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        })
    }

    #[test]
    fn test_download_file_cached_hits_cache_without_network() {
        let namespace = "test-cache-hit";
        let cache_key = "file.txt";
        std::fs::remove_dir_all(cache_dir(namespace)).ok();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = respond_once(listener, b"hello from network");

        let (bytes, from_cache) = download_file_cached(
            &format!("http://{addr}/file"),
            namespace,
            cache_key,
            Duration::from_secs(3600),
        )
        .unwrap();
        assert_eq!(bytes, b"hello from network");
        assert!(!from_cache);
        handle.join().unwrap();

        // Second call must not touch the network: nothing is listening on
        // this address anymore, so a cache miss here would error out.
        let (bytes, from_cache) = download_file_cached(
            &format!("http://{addr}/file"),
            namespace,
            cache_key,
            Duration::from_secs(3600),
        )
        .unwrap();
        assert_eq!(bytes, b"hello from network");
        assert!(from_cache);

        std::fs::remove_dir_all(cache_dir(namespace)).ok();
    }

    #[test]
    fn test_download_file_cached_expired_entry_redownloads() {
        let namespace = "test-cache-expired";
        let cache_key = "file.txt";
        std::fs::remove_dir_all(cache_dir(namespace)).ok();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = respond_once(listener, b"first download");
        let ttl = Duration::from_millis(50);
        let (bytes, from_cache) =
            download_file_cached(&format!("http://{addr}/file"), namespace, cache_key, ttl)
                .unwrap();
        assert_eq!(bytes, b"first download");
        assert!(!from_cache);
        handle.join().unwrap();

        std::thread::sleep(Duration::from_millis(100));

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = respond_once(listener, b"second download");
        let (bytes, from_cache) =
            download_file_cached(&format!("http://{addr}/file"), namespace, cache_key, ttl)
                .unwrap();
        assert_eq!(bytes, b"second download");
        assert!(
            !from_cache,
            "expired cache entry should trigger a fresh download"
        );
        handle.join().unwrap();

        std::fs::remove_dir_all(cache_dir(namespace)).ok();
    }

    #[test]
    fn test_download_file_status_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            use std::io::Write;
            if let Ok((mut stream, _)) = listener.accept() {
                drain_http_request(&stream);
                let _ = stream.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });

        let err = download_file(&format!("http://{addr}/missing")).unwrap_err();
        match err {
            FileError::StatusCode { status, .. } => assert_eq!(status, 404),
            other => panic!("expected StatusCode error, got: {other}"),
        }

        handle.join().unwrap();
    }

    #[test]
    fn test_download_file_body_interrupted_reports_partial_bytes() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            use std::io::Write;
            if let Ok((mut stream, _)) = listener.accept() {
                drain_http_request(&stream);
                // Announce a body far larger than what's actually sent, then
                // close the connection early to simulate an interrupted
                // download (the failure mode behind the original bug).
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\nshort body",
                );
            }
        });

        let err = download_file(&format!("http://{addr}/big")).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("interrupted") && message.contains("bytes before the error"),
            "unexpected message: {message}"
        );

        handle.join().unwrap();
    }

    #[test]
    fn test_zst_roundtrip_content() {
        let original = b"hello zstd world, hello zstd world, hello zstd world";
        let compressed = zstd::stream::encode_all(io::Cursor::new(original), 0).unwrap();
        let decompressed = unzst_content(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_write_and_read_zst_file() {
        let dir = std::env::temp_dir().join("commonmeta_zst_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.json");

        let original = b"{\"hello\":\"world\"}";
        write_zst_file(&path, original).unwrap();

        let zst_path = dir.join("data.json.zst");
        assert!(zst_path.exists());

        let roundtrip = read_zst_file(&zst_path).unwrap();
        assert_eq!(roundtrip, original);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_get_extension_zst() {
        let (path, ext, compress) = get_extension("data.json.zst", ".json");
        assert_eq!(path, PathBuf::from("data.json"));
        assert_eq!(ext, ".json");
        assert_eq!(compress, "zst");
    }

    #[test]
    fn test_get_extension_tgz() {
        let (path, ext, compress) = get_extension("data.json.tgz", ".json");
        assert_eq!(path, PathBuf::from("data.json"));
        assert_eq!(ext, ".json");
        assert_eq!(compress, "tgz");
    }

    #[test]
    fn test_write_zip_archive_multi_entry() {
        let dir = std::env::temp_dir().join("commonmeta_zip_archive_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.zip");

        let entries = vec![
            ("a.json".to_string(), b"{\"a\":1}".to_vec()),
            ("b.json".to_string(), b"{\"b\":2}".to_vec()),
        ];
        write_zip_archive(&path, &entries).unwrap();

        let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(archive.len(), 2);
        let mut contents = String::new();
        archive
            .by_name("a.json")
            .unwrap()
            .read_to_string(&mut contents)
            .unwrap();
        assert_eq!(contents, "{\"a\":1}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_write_tar_gz_archive_multi_entry() {
        let dir = std::env::temp_dir().join("commonmeta_tgz_archive_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.tgz");

        let entries = vec![
            ("a.json".to_string(), b"{\"a\":1}".to_vec()),
            ("b.json".to_string(), b"{\"b\":2}".to_vec()),
        ];
        write_tar_gz_archive(&path, &entries).unwrap();

        let decoder = flate2::read::GzDecoder::new(File::open(&path).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let names: Vec<String> = archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a.json", "b.json"]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_zip_entries_returns_each_entry_separately() {
        let dir = std::env::temp_dir().join("commonmeta_read_zip_entries_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.zip");

        let entries = vec![
            ("a.bin".to_string(), b"first-blob".to_vec()),
            ("b.bin".to_string(), b"second-blob".to_vec()),
        ];
        write_zip_archive(&path, &entries).unwrap();

        let bytes = read_file(&path).unwrap();
        let read_back = read_zip_entries(&bytes).unwrap();
        assert_eq!(
            read_back,
            vec![b"first-blob".to_vec(), b"second-blob".to_vec()]
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_read_tar_gz_entries_returns_each_entry_separately() {
        let dir = std::env::temp_dir().join("commonmeta_read_tgz_entries_test");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("out.tgz");

        let entries = vec![
            ("a.bin".to_string(), b"first-blob".to_vec()),
            ("b.bin".to_string(), b"second-blob".to_vec()),
        ];
        write_tar_gz_archive(&path, &entries).unwrap();

        let bytes = read_file(&path).unwrap();
        let read_back = read_tar_gz_entries(&bytes).unwrap();
        assert_eq!(
            read_back,
            vec![b"first-blob".to_vec(), b"second-blob".to_vec()]
        );

        fs::remove_dir_all(&dir).ok();
    }
}
