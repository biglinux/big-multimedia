//! Database query functions: CRUD operations for tracks, playlists,
//! watch folders, smart playlists, and play history.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::models::{Playlist, Track, WatchFolder};

// Re-exports keep `queries::` as the single import point for callers.
pub use super::queries_advanced::{
    delete_radio_station, get_all_radio_stations, get_compilation_albums, get_library_groups,
    get_library_track_count, get_library_tracks, get_most_played, get_never_played,
    get_recently_added, insert_radio_station, load_queue, migrate_radio_from_json,
    update_track_rating,
};

pub(super) fn row_to_track(row: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: row.get(0)?,
        file_path: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        album: row.get(4)?,
        album_artist: row.get(5)?,
        track_number: row.get(6)?,
        disc_number: row.get(7)?,
        year: row.get(8)?,
        genre: row.get(9)?,
        duration_secs: row.get(10)?,
        file_size: row.get(11)?,
        bitrate: row.get(12)?,
        sample_rate: row.get(13)?,
        channels: row.get(14)?,
        format: row.get(15)?,
        art_hash: row.get(16)?,
        date_added: row.get(17)?,
        last_played: row.get(18)?,
        play_count: row.get(19)?,
        rating: row.get(20)?,
        modified_at: row.get(21)?,
    })
}

pub(super) const TRACK_COLUMNS: &str =
    "id, file_path, title, artist, album, album_artist, track_number,
     disc_number, year, genre, duration_secs, file_size, bitrate,
     sample_rate, channels, format, art_hash, date_added, last_played,
     play_count, rating, modified_at";

/// Same columns prefixed with `t.` for JOINed queries.
pub(super) const TRACK_COLUMNS_PREFIXED: &str =
    "t.id, t.file_path, t.title, t.artist, t.album, t.album_artist, t.track_number,
     t.disc_number, t.year, t.genre, t.duration_secs, t.file_size, t.bitrate,
     t.sample_rate, t.channels, t.format, t.art_hash, t.date_added, t.last_played,
     t.play_count, t.rating, t.modified_at";

pub fn insert_track(conn: &Connection, t: &Track) -> crate::Result<i64> {
    // Upsert keeps date_added across re-scans; INSERT OR REPLACE would reset it.
    conn.execute(
        "INSERT INTO tracks
            (file_path, title, artist, album, album_artist, track_number, disc_number,
             year, genre, duration_secs, file_size, bitrate, sample_rate, channels,
             format, art_hash, modified_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
         ON CONFLICT(file_path) DO UPDATE SET
             title = excluded.title,
             artist = excluded.artist,
             album = excluded.album,
             album_artist = excluded.album_artist,
             track_number = excluded.track_number,
             disc_number = excluded.disc_number,
             year = excluded.year,
             genre = excluded.genre,
             duration_secs = excluded.duration_secs,
             file_size = excluded.file_size,
             bitrate = excluded.bitrate,
             sample_rate = excluded.sample_rate,
             channels = excluded.channels,
             format = excluded.format,
             art_hash = excluded.art_hash,
             modified_at = excluded.modified_at",
        params![
            t.file_path,
            t.title,
            t.artist,
            t.album,
            t.album_artist,
            t.track_number,
            t.disc_number,
            t.year,
            t.genre,
            t.duration_secs,
            t.file_size,
            t.bitrate,
            t.sample_rate,
            t.channels,
            t.format,
            t.art_hash,
            t.modified_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_track(conn: &Connection, track_id: i64) -> crate::Result<()> {
    conn.execute("DELETE FROM tracks WHERE id = ?1", params![track_id])?;
    Ok(())
}

pub fn get_track_by_path(conn: &Connection, path: &str) -> crate::Result<Option<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE file_path = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let track = stmt.query_row([path], row_to_track).optional()?;
    Ok(track)
}

pub fn get_all_file_paths(conn: &Connection) -> crate::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT file_path FROM tracks")?;
    let paths: Vec<String> = crate::db::decoded_rows(stmt.query_map([], |row| row.get(0))?);
    Ok(paths)
}

pub fn add_watch_folder(conn: &Connection, path: &str) -> crate::Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO watch_folders (path) VALUES (?1)",
        params![path],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_watch_folders(conn: &Connection) -> crate::Result<Vec<WatchFolder>> {
    let mut stmt = conn.prepare("SELECT id, path, enabled FROM watch_folders ORDER BY path")?;
    let folders = crate::db::decoded_rows(stmt.query_map([], |row| {
        Ok(WatchFolder {
            id: row.get(0)?,
            path: row.get(1)?,
            enabled: row.get::<_, i32>(2)? != 0,
        })
    })?);
    Ok(folders)
}

pub fn remove_watch_folder(conn: &Connection, folder_id: i64) -> crate::Result<()> {
    conn.execute(
        "DELETE FROM watch_folders WHERE id = ?1",
        params![folder_id],
    )?;
    Ok(())
}

pub fn create_playlist(conn: &Connection, name: &str) -> crate::Result<i64> {
    conn.execute("INSERT INTO playlists (name) VALUES (?1)", params![name])?;
    Ok(conn.last_insert_rowid())
}

/// Create a whole playlist in one transaction: all tracks, or no new playlist.
/// Duplicate ids in a queue are collapsed in first-occurrence order because the
/// persisted playlist schema permits one entry per track. No nested transaction.
pub fn create_playlist_with_tracks(
    conn: &Connection,
    name: &str,
    ids: &[i64],
) -> crate::Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let playlist_id = create_playlist(&tx, name)?;
    {
        let mut statement = tx.prepare(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        )?;
        let mut seen = std::collections::HashSet::new();
        let mut position = 0_i64;
        for &track_id in ids {
            if seen.insert(track_id) {
                statement.execute(params![playlist_id, track_id, position])?;
                position += 1;
            }
        }
    }
    tx.commit()?;
    Ok(playlist_id)
}

/// Returns (track_count, total_duration_secs) for a playlist.
pub fn get_playlist_stats(conn: &Connection, playlist_id: i64) -> crate::Result<(i64, f64)> {
    let row = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(t.duration_secs), 0) \
         FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id \
         WHERE pt.playlist_id = ?1",
        params![playlist_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(row)
}

pub fn get_all_playlists(conn: &Connection) -> crate::Result<Vec<Playlist>> {
    let mut stmt = conn.prepare("SELECT id, name, created_at FROM playlists ORDER BY name")?;
    let playlists = stmt
        .query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(playlists)
}

pub fn add_track_to_playlist(
    conn: &Connection,
    playlist_id: i64,
    track_id: i64,
) -> crate::Result<()> {
    // One SQL statement avoids racing a separate MAX(position) read and insert.
    conn.execute(
        "INSERT INTO playlist_tracks (playlist_id, track_id, position)
         SELECT ?1, ?2, COALESCE(MAX(position), -1) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id, track_id],
    )?;
    Ok(())
}

