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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

pub type DbHandle = Arc<Mutex<Connection>>;

/// How long background library work waits out another writer before it reports
/// the library busy.
pub const BACKGROUND_LOCK_WAIT: Duration = Duration::from_secs(30);

/// The library connection, waited for at most `limit` and given up as soon as
/// `cancel` is set. Polls, so never call it on the GUI thread.
///
/// # Errors
///
/// [`crate::Error::Cancelled`] once `cancel` is set, an [`crate::Error::Io`] of
/// kind [`std::io::ErrorKind::WouldBlock`] when the connection stays held past
/// `limit`, and [`crate::Error::Internal`] when a holder panicked.
pub fn lock_within<'a>(
    db: &'a DbHandle,
    limit: Duration,
    cancel: &AtomicBool,
) -> crate::Result<MutexGuard<'a, Connection>> {
    let until = Instant::now() + limit;
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(crate::Error::Cancelled);
        }
        match db.try_lock() {
            Ok(connection) => return Ok(connection),
            Err(TryLockError::Poisoned(_)) => {
                return Err(crate::Error::internal("Library connection unavailable"));
            }
            Err(TryLockError::WouldBlock) if Instant::now() >= until => {
                return Err(
                    std::io::Error::new(std::io::ErrorKind::WouldBlock, "Library busy").into(),
                );
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// Collect query rows, skipping (and logging) rows that fail to decode: one
/// damaged row should not hide the rest of the library, nor vanish silently.
/// A failure of the statement itself is returned, never taken for an empty result.
pub(crate) fn decoded_rows<T>(
    rows: impl Iterator<Item = rusqlite::Result<T>>,
) -> crate::Result<Vec<T>> {
    let mut decoded = Vec::new();
    for row in rows {
        match row {
            Ok(value) => decoded.push(value),
            Err(
                error @ (rusqlite::Error::FromSqlConversionFailure(..)
                | rusqlite::Error::IntegralValueOutOfRange(..)
                | rusqlite::Error::InvalidColumnType(..)
                | rusqlite::Error::Utf8Error(..)),
            ) => log::warn!("skipping unreadable database row: {error}"),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(decoded)
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

    #[test]
    fn lock_within_waits_boundedly() {
        let db: DbHandle = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        let idle = AtomicBool::new(false);
        assert!(lock_within(&db, Duration::ZERO, &idle).is_ok());

        let held = db.lock().unwrap();
        let started = Instant::now();
        match lock_within(&db, Duration::from_millis(50), &idle) {
            Err(crate::Error::Io(error)) => {
                assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
            }
            other => panic!("expected busy, got {:?}", other.map(|_| ())),
        }
        assert!(started.elapsed() >= Duration::from_millis(50));
        let cancelled = AtomicBool::new(true);
        assert!(matches!(
            lock_within(&db, Duration::from_secs(60), &cancelled),
            Err(crate::Error::Cancelled)
        ));
        drop(held);

        let poisoner = db.clone();
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.lock().unwrap();
            panic!("poison the connection");
        })
        .join();
        assert!(matches!(
            lock_within(&db, Duration::ZERO, &idle),
            Err(crate::Error::Internal(_))
        ));
    }
}
