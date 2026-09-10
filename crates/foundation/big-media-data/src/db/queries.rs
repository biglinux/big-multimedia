//! Database query functions — CRUD operations for tracks, playlists,
//! watch folders, smart playlists, and play history.

use rusqlite::{Connection, Row, params};

use super::models::{Playlist, Track, WatchFolder};

// Re-exports keep `queries::` as the single import point for callers.
pub use super::queries_advanced::{
    create_smart_playlist, delete_radio_station, delete_smart_playlist,
    execute_smart_playlist_rules, get_all_radio_stations, get_compilation_albums,
    get_duplicate_tracks, get_library_groups, get_library_track_count, get_library_tracks,
    get_most_played, get_never_played, get_recently_added, get_smart_playlists,
    insert_radio_station, load_queue, migrate_radio_from_json, save_queue, update_radio_station,
    update_track_metadata_from_mb, update_track_rating,
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
    // Upsert keeps date_added across re-scans — INSERT OR REPLACE resets it.
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

pub fn get_all_tracks(conn: &Connection) -> crate::Result<Vec<Track>> {
    let sql =
        format!("SELECT {TRACK_COLUMNS} FROM tracks ORDER BY artist, album, track_number, title");
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map([], row_to_track)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(tracks)
}

pub fn search_tracks(conn: &Connection, query: &str) -> crate::Result<Vec<Track>> {
    let pattern = format!("%{query}%");
    let sql = format!(
        "SELECT {TRACK_COLUMNS} FROM tracks
         WHERE title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1
         ORDER BY artist, album, track_number
         LIMIT 500"
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map([&pattern], row_to_track)?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(tracks)
}

pub fn increment_play_count(conn: &Connection, track_id: i64) -> crate::Result<()> {
    conn.execute(
        "UPDATE tracks SET play_count = play_count + 1, last_played = strftime('%s','now') WHERE id = ?1",
        params![track_id],
    )?;
    Ok(())
}

pub fn delete_track(conn: &Connection, track_id: i64) -> crate::Result<()> {
    conn.execute("DELETE FROM tracks WHERE id = ?1", params![track_id])?;
    Ok(())
}

pub fn get_track_by_path(conn: &Connection, path: &str) -> crate::Result<Option<Track>> {
    let sql = format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE file_path = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let track = stmt.query_row([path], row_to_track).ok();
    Ok(track)
}

pub fn get_all_file_paths(conn: &Connection) -> crate::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT file_path FROM tracks")?;
    let paths: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(std::result::Result::ok)
        .collect();
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
    let folders = stmt
        .query_map([], |row| {
            Ok(WatchFolder {
                id: row.get(0)?,
                path: row.get(1)?,
                enabled: row.get::<_, i32>(2)? != 0,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(folders)
}

pub fn remove_watch_folder(conn: &Connection, folder_id: i64) -> crate::Result<()> {
    conn.execute(
        "DELETE FROM watch_folders WHERE id = ?1",
        params![folder_id],
    )?;
    Ok(())
}

/// Preserves tracks under nested watch folders that are still monitored.
pub fn remove_tracks_in_folder(conn: &Connection, folder_path: &str) -> crate::Result<usize> {
    let prefix = if folder_path.ends_with('/') {
        folder_path.to_string()
    } else {
        format!("{folder_path}/")
    };

    let other_folders = get_watch_folders(conn)?;
    let preserve_prefixes: Vec<String> = other_folders
        .iter()
        .filter(|f| {
            let fp = if f.path.ends_with('/') {
                f.path.clone()
            } else {
                format!("{}/", f.path)
            };
            fp != prefix && fp.starts_with(&prefix)
        })
        .map(|f| {
            if f.path.ends_with('/') {
                format!("{}%", f.path)
            } else {
                format!("{}/%", f.path)
            }
        })
        .collect();

    if preserve_prefixes.is_empty() {
        let count = conn.execute(
            "DELETE FROM tracks WHERE file_path LIKE ?1",
            params![format!("{prefix}%")],
        )?;
        Ok(count)
    } else {
        let mut sql = String::from("DELETE FROM tracks WHERE file_path LIKE ?1");
        for i in 0..preserve_prefixes.len() {
            sql.push_str(&format!(" AND file_path NOT LIKE ?{}", i + 2));
        }

        let mut stmt = conn.prepare(&sql)?;

        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params_vec.push(Box::new(format!("{prefix}%")));
        for p in &preserve_prefixes {
            params_vec.push(Box::new(p.clone()));
        }
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(std::convert::AsRef::as_ref).collect();

        let count = stmt.execute(params_refs.as_slice())?;
        Ok(count)
    }
}

pub fn create_playlist(conn: &Connection, name: &str) -> crate::Result<i64> {
    conn.execute("INSERT INTO playlists (name) VALUES (?1)", params![name])?;
    Ok(conn.last_insert_rowid())
}

/// Returns (track_count, total_duration_secs) for a playlist.
pub fn get_playlist_stats(conn: &Connection, playlist_id: i64) -> crate::Result<(i64, f64)> {
    // caveman: ? lifts rusqlite::Error into crate::Error via #[from]
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
        .filter_map(std::result::Result::ok)
        .collect();
    Ok(playlists)
}

pub fn add_track_to_playlist(
    conn: &Connection,
    playlist_id: i64,
    track_id: i64,
) -> crate::Result<()> {
    let max_pos: i32 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        params![playlist_id, track_id, max_pos + 1],
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
        .filter_map(std::result::Result::ok)
        .collect();
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
    conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
    )?;
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![playlist_id])?;
    Ok(())
}

pub fn remove_track_from_playlist(
    conn: &Connection,
    playlist_id: i64,
    track_id: i64,
) -> crate::Result<()> {
    conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        params![playlist_id, track_id],
    )?;
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
    fn insert_and_get_track() {
        let conn = setup_db();
        let track = sample_track("/music/test.mp3");
        let id = insert_track(&conn, &track).unwrap();
        assert!(id > 0);

        let all = get_all_tracks(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].file_path, "/music/test.mp3");
        assert_eq!(all[0].title.as_deref(), Some("Test Song"));
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
    fn search_tracks_by_title() {
        let conn = setup_db();
        let mut t = sample_track("/music/song.mp3");
        t.title = Some("Bohemian Rhapsody".into());
        insert_track(&conn, &t).unwrap();

        let results = search_tracks(&conn, "bohemian").unwrap();
        assert_eq!(results.len(), 1);

        let results = search_tracks(&conn, "nothing").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn delete_track_removes_it() {
        let conn = setup_db();
        let id = insert_track(&conn, &sample_track("/music/del.mp3")).unwrap();
        delete_track(&conn, id).unwrap();
        assert!(get_all_tracks(&conn).unwrap().is_empty());
    }

    #[test]
    fn increment_play_count_works() {
        let conn = setup_db();
        let id = insert_track(&conn, &sample_track("/music/play.mp3")).unwrap();
        increment_play_count(&conn, id).unwrap();
        increment_play_count(&conn, id).unwrap();

        let track = get_track_by_path(&conn, "/music/play.mp3")
            .unwrap()
            .unwrap();
        assert_eq!(track.play_count, 2);
        assert!(track.last_played.is_some());
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

        update_radio_station(
            &conn,
            stations[0].id,
            "Updated FM",
            "https://stream.test.fm/v2",
            "",
            "Jazz",
        )
        .unwrap();
        let updated = get_all_radio_stations(&conn).unwrap();
        assert_eq!(updated[0].name, "Updated FM");
        assert_eq!(updated[0].group_name, "Jazz");

        delete_radio_station(&conn, stations[1].id).unwrap();
        let after_delete = get_all_radio_stations(&conn).unwrap();
        assert_eq!(after_delete.len(), 1);
    }

    #[test]
    fn remove_tracks_in_folder_deletes_and_counts() {
        let conn = setup_db();
        insert_track(&conn, &sample_track("/music/a.mp3")).unwrap();
        insert_track(&conn, &sample_track("/music/b.mp3")).unwrap();
        insert_track(&conn, &sample_track("/other/c.mp3")).unwrap();
        let removed = remove_tracks_in_folder(&conn, "/music").unwrap();
        assert_eq!(removed, 2, "both /music tracks removed");
        assert_eq!(get_all_file_paths(&conn).unwrap(), vec!["/other/c.mp3"]);
    }

    #[test]
    fn remove_tracks_in_folder_preserves_nested_watch_folder() {
        // A still-monitored nested watch folder's tracks must survive the parent
        // removal (the `fp != prefix && fp.starts_with(prefix)` preserve rule).
        let conn = setup_db();
        add_watch_folder(&conn, "/music").unwrap();
        add_watch_folder(&conn, "/music/keep").unwrap();
        insert_track(&conn, &sample_track("/music/loose.mp3")).unwrap();
        insert_track(&conn, &sample_track("/music/keep/saved.mp3")).unwrap();
        let removed = remove_tracks_in_folder(&conn, "/music").unwrap();
        assert_eq!(removed, 1, "only the loose track is removed");
        assert_eq!(
            get_all_file_paths(&conn).unwrap(),
            vec!["/music/keep/saved.mp3"]
        );
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
    fn remove_track_from_playlist_drops_only_that_track() {
        let conn = setup_db();
        let t1 = insert_track(&conn, &sample_track("/m/a.mp3")).unwrap();
        let t2 = insert_track(&conn, &sample_track("/m/b.mp3")).unwrap();
        let pl = create_playlist(&conn, "P").unwrap();
        add_track_to_playlist(&conn, pl, t1).unwrap();
        add_track_to_playlist(&conn, pl, t2).unwrap();
        remove_track_from_playlist(&conn, pl, t1).unwrap();
        let remaining = get_playlist_tracks(&conn, pl).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_path, "/m/b.mp3");
    }
}
