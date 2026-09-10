use super::setup_db;
use crate::db::queries_advanced::library::validate_column;
use crate::db::queries_advanced::{
    get_compilation_albums, get_library_groups, get_library_track_count, get_library_tracks,
};
use rusqlite::Connection;

/// Insert a track with an explicit INTEGER `year`/`bitrate` so the integer
/// column affinity tests have non-zero values to group and filter on.
fn insert_year_track(conn: &Connection, id: i64, artist: &str, year: i32, bitrate: i32) {
    conn.execute(
        "INSERT INTO tracks (id, file_path, title, artist, album, album_artist, genre, year, \
         track_number, duration_secs, format, bitrate, sample_rate, play_count, rating, \
         date_added, last_played) \
         VALUES (?1, ?2, ?3, ?4, 'Album', '', 'Rock', ?5, 0, 0.0, 'MP3', ?6, 0, 0, 0, \
         strftime('%s','now'), NULL)",
        rusqlite::params![
            id,
            format!("/{id}.mp3"),
            format!("t{id}"),
            artist,
            year,
            bitrate
        ],
    )
    .unwrap();
}

#[test]
fn validate_column_known() {
    assert_eq!(validate_column("artist"), "artist");
    assert_eq!(validate_column("album"), "album");
    assert_eq!(validate_column("genre"), "genre");
    assert_eq!(validate_column("year"), "year");
    assert_eq!(validate_column("sample_rate"), "sample_rate");
}

#[test]
fn validate_column_injection_attempt() {
    assert_eq!(validate_column("title; DROP TABLE tracks"), "artist");
    assert_eq!(validate_column("unknown_field"), "artist");
    assert_eq!(validate_column(""), "artist");
}

#[test]
fn library_counts_empty() {
    let conn = setup_db();
    assert_eq!(get_library_track_count(&conn, "", None).unwrap(), 0);
    assert!(
        get_library_groups(&conn, "artist", "", &[], None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn integer_year_column_groups_decode_as_text() {
    let conn = setup_db();
    insert_year_track(&conn, 1, "Alice", 2000, 320);
    insert_year_track(&conn, 2, "Alice", 2000, 320);
    insert_year_track(&conn, 3, "Bob", 2010, 256);

    // Regression: integer `year` must come back as decodable TEXT group keys
    // with correct counts (previously the integer rows were dropped).
    let groups = get_library_groups(&conn, "year", "", &[], None).unwrap();
    assert_eq!(
        groups,
        vec![("2000".to_string(), 2), ("2010".to_string(), 1)]
    );
}

#[test]
fn integer_year_column_filters_by_string_param() {
    let conn = setup_db();
    insert_year_track(&conn, 1, "Alice", 2000, 320);
    insert_year_track(&conn, 2, "Alice", 2000, 320);
    insert_year_track(&conn, 3, "Bob", 2010, 256);

    // Regression: filtering the INTEGER `year` column with the string key the
    // library tree passes must match (int-vs-text affinity bug).
    let yr2000 = get_library_tracks(&conn, &[("year", "2000")], "", None).unwrap();
    assert_eq!(yr2000.len(), 2);
    let yr2010 = get_library_tracks(&conn, &[("year", "2010")], "", None).unwrap();
    assert_eq!(yr2010.len(), 1);
    let missing = get_library_tracks(&conn, &[("year", "1999")], "", None).unwrap();
    assert!(missing.is_empty());
}

#[test]
fn integer_bitrate_column_groups_and_filters() {
    let conn = setup_db();
    insert_year_track(&conn, 1, "Alice", 2000, 320);
    insert_year_track(&conn, 2, "Bob", 2010, 256);

    let groups = get_library_groups(&conn, "bitrate", "", &[], None).unwrap();
    assert_eq!(groups, vec![("256".to_string(), 1), ("320".to_string(), 1)]);
    let hi = get_library_tracks(&conn, &[("bitrate", "320")], "", None).unwrap();
    assert_eq!(hi.len(), 1);
    assert_eq!(hi[0].artist.as_deref(), Some("Alice"));
}

#[test]
fn text_artist_column_still_groups_after_cast_change() {
    let conn = setup_db();
    insert_year_track(&conn, 1, "Alice", 2000, 320);
    insert_year_track(&conn, 2, "Bob", 2010, 256);

    let groups = get_library_groups(&conn, "artist", "", &[], None).unwrap();
    assert_eq!(
        groups,
        vec![("Alice".to_string(), 1), ("Bob".to_string(), 1)]
    );
}

/// Insert with explicit album + `date_added` for compilation / age tests.
fn insert_album_track(conn: &Connection, id: i64, artist: &str, album: &str, date_added: i64) {
    conn.execute(
        "INSERT INTO tracks (id, file_path, title, artist, album, album_artist, genre, year, \
         track_number, duration_secs, format, bitrate, sample_rate, play_count, rating, \
         date_added, last_played) \
         VALUES (?1, ?2, ?3, ?4, ?5, '', 'Rock', 2000, 0, 0.0, 'MP3', 0, 0, 0, 0, ?6, NULL)",
        rusqlite::params![
            id,
            format!("/{id}.mp3"),
            format!("t{id}"),
            artist,
            album,
            date_added
        ],
    )
    .unwrap();
}

#[test]
fn compilation_albums_require_more_than_one_artist() {
    let conn = setup_db();
    // "Mix" has two distinct artists -> compilation; "Solo" has one -> not.
    insert_album_track(&conn, 1, "Alice", "Mix", 0);
    insert_album_track(&conn, 2, "Bob", "Mix", 0);
    insert_album_track(&conn, 3, "Carol", "Solo", 0);
    insert_album_track(&conn, 4, "Carol", "Solo", 0);

    let comps = get_compilation_albums(&conn, "", &[], None).unwrap();
    assert_eq!(comps, vec![("Mix".to_string(), 2)]);
}

#[test]
fn text_filter_narrows_tracks_and_groups() {
    let conn = setup_db();
    insert_year_track(&conn, 1, "Alice", 2000, 320);
    insert_year_track(&conn, 2, "Bob", 2010, 256);

    // Empty filter returns everything; a matching filter narrows by FTS.
    assert_eq!(get_library_tracks(&conn, &[], "", None).unwrap().len(), 2);
    let only_alice = get_library_tracks(&conn, &[], "Alice", None).unwrap();
    assert_eq!(only_alice.len(), 1);
    assert_eq!(only_alice[0].artist.as_deref(), Some("Alice"));
    assert_eq!(get_library_track_count(&conn, "Alice", None).unwrap(), 1);
}

#[test]
fn age_filter_excludes_tracks_older_than_window() {
    let conn = setup_db();
    let now: i64 = conn
        .query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| {
            r.get(0)
        })
        .unwrap();
    insert_album_track(&conn, 1, "Recent", "A", now);
    insert_album_track(&conn, 2, "Old", "B", now - 10_000);

    // No window -> both; a 100s window -> only the recent insert.
    assert_eq!(get_library_track_count(&conn, "", None).unwrap(), 2);
    assert_eq!(get_library_track_count(&conn, "", Some(100)).unwrap(), 1);
    let recent = get_library_tracks(&conn, &[], "", Some(100)).unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].artist.as_deref(), Some("Recent"));
}
