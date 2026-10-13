//! Explicit, paged review of unavailable local library entries. Filesystem
//! absence never deletes a row by itself. This API never removes a media file.
use super::{DbHandle, models::Track, queries};
use crate::{Error, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
const CHUNK: usize = 128;
const MAX_CHECKED: usize = 2048;
pub const MAX_SELECTED: usize = 100;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Present,
    Missing,
    NoAccess,
    NotRegular,
    NotLocal,
}
#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub track: Track,
    pub availability: Availability,
    pub playlist_count: usize,
    memberships: Vec<(i64, i64)>,
    queued: Vec<i64>,
}
#[derive(Debug)]
pub struct ReviewPage {
    pub entries: Vec<ReviewItem>,
    pub checked: usize,
    pub next_after: Option<i64>,
    pub upper_id: i64,
}
fn stop(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
fn lock_db<'a>(
    db: &'a DbHandle,
    cancel: &AtomicBool,
) -> Result<std::sync::MutexGuard<'a, Connection>> {
    loop {
        stop(cancel)?;
        match db.try_lock() {
            Ok(connection) => return Ok(connection),
            Err(std::sync::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(5))
            }
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(Error::internal("Library connection unavailable"));
            }
        }
    }
}
/// Native paths only. Remote media is not "missing" and is not deletable here.
pub fn availability(path: &str) -> Availability {
    if !Path::new(path).is_absolute() {
        return Availability::NotLocal;
    }
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Availability::Present,
        Ok(_) => Availability::NotRegular,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Availability::Missing,
        Err(_) => Availability::NoAccess,
    }
}
fn relations(conn: &Connection, id: i64) -> Result<(Vec<(i64, i64)>, Vec<i64>)> {
    let mut stmt=conn.prepare("SELECT playlist_id,position FROM playlist_tracks WHERE track_id=?1 ORDER BY playlist_id,position LIMIT 10001")?;
    let members = stmt
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if members.len() > 10_000 {
        return Err(Error::internal(
            "Too many playlist references for one reviewed removal",
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT position FROM queue_tracks WHERE track_id=?1 ORDER BY position LIMIT 10001",
    )?;
    let queue = stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<i64>>>()?;
    if queue.len() > 10_000 {
        return Err(Error::internal(
            "Too many queue references for one reviewed removal",
        ));
    }
    Ok((members, queue))
}
fn bounded_track(row: &rusqlite::Row<'_>) -> rusqlite::Result<Track> {
    for index in 0..row.as_ref().column_count() {
        if let rusqlite::types::ValueRef::Text(text) = row.get_ref(index)?
            && text.len() > 4096
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    queries::row_to_track(row)
}
/// Examine at most 2048 rows and return at most 100 unavailable records. A fixed
/// high-water id keeps a review finite while new tracks are being imported.
/// Results preserve prior ratings and playlist memberships for CAS at removal.
pub fn page(
    db: &DbHandle,
    after: i64,
    upper: Option<i64>,
    cancel: &AtomicBool,
) -> Result<ReviewPage> {
    let high = if let Some(high) = upper {
        high
    } else {
        lock_db(db, cancel)?
            .query_row("SELECT COALESCE(MAX(id),0) FROM tracks", [], |r| r.get(0))?
    };
    let mut cursor = after.max(0);
    let mut checked = 0;
    let mut entries = Vec::new();
    let mut ended = false;
    while checked < MAX_CHECKED && entries.len() < MAX_SELECTED {
        stop(cancel)?;
        let batch = {
            let conn = lock_db(db, cancel)?;
            let mut statement = conn.prepare(&format!(
                "SELECT {} FROM tracks WHERE id>?1 AND id<=?2 ORDER BY id LIMIT ?3",
                queries::TRACK_COLUMNS
            ))?;
            let rows = statement.query_map(params![cursor, high, CHUNK as i64], bounded_track)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if batch.is_empty() {
            ended = true;
            break;
        }
        for track in batch {
            stop(cancel)?;
            cursor = track.id;
            checked += 1;
            let status = availability(&track.file_path);
            if status != Availability::Present {
                let (memberships, queued) = {
                    let conn = lock_db(db, cancel)?;
                    relations(&conn, track.id)?
                };
                entries.push(ReviewItem {
                    track,
                    availability: status,
                    playlist_count: memberships.len(),
                    memberships,
                    queued,
                });
            }
            if entries.len() == MAX_SELECTED || checked == MAX_CHECKED {
                break;
            }
        }
        if cursor >= high {
            ended = true;
            break;
        }
    }
    Ok(ReviewPage {
        entries,
        checked,
        next_after: (!ended && cursor < high).then_some(cursor),
        upper_id: high,
    })
}
/// Remove only reviewed Missing rows, never inaccessible/remote files. All rows
/// and their playlist/queue relationships are rechecked before the first DELETE.
/// Changed metadata, a restored file, a cancellation or SQL error rolls back the
/// whole selection. The in-memory playback queue is left running.
pub fn remove_reviewed(
    db: &DbHandle,
    selected: &[ReviewItem],
    cancel: &AtomicBool,
) -> Result<usize> {
    if selected.is_empty() || selected.len() > MAX_SELECTED {
        return Err(Error::invalid_argument(
            "selection",
            "choose between 1 and 100 reviewed records",
        ));
    }
    let ids: HashSet<i64> = selected.iter().map(|v| v.track.id).collect();
    if ids.len() != selected.len() {
        return Err(Error::internal("Duplicate reviewed record"));
    }
    let conn = lock_db(db, cancel)?;
    let tx = conn.unchecked_transaction()?;
    for item in selected {
        stop(cancel)?;
        if item.availability != Availability::Missing
            || availability(&item.track.file_path) != Availability::Missing
        {
            return Err(Error::internal(
                "A selected file is available again or cannot be safely reviewed. Reload before removing.",
            ));
        }
        let current = tx
            .query_row(
                &format!("SELECT {} FROM tracks WHERE id=?1", queries::TRACK_COLUMNS),
                [item.track.id],
                bounded_track,
            )
            .optional()?;
        let (memberships, queued) = relations(&tx, item.track.id)?;
        if current.as_ref() != Some(&item.track)
            || memberships != item.memberships
            || queued != item.queued
        {
            return Err(Error::internal(
                "Library metadata changed after review. Reload; nothing was removed.",
            ));
        }
    }
    for item in selected {
        stop(cancel)?;
        // Explicit dependent deletes also protect databases opened without
        // foreign_keys=ON; the reviewed relationships must match above first.
        tx.execute(
            "DELETE FROM playlist_tracks WHERE track_id=?1",
            [item.track.id],
        )?;
        tx.execute(
            "DELETE FROM queue_tracks WHERE track_id=?1",
            [item.track.id],
        )?;
        if tx.execute("DELETE FROM tracks WHERE id=?1", [item.track.id])? != 1 {
            return Err(Error::internal("Reviewed record disappeared"));
        }
    }
    stop(cancel)?;
    tx.commit()?;
    Ok(selected.len())
}
#[cfg(all(test, feature = "library"))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    fn database() -> DbHandle {
        let c = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&c).unwrap();
        Arc::new(Mutex::new(c))
    }
    #[test]
    fn restored_file_blocks_removal() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        let db = database();
        let t = Track::new_external_path(p.to_str().unwrap());
        queries::insert_track(&db.lock().unwrap(), &t).unwrap();
        let stop = AtomicBool::new(false);
        let review = page(&db, 0, None, &stop).unwrap();
        std::fs::write(p, b"restored").unwrap();
        assert!(remove_reviewed(&db, &review.entries, &stop).is_err());
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT count(*) FROM tracks", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    #[test]
    fn changed_rating_blocks_removal() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("missing.wav");
        let db = database();
        let id = queries::insert_track(
            &db.lock().unwrap(),
            &Track::new_external_path(p.to_str().unwrap()),
        )
        .unwrap();
        let stop = AtomicBool::new(false);
        let review = page(&db, 0, None, &stop).unwrap();
        db.lock()
            .unwrap()
            .execute("UPDATE tracks SET rating=5 WHERE id=?1", [id])
            .unwrap();
        assert!(remove_reviewed(&db, &review.entries, &stop).is_err());
        let review = page(&db, 0, None, &stop).unwrap();
        assert_eq!(remove_reviewed(&db, &review.entries, &stop).unwrap(), 1);
        assert!(!p.exists());
    }
    #[test]
    fn cancelled_review_never_deletes() {
        let db = database();
        assert!(matches!(
            page(&db, 0, None, &AtomicBool::new(true)),
            Err(Error::Cancelled)
        ));
    }
}
