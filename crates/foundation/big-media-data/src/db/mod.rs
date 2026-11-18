//! SQLite database layer: schema migrations, track/playlist/podcast CRUD,
//! and advanced search/filter queries.

pub mod fts;
pub mod models;
pub mod queries;
pub mod queries_advanced;
pub mod schema;
/// Explicit, non-destructive review of unavailable media references.
pub mod unavailable;

use log::info;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub type DbHandle = Arc<Mutex<Connection>>;

/// Collect query rows, skipping (and logging) rows that fail to decode: one
/// damaged row should not hide the rest of the library, nor vanish silently.
pub(crate) fn decoded_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Vec<T> {
    rows.filter_map(|row| {
        row.map_err(|error| log::warn!("skipping unreadable database row: {error}"))
            .ok()
    })
    .collect()
}

pub fn open_database() -> crate::Result<DbHandle> {
    let db_path = db_path()?;
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    info!("Opening database at {}", db_path.display());
    let conn = Connection::open(&db_path)?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA foreign_keys = ON;
         PRAGMA mmap_size = 67108864;
         PRAGMA cache_size = -16000;
         PRAGMA busy_timeout = 5000;",
    )?;

    schema::run_migrations(&conn)?;

    if let Err(error) = queries::migrate_radio_from_json(&conn) {
        log::warn!("radio stations from the old JSON file were not imported: {error}");
    }

    Ok(Arc::new(Mutex::new(conn)))
}

/// Read-only WAL connection; concurrent reads don't block the writer.
pub fn open_read_connection() -> crate::Result<DbHandle> {
    let db_path = db_path()?;
    let conn = Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA mmap_size = 67108864;
         PRAGMA cache_size = -8000;
         PRAGMA busy_timeout = 5000;",
    )?;

    Ok(Arc::new(Mutex::new(conn)))
}

/// Default app-id used when the `BIG_MEDIA_DB_APP_ID` env var is unset.
///
/// big-audio-player stays the default so its existing database keeps
/// opening. Other apps (big-video-player, etc) must `export BIG_MEDIA_DB_APP_ID=...`
/// before calling `open_database`, or use [`db_path_for`] directly.
const DEFAULT_APP_ID: &str = "big-audio-player";

/// Resolve `$XDG_CONFIG_HOME/<app_id>/library.db`.
///
/// # Errors
/// Returns an error when neither `XDG_CONFIG_HOME` nor `HOME` is an absolute
/// path; the library never falls back to the working directory.
pub fn db_path_for(app_id: &str) -> crate::Result<PathBuf> {
    library_path(big_os_kit::xdg_dirs::config_dir(), app_id)
}

fn library_path(config_dir: Option<PathBuf>, app_id: &str) -> crate::Result<PathBuf> {
    let config_dir = config_dir.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no configuration directory: XDG_CONFIG_HOME and HOME are unset or relative",
        )
    })?;
    Ok(config_dir.join(app_id).join("library.db"))
}

/// Resolve the library database path using the `BIG_MEDIA_DB_APP_ID`
/// env var, falling back to [`DEFAULT_APP_ID`] when unset / empty.
///
/// Reads the env var at every call so test harnesses can swap it via
/// `std::env::set_var` without restarting the process.
fn db_path() -> crate::Result<PathBuf> {
    let app_id = std::env::var("BIG_MEDIA_DB_APP_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_APP_ID.to_string());
    db_path_for(&app_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_path_needs_config_dir() {
        assert!(library_path(None, DEFAULT_APP_ID).is_err());
        assert_eq!(
            library_path(Some(PathBuf::from("/home/u/.config")), "app").unwrap(),
            PathBuf::from("/home/u/.config/app/library.db")
        );
    }
}
