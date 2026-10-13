// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: hostile filter text + hostile file paths must not be able
//! to drop the tracks table, leak data, or crash the FTS5 parser.
//!
//! Mirrors the `get_library_tracks` call path used by the audio player
//! sidebar filter input.

use big_media_runtime::db::fts::{fts_match_query, fts_phrase};
use rusqlite::{Connection, params};

const HOSTILE_FILENAMES: &[&str] = &[
    "/tmp/x'); DROP TABLE tracks; --.mp3",
    "/tmp/x\"; DROP TABLE tracks;.mp3",
    "/tmp/normal.mp3",
    "/tmp/has \"quotes\" inside.mp3",
    "/tmp/a*b(c)d.mp3",
];

const HOSTILE_FILTERS: &[&str] = &[
    "x'); DROP TABLE tracks; --",
    "OR 1=1",
    "* OR \"a\"",
    "\"; DELETE FROM tracks; --",
    "a OR b",
];

fn setup() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch(
        "
        CREATE TABLE tracks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path TEXT NOT NULL UNIQUE,
            title TEXT,
            artist TEXT,
            album TEXT,
            genre TEXT
        );
        CREATE VIRTUAL TABLE tracks_fts USING fts5(
            title, artist, album, genre, content='tracks', content_rowid='id'
        );
        CREATE TRIGGER tracks_ai AFTER INSERT ON tracks BEGIN
            INSERT INTO tracks_fts(rowid, title, artist, album, genre)
            VALUES (new.id, new.title, new.artist, new.album, new.genre);
        END;
        ",
    )
    .expect("schema");
    for path in HOSTILE_FILENAMES {
        conn.execute(
            "INSERT INTO tracks (file_path, title, artist, album, genre)
             VALUES (?1, ?2, 'Test Artist', 'Test Album', 'Test Genre')",
            params![path, path],
        )
        .expect("insert");
    }
    conn
}

fn count_tracks(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .expect("count")
}

#[test]
fn hostile_filter_text_keeps_tables() {
    let conn = setup();
    let initial = count_tracks(&conn);
    assert_eq!(initial as usize, HOSTILE_FILENAMES.len());

    for filter in HOSTILE_FILTERS {
        let query = fts_match_query(filter);
        // FTS5 MATCH returns rows or nothing; never executes injected DDL/DML.
        let _ = conn
            .prepare("SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1")
            .and_then(|mut s| {
                let _rows: Vec<i64> = s
                    .query_map(params![&query], |r| r.get(0))?
                    .filter_map(Result::ok)
                    .collect();
                Ok(())
            });
    }
    assert_eq!(
        count_tracks(&conn),
        initial,
        "hostile filters must not delete rows"
    );
}

#[test]
fn hostile_filenames_round_trip_through_fts() {
    let conn = setup();
    // The "DROP TABLE" filename is indexed as content; querying for the
    // literal substring through fts_match_query must locate it without
    // executing the embedded SQL.
    let query = fts_match_query("DROP TABLE");
    let mut stmt = conn
        .prepare("SELECT rowid FROM tracks_fts WHERE tracks_fts MATCH ?1")
        .expect("prepare");
    let rows: Vec<i64> = stmt
        .query_map(params![&query], |r| r.get(0))
        .expect("query")
        .filter_map(Result::ok)
        .collect();
    // At least one of the hostile filenames must be returned as a match
    // (the FTS index sees the literal text, not as SQL).
    assert!(!rows.is_empty(), "FTS should find DROP TABLE as plain text");
    assert_eq!(
        count_tracks(&conn),
        HOSTILE_FILENAMES.len() as i64,
        "table still intact"
    );
}

#[test]
fn empty_filter_yields_no_match_clause() {
    // Caller policy: empty fts_match_query must be skipped before passing
    // to FTS5 (FTS5 syntax-errors on empty MATCH). This test pins the
    // contract.
    assert_eq!(fts_match_query(""), "");
    assert_eq!(fts_match_query("   "), "");
}

#[test]
fn fts_phrase_doubles_embedded_quotes() {
    assert_eq!(fts_phrase("a\"b"), "\"a\"\"b\"");
}
