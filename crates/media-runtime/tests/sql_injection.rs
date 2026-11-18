// SPDX-License-Identifier: GPL-3.0-or-later

//! Hostile filter text, parent-filter values and file paths sent through the
//! library query the audio sidebar uses, on a database built by the production
//! migrations.

use big_media_runtime::db::queries::{get_library_track_count, get_library_tracks};
use big_media_runtime::db::schema::run_migrations;
use rusqlite::{Connection, params};

const HOSTILE_FILENAMES: &[&str] = &[
    "/tmp/x'); DROP TABLE tracks; --.mp3",
    "/tmp/x\"; DROP TABLE tracks;.mp3",
    "/tmp/normal.mp3",
    "/tmp/has \"quotes\" inside.mp3",
    "/tmp/a*b(c)d.mp3",
];

/// Shares no token with any filter below, so any query that returns it leaked.
const PRIVATE_TRACK: &str = "/music/private.flac";

fn library() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    run_migrations(&conn).expect("migrations");
    for path in HOSTILE_FILENAMES {
        conn.execute(
            "INSERT INTO tracks (file_path, title, artist, album, genre)
             VALUES (?1, ?1, 'Test Artist', 'Test Album', 'Test Genre')",
            params![path],
        )
        .expect("insert");
    }
    conn.execute(
        "INSERT INTO tracks (file_path, title, artist, album, genre)
         VALUES (?1, 'Private Song', 'Other Band', 'Hidden', 'Jazz')",
        params![PRIVATE_TRACK],
    )
    .expect("insert");
    conn
}

fn sorted_paths(conn: &Connection, parents: &[(&str, &str)], filter: &str) -> Vec<String> {
    let tracks = get_library_tracks(conn, parents, filter, None)
        .unwrap_or_else(|e| panic!("{parents:?} {filter:?}: {e}"));
    let mut paths: Vec<String> = tracks.into_iter().map(|t| t.file_path).collect();
    paths.sort();
    paths
}

#[test]
fn hostile_filters_match_only_text() {
    let conn = library();
    // Expected rows from the unicode61 tokens of each title: every token is a
    // quoted prefix phrase, and an operator word or punctuation-only token
    // matches no row.
    let drop_table = vec![
        HOSTILE_FILENAMES[1].to_string(),
        HOSTILE_FILENAMES[0].to_string(),
    ];
    let cases: [(&str, Vec<String>); 6] = [
        ("x'); DROP TABLE tracks; --", drop_table.clone()),
        ("OR 1=1", Vec::new()),
        ("* OR \"a\"", Vec::new()),
        ("\"; DELETE FROM tracks; --", Vec::new()),
        ("a OR b", Vec::new()),
        ("DROP TABLE", drop_table),
    ];
    for (filter, expected) in cases {
        assert_eq!(sorted_paths(&conn, &[], filter), expected, "{filter:?}");
    }

    let mut all: Vec<String> = HOSTILE_FILENAMES.iter().map(|p| (*p).to_string()).collect();
    all.push(PRIVATE_TRACK.to_string());
    all.sort();
    assert_eq!(sorted_paths(&conn, &[], ""), all, "rows and paths intact");
}

#[test]
fn hostile_parent_values_are_bound() {
    let conn = library();
    for value in ["x' OR '1'='1", "Test Artist' --", "') OR 1=1 --"] {
        assert!(
            sorted_paths(&conn, &[("artist", value)], "").is_empty(),
            "{value:?}"
        );
    }
    assert_eq!(
        sorted_paths(&conn, &[("genre", "Jazz")], ""),
        [PRIVATE_TRACK]
    );
    assert!(get_library_tracks(&conn, &[("artist = artist OR 1=1 --", "")], "", None).is_err());
}

#[test]
fn blank_search_lists_whole_library() {
    let conn = library();
    let all = sorted_paths(&conn, &[], "");
    assert_eq!(all.len(), HOSTILE_FILENAMES.len() + 1);
    for filter in ["   ", "\t\n"] {
        assert_eq!(sorted_paths(&conn, &[], filter), all, "{filter:?}");
        let count = get_library_track_count(&conn, filter, None)
            .unwrap_or_else(|e| panic!("{filter:?}: {e}"));
        assert_eq!(usize::try_from(count).unwrap(), all.len(), "{filter:?}");
    }
}

#[test]
fn damaged_search_index_fails_query() {
    let conn = library();
    conn.execute_batch("DELETE FROM tracks_fts_data")
        .expect("damage index");
    assert!(get_library_tracks(&conn, &[], "Test", None).is_err());
}
