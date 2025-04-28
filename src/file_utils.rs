use reqwest::blocking::Client;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
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
    zip_path.set_extension("zip");

    let zipfile = File::create(zip_path)?;
    let mut zip_writer = zip::ZipWriter::new(zipfile);

    let options = zip::write::FileOptions::default()
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

// ---------- network functions ----------

/// download content of a URL.
pub fn download_file(url: &str) -> Result<Vec<u8>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(FileError::Http)?;

    let resp = client.get(url).send()?;

    if !resp.status().is_success() {
        return Err(FileError::StatusCode {
            status: resp.status().as_u16(),
            text: resp.status().to_string(),
        });
    }

    Ok(resp.bytes()?.to_vec())
}

// ---------- write functions ----------

/// Saves the content to a file.
pub fn write_file<P: AsRef<Path>>(filename: P, output: &[u8]) -> Result<()> {
    // Create parent directories if they don't exist
    if let Some(parent) = filename.as_ref().parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let mut file = File::create(filename)?;
    file.write_all(output)?;
    Ok(())
}

// ---------- helper functions ----------

pub fn get_extension<P: AsRef<Path>>(filename: P, ext: &str) -> (PathBuf, String, bool) {
    let path = PathBuf::from(filename.as_ref());

    if path != PathBuf::new() {
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_default();

        let compress = if extension == "zip" {
            // Remove the ".zip" extension from the filename
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

            (new_path, formatted_ext, true)
        } else {
            let formatted_ext = if extension.is_empty() {
                "".to_string()
            } else {
                format!(".{}", extension)
            };

            (path, formatted_ext, false)
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

    (path, extension, false)
}
