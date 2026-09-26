use rusqlite::{Connection, params};

use super::{insert_minimal_track, setup_db};
use crate::db::queries_advanced::load_queue;

/// Seed the legacy single-queue table that `load_queue` still migrates from.
fn seed_legacy_queue(conn: &Connection, track_ids: &[i64]) {
    for (position, track_id) in track_ids.iter().enumerate() {
        conn.execute(
            "INSERT INTO queue_tracks (position, track_id) VALUES (?1, ?2)",
            params![position as i64, track_id],
        )
        .unwrap();
    }
}

#[test]
fn legacy_queue_loads_in_saved_order() {
    let conn = setup_db();
    insert_minimal_track(&conn, 1, "Song", "Art");

    seed_legacy_queue(&conn, &[1]);
    let tracks = load_queue(&conn).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].id, 1);
}

#[test]
fn session_roundtrips_generated_orders_duplicates_and_external_paths() {
    use crate::db::models::Track;
    use crate::db::queries_advanced::{
        QueueSession, SavedQueue, load_queue_session, save_queue_session,
    };
    let conn = setup_db();
    conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    insert_minimal_track(&conn, 1, "Library", "Artist");
    for seed in 0..80 {
        let tracks: Vec<_> = (0..seed % 19)
            .map(|position| {
                let mut track = Track::new_external_path(format!(
                    "/música/{} ' faixa.flac",
                    (position * 17 + seed) % 7
                ));
                track.title = Some(format!("Título {position}"));
                track.duration_secs = f64::from(position * 3);
                track
            })
            .collect();
        let current = (!tracks.is_empty()).then(|| seed as usize % tracks.len());
        let podcast =
            Track::new_podcast_episode(-1, "https://example.org/episode.mp3", "Episode", "Feed", 1);
        let session = QueueSession {
            music: SavedQueue { tracks, current },
            podcasts: SavedQueue {
                tracks: vec![podcast],
                current: Some(0),
            },
        };
        save_queue_session(&conn, &session).unwrap();
        let restored = load_queue_session(&conn).unwrap();
        for (before, after) in [
            (&session.music, &restored.music),
            (&session.podcasts, &restored.podcasts),
        ] {
            assert_eq!(before.current, after.current);
            assert_eq!(before.tracks.len(), after.tracks.len());
            for (before, after) in before.tracks.iter().zip(&after.tracks) {
                assert_eq!(
                    (
                        &before.file_path,
                        &before.title,
                        &before.artist,
                        &before.album,
                        before.duration_secs,
                        before.id
                    ),
                    (
                        &after.file_path,
                        &after.title,
                        &after.artist,
                        &after.album,
                        after.duration_secs,
                        after.id
                    )
                );
            }
        }
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[test]
fn legacy_queue_is_read_once_and_failed_session_save_keeps_both_queues() {
    use crate::db::models::Track;
    use crate::db::queries_advanced::{
        QueueSession, SavedQueue, load_queue_session, save_queue_session,
    };
    let conn = setup_db();
    insert_minimal_track(&conn, 1, "Library", "Artist");
    seed_legacy_queue(&conn, &[1, 1]);
    let mut session = load_queue_session(&conn).unwrap();
    assert_eq!(session.music.tracks.len(), 2);
    session.podcasts = SavedQueue {
        tracks: vec![Track::new_podcast_episode(
            -1,
            "https://example.org/a.mp3",
            "A",
            "Feed",
            1,
        )],
        current: Some(0),
    };
    save_queue_session(&conn, &session).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_queue BEFORE INSERT ON playback_queue_items
                        WHEN NEW.queue_name='podcasts' BEGIN SELECT RAISE(ABORT, 'disk failure'); END;").unwrap();
    session.music.tracks.clear();
    session.music.current = None;
    assert!(save_queue_session(&conn, &session).is_err());
    // A failed save must not leave the shared connection on FULL (1 = NORMAL).
    let synchronous: i64 = conn
        .pragma_query_value(None, "synchronous", |row| row.get(0))
        .unwrap();
    assert_eq!(synchronous, 1);
    let restored = load_queue_session(&conn).unwrap();
    assert_eq!(restored.music.tracks.len(), 2);
    assert_eq!(restored.podcasts.tracks[0].title.as_deref(), Some("A"));
    conn.execute_batch("DROP TRIGGER fail_queue").unwrap();
    save_queue_session(&conn, &QueueSession::default()).unwrap();
    crate::db::schema::run_migrations(&conn).unwrap();
    assert!(load_queue_session(&conn).unwrap().music.tracks.is_empty());
    assert_eq!(load_queue(&conn).unwrap().len(), 2);
}