pub fn get_playlist_tracks(conn: &Connection, playlist_id: i64) -> crate::Result<Vec<Track>> {
    let sql = format!(
        "SELECT {TRACK_COLUMNS_PREFIXED} FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![playlist_id], row_to_track)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tracks)
}

pub fn rename_playlist(conn: &Connection, playlist_id: i64, new_name: &str) -> crate::Result<()> {
    conn.execute(
        "UPDATE playlists SET name = ?1 WHERE id = ?2",
        params![new_name, playlist_id],
    )?;
    Ok(())
}

pub fn delete_playlist(conn: &Connection, playlist_id: i64) -> crate::Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
    )?;
    tx.execute("DELETE FROM playlists WHERE id = ?1", params![playlist_id])?;
    tx.commit()?;
    Ok(())
}

// These tests exercise rusqlite against an in-memory SQLite database. Miri
// cannot execute libsqlite3 FFI, so the ordinary test gate owns this coverage.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use crate::db::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    fn sample_track(path: &str) -> Track {
        Track {
            id: 0,
            file_path: path.to_string(),
            title: Some("Test Song".into()),
            artist: Some("Test Artist".into()),
            album: Some("Test Album".into()),
            album_artist: None,
            track_number: Some(1),
            disc_number: None,
            year: Some(2024),
            genre: Some("Rock".into()),
            duration_secs: 180.0,
            file_size: Some(5_000_000),
            bitrate: Some(320),
            sample_rate: Some(44100),
            channels: Some(2),
            format: Some("MP3".into()),
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    #[test]
    fn get_track_by_path_found_and_missing() {
        let conn = setup_db();
        insert_track(&conn, &sample_track("/music/a.flac")).unwrap();

        assert!(get_track_by_path(&conn, "/music/a.flac").unwrap().is_some());
        assert!(
            get_track_by_path(&conn, "/music/missing.flac")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn watch_folders_crud() {
        let conn = setup_db();
        let id = add_watch_folder(&conn, "/home/user/Music").unwrap();
        assert!(id > 0);

        let folders = get_watch_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].path, "/home/user/Music");
        assert!(folders[0].enabled);

        remove_watch_folder(&conn, folders[0].id).unwrap();
        assert!(get_watch_folders(&conn).unwrap().is_empty());
    }

    #[test]
    fn playlist_with_tracks() {
        let conn = setup_db();
        let t1 = insert_track(&conn, &sample_track("/music/one.mp3")).unwrap();
        let t2 = insert_track(&conn, &sample_track("/music/two.mp3")).unwrap();
        let pl = create_playlist(&conn, "My Playlist").unwrap();

        add_track_to_playlist(&conn, pl, t1).unwrap();
        add_track_to_playlist(&conn, pl, t2).unwrap();

        let tracks = get_playlist_tracks(&conn, pl).unwrap();
        assert_eq!(tracks.len(), 2);

        let playlists = get_all_playlists(&conn).unwrap();
        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].name, "My Playlist");
    }

    #[test]
    fn get_all_file_paths_returns_all() {
        let conn = setup_db();
        insert_track(&conn, &sample_track("/a.mp3")).unwrap();
        insert_track(&conn, &sample_track("/b.flac")).unwrap();
        let paths = get_all_file_paths(&conn).unwrap();
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn playlist_stats_count_and_total_duration() {
        let conn = setup_db();
        let mut t1 = sample_track("/m/1.mp3");
        t1.duration_secs = 10.0;
        let mut t2 = sample_track("/m/2.mp3");
        t2.duration_secs = 20.0;
        let id1 = insert_track(&conn, &t1).unwrap();
        let id2 = insert_track(&conn, &t2).unwrap();
        let pl = create_playlist(&conn, "P").unwrap();
        add_track_to_playlist(&conn, pl, id1).unwrap();
        add_track_to_playlist(&conn, pl, id2).unwrap();
        let (count, total) = get_playlist_stats(&conn, pl).unwrap();
        assert_eq!(count, 2);
        assert!((total - 30.0).abs() < 1e-9, "total duration = {total}");
    }

    #[test]
    fn rename_playlist_changes_name() {
        let conn = setup_db();
        let pl = create_playlist(&conn, "Old").unwrap();
        rename_playlist(&conn, pl, "New").unwrap();
        let pls = get_all_playlists(&conn).unwrap();
        assert_eq!(pls.len(), 1);
        assert_eq!(pls[0].name, "New");
    }

    #[test]
    fn delete_playlist_removes_playlist_and_its_tracks() {
        let conn = setup_db();
        let tid = insert_track(&conn, &sample_track("/m/x.mp3")).unwrap();
        let pl = create_playlist(&conn, "Doomed").unwrap();
        add_track_to_playlist(&conn, pl, tid).unwrap();
        delete_playlist(&conn, pl).unwrap();
        assert!(get_all_playlists(&conn).unwrap().is_empty());
        assert!(get_playlist_tracks(&conn, pl).unwrap().is_empty());
    }

    #[test]
    fn insert_and_get_track() {
        let conn = setup_db();
        let track = sample_track("/music/test.mp3");
        let id = insert_track(&conn, &track).unwrap();
        assert!(id > 0);

        let stored = get_track_by_path(&conn, "/music/test.mp3")
            .unwrap()
            .expect("inserted track");
        assert_eq!(stored.id, id);
        assert_eq!(stored.title.as_deref(), Some("Test Song"));
    }

    #[test]
    fn delete_track_removes_it() {
        let conn = setup_db();
        let id = insert_track(&conn, &sample_track("/music/del.mp3")).unwrap();
        delete_track(&conn, id).unwrap();
        assert!(
            get_track_by_path(&conn, "/music/del.mp3")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn radio_station_crud() {
        let conn = setup_db();

        let id = insert_radio_station(&conn, "Test FM", "https://stream.test.fm/live", "", "Music")
            .unwrap();
        assert!(id > 0);

        let _ = insert_radio_station(
            &conn,
            "News24",
            "https://news24.example.com/stream",
            "https://news24.example.com/icon.png",
            "News",
        )
        .unwrap();

        let stations = get_all_radio_stations(&conn).unwrap();
        assert_eq!(stations.len(), 2);
        assert_eq!(stations[0].name, "Test FM");
        assert_eq!(stations[0].group_name, "Music");
        assert_eq!(stations[1].name, "News24");
        assert_eq!(stations[1].favicon, "https://news24.example.com/icon.png");

        delete_radio_station(&conn, stations[1].id).unwrap();
        let after_delete = get_all_radio_stations(&conn).unwrap();
        assert_eq!(after_delete.len(), 1);
    }
}

#[cfg(test)]
mod playlist_atomicity_tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        super::super::schema::run_migrations(&conn).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;
            INSERT INTO tracks (file_path, title) VALUES ('/one.flac', 'One'), ('/two.flac', 'Two');").unwrap();
        conn
    }

    #[test]
    fn invalid_track_rolls_back_playlist() {
        let conn = database();
        assert!(create_playlist_with_tracks(&conn, "Failed", &[1, 999999]).is_err());
        assert!(get_all_playlists(&conn).unwrap().is_empty());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlist_tracks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn batch_dedups_keeping_first_order() {
        let conn = database();
        let id = create_playlist_with_tracks(&conn, "Saved", &[2, 1, 2]).unwrap();
        let ids: Vec<_> = get_playlist_tracks(&conn, id)
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, [2, 1]);
    }

    #[test]
    fn failed_delete_keeps_playlist_and_membership() {
        let conn = database();
        let id = create_playlist_with_tracks(&conn, "Kept", &[1, 2]).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER block_delete BEFORE DELETE ON playlists
            BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
        assert!(delete_playlist(&conn, id).is_err());
        assert_eq!(get_playlist_tracks(&conn, id).unwrap().len(), 2);
        assert_eq!(get_all_playlists(&conn).unwrap().len(), 1);
    }
}
