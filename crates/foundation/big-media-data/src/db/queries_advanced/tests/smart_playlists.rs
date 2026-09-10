use rusqlite::Connection;

use super::setup_db;
use crate::db::queries_advanced::{
    create_smart_playlist, delete_smart_playlist, execute_smart_playlist_rules,
    get_duplicate_tracks, get_most_played, get_never_played, get_recently_added,
    get_smart_playlists,
};

/// Insert a track with control over the fields the smart-playlist queries filter
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
fn rule_titles(
    conn: &Connection,
    rules_json: &str,
    logic: &str,
    sort: Option<&str>,
    dir: &str,
    max: i32,
) -> Vec<String> {
    execute_smart_playlist_rules(conn, rules_json, logic, sort, dir, max)
        .unwrap()
        .iter()
        .map(|t| t.title.clone().unwrap_or_default())
        .collect()
}

/// Four tracks with distinct genres/years for the operator tests. Sorted by
/// title ascending (the default), results come back as T1, T2, T3, T4 order.
fn genre_fixture(conn: &Connection) {
    insert_full(conn, 1, "T1", "A", "Rock", 1990, 0, 100);
    insert_full(conn, 2, "T2", "A", "Jazz", 2010, 0, 100);
    insert_full(conn, 3, "T3", "A", "Rocket", 1990, 0, 100);
    insert_full(conn, 4, "T4", "A", "", 2000, 0, 100);
}

#[test]
fn create_and_list_smart_playlists() {
    let conn = setup_db();
    create_smart_playlist(
        &conn,
        "Rock songs",
        r#"[{"field":"genre","operator":"equals","value":"Rock"}]"#,
        "AND",
        Some("title"),
        "ASC",
        50,
    )
    .unwrap();

    let playlists = get_smart_playlists(&conn).unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "Rock songs");
}

#[test]
fn delete_smart_playlist_removes_it() {
    let conn = setup_db();
    create_smart_playlist(&conn, "Test", "[]", "AND", None, "ASC", 0).unwrap();
    let list = get_smart_playlists(&conn).unwrap();
    assert_eq!(list.len(), 1);

    delete_smart_playlist(&conn, list[0].id).unwrap();
    let list2 = get_smart_playlists(&conn).unwrap();
    assert!(list2.is_empty());
}

#[test]
fn execute_smart_playlist_empty_rules() {
    let conn = setup_db();
    let tracks = execute_smart_playlist_rules(&conn, "[]", "AND", None, "ASC", 0).unwrap();
    assert!(tracks.is_empty());
}

#[test]
fn execute_smart_playlist_invalid_json() {
    let conn = setup_db();
    let tracks = execute_smart_playlist_rules(&conn, "not json", "AND", None, "ASC", 0).unwrap();
    assert!(tracks.is_empty());
}

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

#[test]
fn duplicate_tracks_empty() {
    let conn = setup_db();
    assert!(get_duplicate_tracks(&conn, 10).unwrap().is_empty());
}

// ── curated-query results (not just the empty-DB case) ──────────────────────

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

#[test]
fn duplicate_tracks_detects_case_insensitive_pairs() {
    let conn = setup_db();
    insert_full(&conn, 1, "Song", "Artist", "", 0, 0, 100);
    insert_full(&conn, 2, "song", "artist", "", 0, 0, 100); // dup of 1, case-insensitive
    insert_full(&conn, 3, "Unique", "Other", "", 0, 0, 100);
    let titles: Vec<String> = get_duplicate_tracks(&conn, 10)
        .unwrap()
        .iter()
        .map(|t| t.title.clone().unwrap_or_default())
        .collect();
    assert_eq!(titles.len(), 2, "only the duplicated pair");
    assert!(titles.iter().all(|t| t.eq_ignore_ascii_case("song")));
}

// ── rule operators (each match arm produces a distinct filter) ──────────────

#[test]
fn rule_contains_and_not_contains() {
    let conn = setup_db();
    genre_fixture(&conn);
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"contains","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1", "T3"],
        "contains: Rock + Rocket",
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"not_contains","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T2", "T4"],
        "not_contains: Jazz + empty",
    );
}

#[test]
fn rule_equals_and_not_equals() {
    let conn = setup_db();
    genre_fixture(&conn);
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"equals","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1"],
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"not_equals","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T2", "T3", "T4"],
    );
}

