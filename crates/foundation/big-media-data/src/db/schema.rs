//! Database schema and migrations for the SQLite library database.

use rusqlite::Connection;

/// Ordered upgrades on top of the idempotent baseline below;
/// `PRAGMA user_version` records how many have run. Append only: a released
/// step never changes, and a column added to a baseline table needs its own
/// `ALTER TABLE` step here so existing databases get it too.
const MIGRATIONS: &[&str] = &[
    // 1: `file_path ... UNIQUE` already carries an index; this one only
    // doubled the write cost of every scan.
    "DROP INDEX IF EXISTS idx_tracks_file_path;",
    // 2: the full-text index covers five text columns, yet every play-count,
    // rating or last-played update deleted and re-added the track's entry.
    // Deleting a track cascades into these two tables, which had no index on
    // `track_id`, so each deletion scanned both.
    "DROP TRIGGER IF EXISTS tracks_fts_au;
     CREATE TRIGGER tracks_fts_au AFTER UPDATE OF title, artist, album, album_artist, genre
     ON tracks BEGIN
         INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
         VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
         INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
         VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
     END;
     CREATE INDEX IF NOT EXISTS idx_playlist_tracks_track ON playlist_tracks(track_id);
     CREATE INDEX IF NOT EXISTS idx_queue_tracks_track ON queue_tracks(track_id);",
];

