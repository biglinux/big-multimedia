use rusqlite::Connection;

use super::setup_db;
use crate::db::queries_advanced::{get_most_played, get_never_played, get_recently_added};

/// Insert a track with control over the fields the play-statistics queries filter
/// and sort on. Other columns get harmless defaults.
#[allow(clippy::too_many_arguments)] // test fixture: explicit columns read clearer than a struct
fn insert_full(
    conn: &Connection,
    id: i64,
    title: &str,
    artist: &str,
    genre: &str,
    year: i64,
    play_count: i64,
    date_added: i64,
) {
    conn.execute(
        "INSERT INTO tracks (id, file_path, title, artist, album, album_artist, genre, year, \
         track_number, duration_secs, format, bitrate, sample_rate, play_count, rating, \
         date_added, last_played) \
         VALUES (?1, ?2, ?3, ?4, '', '', ?5, ?6, 0, 0.0, '', 0, 0, ?7, 0, ?8, NULL)",
        rusqlite::params![
            id,
            format!("/{id}.mp3"),
            title,
            artist,
            genre,
            year,
            play_count,
            date_added
        ],
    )
    .unwrap();
}

/// Run the rule engine and return the matched titles in result order.
#[test]
fn most_played_empty() {
    let conn = setup_db();
    assert!(get_most_played(&conn, 10).unwrap().is_empty());
}

#[test]
fn recently_added_empty() {
    let conn = setup_db();
    assert!(get_recently_added(&conn, 10).unwrap().is_empty());
}

#[test]
fn never_played_empty() {
    let conn = setup_db();
    assert!(get_never_played(&conn, 10).unwrap().is_empty());
}

// Curated-query results on a populated database.

#[test]
fn most_played_orders_desc_and_excludes_unplayed() {
    let conn = setup_db();
    insert_full(&conn, 1, "Low", "A", "", 0, 2, 100);
    insert_full(&conn, 2, "High", "A", "", 0, 9, 100);
    insert_full(&conn, 3, "Never", "A", "", 0, 0, 100);
    let titles: Vec<String> = get_most_played(&conn, 10)
        .unwrap()
        .iter()
        .map(|t| t.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles, ["High", "Low"], "play_count>0, ordered DESC");
}

#[test]
fn never_played_returns_only_zero_play_count() {
    let conn = setup_db();
    insert_full(&conn, 1, "Fresh", "A", "", 0, 0, 100);
    insert_full(&conn, 2, "Played", "A", "", 0, 3, 100);
    let titles: Vec<String> = get_never_played(&conn, 10)
        .unwrap()
        .iter()
        .map(|t| t.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles, ["Fresh"]);
}

#[test]
fn recently_added_orders_by_date_desc() {
    let conn = setup_db();
    insert_full(&conn, 1, "Old", "A", "", 0, 0, 100);
    insert_full(&conn, 2, "New", "A", "", 0, 0, 200);
    let titles: Vec<String> = get_recently_added(&conn, 10)
        .unwrap()
        .iter()
        .map(|t| t.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles, ["New", "Old"]);
}
