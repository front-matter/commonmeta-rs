use flate2::Compression;
use flate2::write::GzEncoder;
use reqwest::blocking::Client;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
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
pub fn write_tar_gz_archive<P: AsRef<Path>>(filename: P, entries: &[(String, Vec<u8>)]) -> Result<()> {
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
    let mut writer = ProgressWriter { buffer: &mut buffer, bar: &bar };
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
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("commonmeta").join(namespace)
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
    if let Err(e) = write_file(path, bytes) {
        eprintln!("warning: failed to cache '{}': {}", path.display(), e);
    }
}

/// Remove cached files older than `ttl` from `cache_dir(namespace)`.
fn prune_cache(namespace: &str, ttl: Duration) {
    let Ok(entries) = fs::read_dir(cache_dir(namespace)) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        let Ok(modified) = metadata.modified() else { continue };
        if SystemTime::now().duration_since(modified).unwrap_or_default() > ttl {
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

/// Classify a `reqwest::Error` into a more actionable message than its
/// `Display` impl alone, which for body/decode failures is just the opaque
/// "error decoding response body" regardless of root cause.
fn describe_reqwest_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        format!("the request timed out: {e}")
    } else if e.is_connect() {
        format!("could not connect to the server: {e}")
    } else if e.is_body() || e.is_decode() {
        format!(
            "the connection was interrupted before the download finished: {e}"
        )
    } else {
        e.to_string()
    }
}

// ---------- write functions ----------

/// Saves the content to a file.
pub fn write_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    // Create parent directories if they don't exist
    if let Some(parent) = filename.as_ref().parent()
        && !parent.exists() {
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

        let compress = if extension == "zip" || extension == "gz" || extension == "zst" || extension == "tgz" {
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

    fn respond_once(listener: std::net::TcpListener, body: &'static [u8]) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            use std::io::Write;
            if let Ok((mut stream, _)) = listener.accept() {
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
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
            download_file_cached(&format!("http://{addr}/file"), namespace, cache_key, ttl).unwrap();
        assert_eq!(bytes, b"first download");
        assert!(!from_cache);
        handle.join().unwrap();

        std::thread::sleep(Duration::from_millis(100));

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = respond_once(listener, b"second download");
        let (bytes, from_cache) =
            download_file_cached(&format!("http://{addr}/file"), namespace, cache_key, ttl).unwrap();
        assert_eq!(bytes, b"second download");
        assert!(!from_cache, "expired cache entry should trigger a fresh download");
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
                let _ = stream
                    .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
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
        archive.by_name("a.json").unwrap().read_to_string(&mut contents).unwrap();
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
}
