use rusqlite::{Connection, params};

use crate::db::models::Track;
use crate::db::queries::{TRACK_COLUMNS, row_to_track};

pub fn get_most_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS} FROM tracks WHERE play_count > 0 ORDER BY play_count DESC LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_recently_added(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks ORDER BY date_added DESC LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_never_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let sql =
        format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE play_count = 0 ORDER BY title LIMIT ?1");
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}