pub fn run_migrations(conn: &Connection) -> crate::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS tracks (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path     TEXT NOT NULL UNIQUE,
            title         TEXT,
            artist        TEXT,
            album         TEXT,
            album_artist  TEXT,
            track_number  INTEGER,
            disc_number   INTEGER,
            year          INTEGER,
            genre         TEXT,
            duration_secs REAL NOT NULL DEFAULT 0,
            file_size     INTEGER,
            bitrate       INTEGER,
            sample_rate   INTEGER,
            channels      INTEGER,
            format        TEXT,
            art_hash      TEXT,
            date_added    INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            last_played   INTEGER,
            play_count    INTEGER NOT NULL DEFAULT 0,
            rating        INTEGER NOT NULL DEFAULT 0,
            modified_at   INTEGER
        );

        CREATE TABLE IF NOT EXISTS playlists (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT NOT NULL UNIQUE,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS playlist_tracks (
            playlist_id INTEGER NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
            track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
            position    INTEGER NOT NULL,
            PRIMARY KEY (playlist_id, track_id)
        );

        CREATE TABLE IF NOT EXISTS watch_folders (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            path     TEXT NOT NULL UNIQUE,
            enabled  INTEGER NOT NULL DEFAULT 1
        );

        CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
        CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
        CREATE INDEX IF NOT EXISTS idx_tracks_genre ON tracks(genre);
        CREATE INDEX IF NOT EXISTS idx_playlist_tracks_playlist ON playlist_tracks(playlist_id, position);

        CREATE TABLE IF NOT EXISTS smart_playlists (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            name         TEXT NOT NULL UNIQUE,
            rules_json   TEXT NOT NULL,
            logic        TEXT NOT NULL DEFAULT 'AND',
            sort_field   TEXT,
            sort_dir     TEXT DEFAULT 'ASC',
            max_tracks   INTEGER DEFAULT 0,
            created_at   INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS queue_tracks (
            position    INTEGER NOT NULL PRIMARY KEY,
            track_id    INTEGER NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS playback_queues (
            name TEXT NOT NULL PRIMARY KEY CHECK (name IN ('music', 'podcasts')),
            current_position INTEGER CHECK (current_position >= 0)
        );
        CREATE TABLE IF NOT EXISTS playback_queue_items (
            queue_name TEXT NOT NULL REFERENCES playback_queues(name) ON DELETE CASCADE,
            position INTEGER NOT NULL CHECK (position >= 0),
            file_path TEXT NOT NULL CHECK (length(file_path) > 0),
            track_id INTEGER NOT NULL,
            title TEXT,
            artist TEXT,
            album TEXT,
            duration_secs REAL NOT NULL CHECK (duration_secs >= 0),
            PRIMARY KEY (queue_name, position)
        );

        CREATE TABLE IF NOT EXISTS radio_groups (
            id    INTEGER PRIMARY KEY AUTOINCREMENT,
            name  TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS radio_stations (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL,
            url        TEXT NOT NULL UNIQUE,
            favicon    TEXT NOT NULL DEFAULT '',
            group_name TEXT NOT NULL DEFAULT '',
            position   INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_radio_stations_group ON radio_stations(group_name);
        ",
    )?;

    // FTS5 is created separately: some SQLite builds reject `if not exists` on
    // virtual tables.
    let has_fts: bool = conn
        .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name='tracks_fts'")
        .and_then(|mut s| s.exists([]))
        .unwrap_or(false);

    if !has_fts {
        // One transaction: a failure after CREATE VIRTUAL TABLE must not leave the
        // table without its triggers, since `has_fts` would then skip repair forever.
        let transaction = conn.unchecked_transaction()?;
        transaction.execute_batch(
            "
            CREATE VIRTUAL TABLE tracks_fts USING fts5(
                title, artist, album, album_artist, genre,
                content='tracks',
                content_rowid='id'
            );

            CREATE TRIGGER tracks_fts_ai AFTER INSERT ON tracks BEGIN
                INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
                VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
            END;

            CREATE TRIGGER tracks_fts_ad AFTER DELETE ON tracks BEGIN
                INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
                VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
            END;

            CREATE TRIGGER tracks_fts_au AFTER UPDATE ON tracks BEGIN
                INSERT INTO tracks_fts(tracks_fts, rowid, title, artist, album, album_artist, genre)
                VALUES ('delete', old.id, old.title, old.artist, old.album, old.album_artist, old.genre);
                INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
                VALUES (new.id, new.title, new.artist, new.album, new.album_artist, new.genre);
            END;

            -- Populate FTS from existing tracks
            INSERT INTO tracks_fts(rowid, title, artist, album, album_artist, genre)
            SELECT id, title, artist, album, album_artist, genre FROM tracks;
            ",
        )?;
        transaction.commit()?;
    }

    apply_versioned_migrations(conn)
}

fn apply_versioned_migrations(conn: &Connection) -> crate::Result<()> {
    let applied: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // A database written by a newer build keeps its version; never downgrade.
    let Some(pending) = MIGRATIONS.get(usize::try_from(applied).unwrap_or(usize::MAX)..) else {
        return Ok(());
    };
    if pending.is_empty() {
        return Ok(());
    }
    let transaction = conn.unchecked_transaction()?;
    for step in pending {
        transaction.execute_batch(step)?;
    }
    transaction.pragma_update(None, "user_version", MIGRATIONS.len() as i64)?;
    transaction.commit()?;
    Ok(())
}

// These tests exercise rusqlite against an in-memory SQLite database. Miri
// cannot execute libsqlite3 FFI, so the ordinary test gate owns this coverage.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn migration_creates_all_tables() {
        let conn = memory_db();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .collect();

        for expected in [
            "tracks",
            "playlists",
            "playlist_tracks",
            "watch_folders",
            "smart_playlists",
            "queue_tracks",
            "radio_groups",
            "radio_stations",
            "tracks_fts",
        ] {
            assert!(
                tables.contains(&expected.to_string()),
                "missing table: {expected}"
            );
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = memory_db();
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn versioned_steps_upgrade_an_old_database_once() {
        // What a database created before versioning looked like: the full
        // baseline, the old redundant index, and no recorded version.
        let conn = memory_db();
        conn.execute_batch(
            "CREATE INDEX idx_tracks_file_path ON tracks(file_path);
             PRAGMA user_version = 0;",
        )
        .unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let redundant: bool = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE name = 'idx_tracks_file_path'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(!redundant);
    }

    #[test]
    fn newer_database_version_is_left_alone() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        run_migrations(&conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 999);
    }

    #[test]
    fn fts_trigger_populates_on_insert() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO tracks (file_path, title, artist, album, duration_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["/test.mp3", "Song Title", "Artist Name", "Album", 180.0],
        )
        .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'Song'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn fts_tracks_text_edits_only() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO tracks (file_path, title, artist, duration_secs) VALUES ('/a.mp3', 'Old Name', 'Art', 60.0)",
            [],
        )
        .unwrap();
        let matches = |query: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1",
                [query],
                |r| r.get(0),
            )
            .unwrap()
        };
        let changes_before = conn.total_changes();
        conn.execute("UPDATE tracks SET play_count = play_count + 1", [])
            .unwrap();
        // One row changed: the tracks row, and no full-text rewrite.
        assert_eq!(conn.total_changes() - changes_before, 1);
        conn.execute("UPDATE tracks SET title = 'New Name'", [])
            .unwrap();
        assert_eq!(matches("New"), 1);
        assert_eq!(matches("Old"), 0);
    }

    #[test]
    fn fts_trigger_removes_on_delete() {
        let conn = memory_db();
        conn.execute(
            "INSERT INTO tracks (file_path, title, artist, duration_secs) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["/test.mp3", "Unique Title XYZ", "Art", 60.0],
        )
        .unwrap();
        conn.execute("DELETE FROM tracks WHERE file_path = '/test.mp3'", [])
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH 'Unique'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }
}
