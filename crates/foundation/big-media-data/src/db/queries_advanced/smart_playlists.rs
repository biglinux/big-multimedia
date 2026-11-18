use rusqlite::{Connection, params};

use crate::db::models::Track;
use crate::db::queries::{row_to_track, track_columns};

pub fn get_most_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let mut stmt = conn.prepare(concat!(
        "SELECT ",
        track_columns!(),
        " FROM tracks WHERE play_count > 0 ORDER BY play_count DESC LIMIT ?1"
    ))?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_recently_added(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let mut stmt = conn.prepare(concat!(
        "SELECT ",
        track_columns!(),
        " FROM tracks ORDER BY date_added DESC LIMIT ?1"
    ))?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn get_never_played(conn: &Connection, limit: u32) -> crate::Result<Vec<Track>> {
    let mut stmt = conn.prepare(concat!(
        "SELECT ",
        track_columns!(),
        " FROM tracks WHERE play_count = 0 ORDER BY title LIMIT ?1"
    ))?;
    let tracks = stmt
        .query_map(params![limit], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}
