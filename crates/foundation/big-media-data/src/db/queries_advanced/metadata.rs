use rusqlite::{Connection, params};

pub fn update_track_rating(conn: &Connection, track_id: i64, rating: i32) -> crate::Result<()> {
    conn.execute(
        "UPDATE tracks SET rating = ?1 WHERE id = ?2",
        params![rating.clamp(0, 5), track_id],
    )?;
    Ok(())
}

pub struct TrackMetadataUpdate<'a> {
    pub title: Option<&'a str>,
    pub artist: Option<&'a str>,
    pub album: Option<&'a str>,
    pub album_artist: Option<&'a str>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub year: Option<i32>,
    pub genre: Option<&'a str>,
}

/// Empty strings stored as NULL (caller passes `None` for empties).
#[allow(clippy::needless_pass_by_value)]
pub fn update_track_metadata(
    conn: &Connection,
    track_id: i64,
    m: TrackMetadataUpdate<'_>,
) -> crate::Result<()> {
    conn.execute(
        "UPDATE tracks SET \
         title = ?1, artist = ?2, album = ?3, album_artist = ?4, \
         track_number = ?5, disc_number = ?6, year = ?7, genre = ?8 \
         WHERE id = ?9",
        params![
            m.title,
            m.artist,
            m.album,
            m.album_artist,
            m.track_number,
            m.disc_number,
            m.year,
            m.genre,
            track_id
        ],
    )?;
    Ok(())
}
