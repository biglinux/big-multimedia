use rusqlite::{Connection, params};

use crate::db::models::Track;
use crate::db::queries::{TRACK_COLUMNS_PREFIXED, row_to_track};

pub fn load_queue(conn: &Connection) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS_PREFIXED} FROM queue_tracks qt
         JOIN tracks t ON t.id = qt.track_id
         ORDER BY qt.position"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map([], row_to_track)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(tracks)
}

/// A queue can contain library tracks, external files and remote episodes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SavedQueue {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QueueSession {
    pub music: SavedQueue,
    pub podcasts: SavedQueue,
}

/// Publish both queues together. Keep the legacy ID-only table untouched so an
/// interrupted upgrade never destroys the previous application's queue.
pub fn save_queue_session(conn: &Connection, session: &QueueSession) -> crate::Result<()> {
    // Acknowledged queue saves must sync the WAL before the window may close.
    // The connection is shared, so every other write goes back to NORMAL.
    conn.pragma_update(None, "synchronous", "FULL")?;
    let saved = write_queue_session(conn, session);
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    saved
}

fn write_queue_session(conn: &Connection, session: &QueueSession) -> crate::Result<()> {
    let transaction = conn.unchecked_transaction()?;
    for (name, queue) in [("music", &session.music), ("podcasts", &session.podcasts)] {
        if queue
            .current
            .is_some_and(|index| index >= queue.tracks.len())
        {
            return Err(crate::Error::invalid_argument(
                "queue selection",
                "out of bounds",
            ));
        }
        transaction.execute(
            "INSERT INTO playback_queues (name, current_position) VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET current_position=excluded.current_position",
            params![name, queue.current.map(|index| index as i64)],
        )?;
        transaction.execute(
            "DELETE FROM playback_queue_items WHERE queue_name=?1",
            [name],
        )?;
        let mut insert = transaction.prepare(
            "INSERT INTO playback_queue_items
             (queue_name, position, file_path, track_id, title, artist, album, duration_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for (position, track) in queue.tracks.iter().enumerate() {
            if !track.duration_secs.is_finite() || track.duration_secs < 0.0 {
                return Err(crate::Error::invalid_argument(
                    "queue duration",
                    "not finite or negative",
                ));
            }
            insert.execute(params![
                name,
                position as i64,
                track.file_path,
                track.id,
                track.title,
                track.artist,
                track.album,
                track.duration_secs
            ])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn load_queue_session(conn: &Connection) -> crate::Result<QueueSession> {
    use rusqlite::OptionalExtension;
    let transaction = conn.unchecked_transaction()?;
    let mut session = QueueSession::default();
    for (name, queue) in [
        ("music", &mut session.music),
        ("podcasts", &mut session.podcasts),
    ] {
        let selection: Option<Option<i64>> = transaction
            .query_row(
                "SELECT current_position FROM playback_queues WHERE name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()?;
        let Some(selection) = selection else {
            if name == "music" {
                queue.tracks = load_queue(&transaction)?;
                queue.current = (!queue.tracks.is_empty()).then_some(0);
            }
            continue;
        };
        let sql = format!(
            "SELECT {TRACK_COLUMNS_PREFIXED}, q.file_path, q.track_id, q.title, q.artist,
                    q.album, q.duration_secs
             FROM playback_queue_items q LEFT JOIN tracks t ON t.file_path=q.file_path
             WHERE q.queue_name=?1 ORDER BY q.position"
        );
        queue.tracks = transaction
            .prepare(&sql)?
            .query_map([name], |row| {
                if row.get::<_, Option<i64>>(0)?.is_some() {
                    return row_to_track(row);
                }
                // The q.* columns follow the track columns; derive the offset from
                // the list itself so a new track column can't shift these reads.
                let q = TRACK_COLUMNS_PREFIXED.split(',').count();
                let mut track = Track::new_external_path(row.get::<_, String>(q)?);
                // A library entry removed since the save remains a playable external
                // path. Negative IDs identify podcast episodes, not library records.
                track.id = row.get::<_, i64>(q + 1)?.min(0);
                track.title = row.get(q + 2)?;
                track.artist = row.get(q + 3)?;
                track.album = row.get(q + 4)?;
                track.duration_secs = row.get(q + 5)?;
                Ok(track)
            })?
            .collect::<rusqlite::Result<_>>()?;
        queue.current = selection.and_then(|index| usize::try_from(index).ok());
        if queue
            .current
            .is_some_and(|index| index >= queue.tracks.len())
        {
            return Err(crate::Error::invalid_argument(
                "saved queue selection",
                "out of bounds",
            ));
        }
    }
    transaction.commit()?;
    Ok(session)
}
