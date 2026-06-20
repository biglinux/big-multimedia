//! Pure (GTK-free) library folder scanner for the Relm4 `AudioShell`.
//!
//! Ported out of the god-object `window::library_scan::scan` (which was an
//! `impl Window` method) so the shell can run it off-thread without a GTK
//! widget. Discovers audio files, reads tags, batch-inserts, and prunes stale
//! rows. The shell wraps it in `gio::spawn_blocking` and refreshes the tree on
//! completion; live progress (the old 250 ms status-label poll) is dropped.

use std::collections::HashSet;
use std::path::Path;
use std::sync::PoisonError;

use crate::db::queries;
use crate::db::DbHandle;
use crate::library::{metadata, scanner};

/// Scan `folder` into the library: register it as a watch folder, insert any
/// newly-discovered tracks (batched), and prune entries whose files vanished.
/// Returns `(added, skipped)`.
pub(crate) fn scan_folder_blocking(db: &DbHandle, folder: &str) -> Result<(usize, usize), String> {
    {
        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = queries::add_watch_folder(&conn, folder);
    }

    let existing: HashSet<String> = {
        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        queries::get_all_file_paths(&conn)
            .unwrap_or_default()
            .into_iter()
            .collect()
    };

    let files = scanner::discover_audio_files(folder).map_err(|e| e.to_string())?;
    let total = files.len();

    let mut new_tracks = Vec::new();
    let mut skipped = 0_usize;
    for path in &files {
        let path_str = path.to_string_lossy();
        if existing.contains(path_str.as_ref()) {
            continue;
        }
        match metadata::read_track_metadata(&path_str) {
            Ok(track) => new_tracks.push(track),
            Err(e) => {
                log::debug!("scan skip {path_str}: {e}");
                skipped += 1;
            }
        }
    }

    let mut added = 0_usize;
    for chunk in new_tracks.chunks(50) {
        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        let txx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for track in chunk {
            if queries::insert_track(&txx, track).is_ok() {
                added += 1;
            }
        }
        txx.commit().map_err(|e| e.to_string())?;
    }

    {
        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        let stale = queries::get_all_file_paths(&conn).unwrap_or_default();
        for path in stale.iter().filter(|p| !Path::new(p).exists()) {
            let _ = conn.execute("DELETE FROM tracks WHERE file_path = ?1", [path]);
        }
    }

    log::info!("scan {folder}: +{added} new, {skipped} skipped of {total}");
    Ok((added, skipped))
}

/// Remove a watch folder by id (its tracks too), returning `true` on success.
pub(crate) fn remove_watch_folder_blocking(db: &DbHandle, folder_id: i64) -> bool {
    let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
    let path = queries::get_watch_folders(&conn)
        .unwrap_or_default()
        .into_iter()
        .find(|f| f.id == folder_id)
        .map(|f| f.path);
    if let Some(path) = path {
        let _ = queries::remove_tracks_in_folder(&conn, &path);
    }
    queries::remove_watch_folder(&conn, folder_id).is_ok()
}

/// All watch-folder paths (for a rescan-all).
pub(crate) fn watch_folder_paths_blocking(db: &DbHandle) -> Vec<String> {
    let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
    queries::get_watch_folders(&conn)
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path)
        .collect()
}
