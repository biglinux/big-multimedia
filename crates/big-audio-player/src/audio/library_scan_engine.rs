//! Bounded metadata import. A scan never deletes unavailable tracks.
//! One batch has at most 64 records; filesystem metadata is read outside the
//! DB mutex and transaction. Earlier committed batches survive later failures.
use std::path::Path;
use std::sync::PoisonError;
use crate::db::{DbHandle, queries};
use crate::db::models::Track;
use crate::library::{metadata, scanner};

pub(crate) fn scan_folder_blocking(db: &DbHandle, folder: &str) -> Result<(usize, usize), String> {
    if !std::fs::metadata(folder).map_err(|_| "Folder is unavailable")?.is_dir() {
        return Err("Choose an available folder".into());
    }
    {
        let conn = db.lock().map_err(|_| "Library connection failed")?;
        queries::add_watch_folder(&conn, folder).map_err(|_| "Could not save watch folder")?;
    }
    let mut batch: Vec<Track> = Vec::with_capacity(64);
    let mut added = 0; let mut skipped = 0;
    scanner::visit_audio_files(folder, |path| {
        let path = path.to_str().ok_or_else(|| std::io::Error::other("Non-UTF-8 paths require a library schema migration"))?;
        let exists: bool = {
            let conn = db.lock().map_err(|_| std::io::Error::other("Library connection failed"))?;
            conn.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE file_path=?1)", [path], |row| row.get(0))?
        };
        if exists { return Ok(()); }
        match metadata::read_track_metadata(path) {
            Ok(track) => batch.push(track),
            Err(_) => skipped += 1,
        }
        if batch.len() == 64 { added += insert_scan_batch(db, &batch)?; batch.clear(); }
        Ok(())
    }).map_err(|_| "Scan was incomplete; earlier imports were kept".to_owned())?;
    if !batch.is_empty() { added += insert_scan_batch(db, &batch).map_err(|_| "Import was incomplete; earlier batches were kept")?; }
    Ok((added, skipped))
}
fn insert_scan_batch(db: &DbHandle, tracks: &[Track]) -> std::io::Result<usize> {
    let conn = db.lock().map_err(|_| std::io::Error::other("Library connection failed"))?;
    let tx = conn.unchecked_transaction().map_err(|_| std::io::Error::other("Library write failed"))?;
    for track in tracks { queries::insert_track(&tx, track).map_err(|_| std::io::Error::other("Library write failed"))?; }
    tx.commit().map_err(|_| std::io::Error::other("Library commit failed"))?;
    Ok(tracks.len())
}
/// Stop monitoring only. Removing tracks/ratings/playlists is a separate,
/// destructive user decision and must not be inferred from removing a watch.
pub(crate) fn remove_watch_folder_blocking(db: &DbHandle, folder_id: i64) -> bool {
    db.lock().is_ok_and(|conn| queries::remove_watch_folder(&conn, folder_id).is_ok())
}
pub(crate) fn watch_folder_paths_blocking(db: &DbHandle) -> Result<Vec<String>, String> {
    let conn = db.lock().map_err(|_| "Library connection failed")?;
    Ok(queries::get_watch_folders(&conn).map_err(|_| "Could not read watch folders")?
        .into_iter().map(|folder| folder.path).collect())
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use crate::db::models::Track;
    use crate::db::{DbHandle, schema};
    use big_media_runtime::Result as MediaResult;
    use rusqlite::Connection;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn memory_library_db() -> MediaResult<DbHandle> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::run_migrations(&conn)?;
        Ok(Arc::new(Mutex::new(conn)))
    }

    fn unique_audio_scan_folder(
        purpose: &str,
    ) -> std::result::Result<std::path::PathBuf, Box<dyn std::error::Error>> {
        let unique_suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let folder = std::env::temp_dir().join(format!(
            "big-audio-library-scan-{purpose}-{}-{unique_suffix}",
            std::process::id()
        ));
        fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    fn track_for_path(path: &Path) -> Track {
        Track {
            id: 0,
            file_path: path.to_string_lossy().into_owned(),
            title: Some("Existing Track".to_string()),
            artist: Some("Existing Artist".to_string()),
            album: Some("Existing Album".to_string()),
            album_artist: None,
            track_number: Some(1),
            disc_number: None,
            year: Some(2026),
            genre: Some("Test".to_string()),
            duration_secs: 2.0,
            file_size: Some(128),
            bitrate: None,
            sample_rate: Some(8_000),
            channels: Some(1),
            format: Some("MP3".to_string()),
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    fn silent_wav_file_bytes(sample_count: u32) -> Vec<u8> {
        let sample_rate = 8_000_u32;
        let channel_count = 1_u16;
        let bits_per_sample = 16_u16;
        let bytes_per_sample = u32::from(bits_per_sample / 8);
        let sample_chunk_size = sample_count * bytes_per_sample;
        let riff_chunk_size = 36 + sample_chunk_size;

        let mut wav_bytes = Vec::with_capacity(44 + sample_chunk_size as usize);
        wav_bytes.extend_from_slice(b"RIFF");
        wav_bytes.extend_from_slice(&riff_chunk_size.to_le_bytes());
        wav_bytes.extend_from_slice(b"WAVE");
        wav_bytes.extend_from_slice(b"fmt ");
        wav_bytes.extend_from_slice(&16_u32.to_le_bytes());
        wav_bytes.extend_from_slice(&1_u16.to_le_bytes());
        wav_bytes.extend_from_slice(&channel_count.to_le_bytes());
        wav_bytes.extend_from_slice(&sample_rate.to_le_bytes());
        wav_bytes.extend_from_slice(
            &(sample_rate * u32::from(channel_count) * bytes_per_sample).to_le_bytes(),
        );
        wav_bytes.extend_from_slice(&(channel_count * bits_per_sample / 8).to_le_bytes());
        wav_bytes.extend_from_slice(&bits_per_sample.to_le_bytes());
        wav_bytes.extend_from_slice(b"data");
        wav_bytes.extend_from_slice(&sample_chunk_size.to_le_bytes());
        wav_bytes.resize(44 + sample_chunk_size as usize, 0);
        wav_bytes
    }

    #[test]
    fn scan_folder_imports_bounded_batches_and_preserves_unavailable_rows()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let db = memory_library_db()?;
        let scan_folder = unique_audio_scan_folder("mixed")?;
        let valid_track_path = scan_folder.join("valid.wav");
        let invalid_track_path = scan_folder.join("invalid.mp3");
        let second_invalid_track_path = scan_folder.join("invalid.opus");
        let ignored_text_path = scan_folder.join("notes.txt");
        let stale_track_path = scan_folder.join("deleted.mp3");

        fs::write(&valid_track_path, silent_wav_file_bytes(800))?;
        fs::write(&invalid_track_path, [])?;
        fs::write(&second_invalid_track_path, [])?;
        fs::write(&ignored_text_path, b"not audio")?;

        {
            let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
            queries::insert_track(&conn, &track_for_path(&stale_track_path))?;
        }

        let scan_result = scan_folder_blocking(&db, &scan_folder.to_string_lossy())?;
        assert_eq!(
            scan_result,
            (1, 2),
            "scanner should add one valid WAV and skip two invalid audio files"
        );

        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        let track_paths = queries::get_all_file_paths(&conn)?;
        assert!(
            track_paths.contains(&valid_track_path.to_string_lossy().into_owned()),
            "valid WAV file should be inserted into the library"
        );
        assert!(
            track_paths.contains(&stale_track_path.to_string_lossy().into_owned()),
            "unavailable track metadata must be preserved"
        );
        assert!(
            !track_paths.contains(&invalid_track_path.to_string_lossy().into_owned()),
            "invalid MP3 should not be inserted"
        );
        let watch_folders = queries::get_watch_folders(&conn)?;
        assert_eq!(watch_folders.len(), 1);
        assert_eq!(watch_folders[0].path, scan_folder.to_string_lossy());

        drop(conn);
        fs::remove_dir_all(&scan_folder)?;
        Ok(())
    }

    #[test]
    fn watch_folder_paths_returns_every_registered_path_sorted_by_query()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let db = memory_library_db()?;
        let scan_folder = unique_audio_scan_folder("watch-paths")?;
        let album_folder = scan_folder.join("albums");
        fs::create_dir_all(&album_folder)?;

        {
            let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
            queries::add_watch_folder(&conn, &album_folder.to_string_lossy())?;
            queries::add_watch_folder(&conn, &scan_folder.to_string_lossy())?;
        }

        let watch_folder_paths = watch_folder_paths_blocking(&db)?;
        assert_eq!(
            watch_folder_paths,
            vec![
                scan_folder.to_string_lossy().into_owned(),
                album_folder.to_string_lossy().into_owned()
            ]
        );

        fs::remove_dir_all(&scan_folder)?;
        Ok(())
    }

    #[test]
    fn remove_watch_folder_preserves_tracks_and_nested_watch_folder()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let db = memory_library_db()?;
        let scan_folder = unique_audio_scan_folder("remove")?;
        let nested_folder = scan_folder.join("nested");
        fs::create_dir_all(&nested_folder)?;
        let parent_track_path = scan_folder.join("parent.mp3");
        let nested_track_path = nested_folder.join("child.mp3");

        let parent_folder_id;
        let nested_folder_id;
        {
            let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
            parent_folder_id = queries::add_watch_folder(&conn, &scan_folder.to_string_lossy())?;
            nested_folder_id = queries::add_watch_folder(&conn, &nested_folder.to_string_lossy())?;
            queries::insert_track(&conn, &track_for_path(&parent_track_path))?;
            queries::insert_track(&conn, &track_for_path(&nested_track_path))?;
        }

        assert!(
            remove_watch_folder_blocking(&db, parent_folder_id),
            "existing parent watch folder should be removed successfully"
        );

        let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
        let remaining_watch_folders = queries::get_watch_folders(&conn)?;
        assert_eq!(remaining_watch_folders.len(), 1);
        assert_eq!(remaining_watch_folders[0].id, nested_folder_id);
        assert_eq!(
            remaining_watch_folders[0].path,
            nested_folder.to_string_lossy()
        );

        let remaining_track_paths = queries::get_all_file_paths(&conn)?;
        assert!(
            remaining_track_paths.contains(&parent_track_path.to_string_lossy().into_owned()),
            "unwatching a folder does not delete tracks"
        );
        assert!(
            remaining_track_paths.contains(&nested_track_path.to_string_lossy().into_owned()),
            "track under still-registered nested watch folder should remain"
        );

        drop(conn);
        fs::remove_dir_all(&scan_folder)?;
        Ok(())
    }
}