#[test]
fn rule_starts_with_and_ends_with() {
    let conn = setup_db();
    genre_fixture(&conn);
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"starts_with","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1", "T3"],
        "starts_with: Rock + Rocket",
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"ends_with","value":"ck"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1"],
        "ends_with ck: Rock only (Rocket ends 'et')",
    );
}

#[test]
fn rule_greater_and_less_numeric() {
    let conn = setup_db();
    genre_fixture(&conn);
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"year","operator":"greater","value":"2000"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T2"],
        "year > 2000: only 2010",
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"year","operator":"less","value":"2000"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1", "T3"],
        "year < 2000: the two 1990s",
    );
}

#[test]
fn rule_is_empty_and_is_not_empty() {
    let conn = setup_db();
    genre_fixture(&conn);
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"is_empty","value":""}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T4"],
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"is_not_empty","value":""}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1", "T2", "T3"],
    );
}

#[test]
fn rule_logic_or_unions_conditions() {
    let conn = setup_db();
    genre_fixture(&conn);
    // OR of two exact-match rules that no single track satisfies under AND.
    let rules = r#"[{"field":"genre","operator":"equals","value":"Rock"},{"field":"genre","operator":"equals","value":"Jazz"}]"#;
    assert_eq!(
        rule_titles(&conn, rules, "OR", Some("title"), "ASC", 0),
        ["T1", "T2"],
        "OR unions Rock and Jazz",
    );
    assert!(
        rule_titles(&conn, rules, "AND", Some("title"), "ASC", 0).is_empty(),
        "AND of two distinct exact genres matches nothing",
    );
}

#[test]
fn sort_dir_desc_reverses_order() {
    let conn = setup_db();
    genre_fixture(&conn);
    let rules = r#"[{"field":"genre","operator":"is_not_empty","value":""}]"#;
    assert_eq!(
        rule_titles(&conn, rules, "AND", Some("title"), "DESC", 0),
        ["T3", "T2", "T1"],
        "DESC must reverse the ASC order",
    );
}

#[test]
fn max_tracks_limits_only_when_positive() {
    let conn = setup_db();
    genre_fixture(&conn);
    // genre contains "" matches every row (LIKE '%%').
    let rules = r#"[{"field":"genre","operator":"contains","value":""}]"#;
    assert_eq!(
        rule_titles(&conn, rules, "AND", Some("title"), "ASC", 2).len(),
        2,
        "max_tracks=2 caps the result",
    );
    assert_eq!(
        rule_titles(&conn, rules, "AND", Some("title"), "ASC", 0).len(),
        4,
        "max_tracks=0 means no limit (all four rows)",
    );
}

#[test]
fn sort_field_resolves_to_a_real_column() {
    let conn = setup_db();
    // title order (Aaa, Zzz) deliberately differs from play_count order.
    insert_full(&conn, 1, "Aaa", "A", "X", 0, 9, 100);
    insert_full(&conn, 2, "Zzz", "A", "X", 0, 1, 100);
    let rules = r#"[{"field":"genre","operator":"equals","value":"X"}]"#;
    assert_eq!(
        rule_titles(&conn, rules, "AND", Some("play_count"), "ASC", 0),
        ["Zzz", "Aaa"],
        "sorting by play_count ASC must order by play_count, not fall back to title",
    );
}

#[test]
fn unknown_rule_field_is_rejected_no_sql_injection() {
    // execute_smart_playlist_rules interpolates `field` straight into SQL; the
    // valid_rule_fields() allowlist is the ONLY guard. An off-list / malicious
    // field must be dropped (not executed), the table must survive, and a
    // legitimate field must still filter (proving the allowlist check itself
    // is not inverted).
    let conn = setup_db();
    genre_fixture(&conn);
    let malicious =
        r#"[{"field":"title); DROP TABLE tracks;--","operator":"contains","value":"x"}]"#;
    assert!(
        rule_titles(&conn, malicious, "AND", Some("title"), "ASC", 0).is_empty(),
        "an off-allowlist field must yield no conditions, never inject SQL",
    );
    assert_eq!(
        get_recently_added(&conn, 10).unwrap().len(),
        4,
        "the tracks table must still exist (no injection executed)",
    );
    assert_eq!(
        rule_titles(
            &conn,
            r#"[{"field":"genre","operator":"equals","value":"Rock"}]"#,
            "AND",
            Some("title"),
            "ASC",
            0,
        ),
        ["T1"],
        "a valid field must still match (the allowlist check is not inverted)",
    );
}
