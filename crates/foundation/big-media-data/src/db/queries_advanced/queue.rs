use rusqlite::{Connection, params};

use crate::db::models::Track;
use crate::db::queries::{TRACK_COLUMNS_PREFIXED, row_to_track};

pub fn save_queue(conn: &Connection, track_ids: &[i64]) -> crate::Result<()> {
    conn.execute("DELETE FROM queue_tracks", [])?;
    let mut stmt = conn.prepare("INSERT INTO queue_tracks (position, track_id) VALUES (?1, ?2)")?;
    for (i, tid) in track_ids.iter().enumerate() {
        stmt.execute(params![i as i64, tid])?;
    }
    Ok(())
}

pub fn load_queue(conn: &Connection) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS_PREFIXED} FROM queue_tracks qt
         JOIN tracks t ON t.id = qt.track_id
         ORDER BY qt.position"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map([], row_to_track)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(tracks)
}
