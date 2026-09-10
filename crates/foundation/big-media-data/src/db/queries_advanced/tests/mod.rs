use rusqlite::Connection;

use crate::db::schema;

mod library;
mod metadata;
mod queue;
mod radio;
mod smart_playlists;

fn setup_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::run_migrations(&conn).unwrap();
    conn
}

fn insert_minimal_track(conn: &Connection, id: i64, title: &str, artist: &str) {
    conn.execute(
        "INSERT INTO tracks (id, file_path, title, artist, album, album_artist, genre, year, \
         track_number, duration_secs, format, bitrate, sample_rate, play_count, rating, \
         date_added, last_played) \
         VALUES (?1, ?2, ?3, ?4, '', '', '', 0, 0, 0.0, '', 0, 0, 0, 0, strftime('%s','now'), NULL)",
        rusqlite::params![id, format!("/{id}.mp3"), title, artist],
    )
    .unwrap();
}
