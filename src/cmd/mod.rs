pub mod convert;
pub mod decode;
pub mod dump;
pub mod encode;
pub mod import;
pub mod install;
pub mod list;
pub mod r#match;
pub mod push;
pub mod put;

pub const PIDBOX_URL: &str = "https://metadata.vraix.org/pidbox.sqlite3.zst";
pub const PIDBOX_CACHE_KEY: &str = "pidbox.sqlite3.zst";
pub const VRAIX_CACHE_TTL: std::time::Duration =
    std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// Resolve the path to the local commonmeta works SQLite database.
///
/// Precedence (highest first):
///   1. `explicit` — the value of a `--file` CLI flag when provided
///   2. `COMMONMETA_DB` environment variable
///   3. Platform default:
///      - macOS  → `~/Library/Application Support/commonmeta/commonmeta.sqlite3`
///      - Linux  → `/var/lib/commonmeta/commonmeta.sqlite3`
///      - other  → `./commonmeta.sqlite3`
pub fn resolve_db_path(explicit: Option<&String>) -> String {
    if let Some(p) = explicit {
        return p.clone();
    }
    if let Ok(p) = std::env::var("COMMONMETA_DB") {
        return p;
    }
    platform_default_db_path()
}

fn platform_default_db_path() -> String {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        format!(
            "{}/Library/Application Support/commonmeta/commonmeta.sqlite3",
            home
        )
    }
    #[cfg(target_os = "linux")]
    {
        "/var/lib/commonmeta/commonmeta.sqlite3".to_string()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        "commonmeta.sqlite3".to_string()
    }
}

