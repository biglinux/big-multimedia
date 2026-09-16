//! SQLite database layer — schema migrations, track/playlist/podcast CRUD,
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

pub fn open_database() -> crate::Result<DbHandle> {
    let db_path = db_path();
    if let Some(parent) = db_path.parent() {
        let _ = std::fs::create_dir_all(parent);
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

    let _ = queries::migrate_radio_from_json(&conn);

    Ok(Arc::new(Mutex::new(conn)))
}

/// Read-only WAL connection — concurrent reads don't block the writer.
pub fn open_read_connection() -> crate::Result<DbHandle> {
    let db_path = db_path();
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
/// caveman: keep big-audio-player as default for backwards-compat. New
/// apps (big-video-player, etc) must `export BIG_MEDIA_DB_APP_ID=...`
/// before calling `open_database`, or use [`db_path_for`] directly.
const DEFAULT_APP_ID: &str = "big-audio-player";

/// Resolve the library database path for a specific app-id.
///
/// Always returns `$XDG_CONFIG_HOME/<app_id>/library.db` (falling back to
/// `./<app_id>/library.db` if the XDG config dir cannot resolve a home).
#[must_use]
pub fn db_path_for(app_id: &str) -> PathBuf {
    let mut path = big_os_kit::xdg_dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(app_id);
    path.push("library.db");
    path
}

/// Resolve the library database path using the `BIG_MEDIA_DB_APP_ID`
/// env var, falling back to [`DEFAULT_APP_ID`] when unset / empty.
///
/// Reads the env var at every call so test harnesses can swap it via
/// `std::env::set_var` without restarting the process.
fn db_path() -> PathBuf {
    let app_id = std::env::var("BIG_MEDIA_DB_APP_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_APP_ID.to_string());
    db_path_for(&app_id)
}
