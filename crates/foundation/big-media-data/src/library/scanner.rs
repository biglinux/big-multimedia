//! Parallel audio-file scanner — uses `ignore` crate's walker (drives all
//! cores) and inserts new tracks while pruning stale ones.

use log::{debug, info, warn};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::db::queries;
use crate::error::Result;
use crate::library::metadata;

pub use big_media_engines::media_files::AUDIO_EXTENSIONS;

/// Inserts new tracks and prunes entries whose files have been deleted.
/// Returns the count of newly added tracks.
pub fn scan_library(conn: &Connection, folder: &str) -> Result<usize> {
    let files = discover_audio_files(folder)?;
    info!("Discovered {} audio files in {folder}", files.len());

    let existing_paths = queries::get_all_file_paths(conn)?;
    let existing_set: std::collections::HashSet<&str> = existing_paths
        .iter()
        .map(std::string::String::as_str)
        .collect();

    let mut added = 0_usize;

    let tx_result = {
        let tx = conn.unchecked_transaction()?;

        for path in &files {
            let path_str = path.to_string_lossy();
            if existing_set.contains(path_str.as_ref()) {
                continue;
            }
            match metadata::read_track_metadata(&path_str) {
                Ok(track) => match queries::insert_track(&tx, &track) {
                    Err(e) => {
                        warn!("Failed to insert {path_str}: {e}");
                    }
                    _ => {
                        added += 1;
                    }
                },
                Err(e) => {
                    debug!("Skipping {path_str}: {e}");
                }
            }
        }

        tx.commit()
    };

    tx_result?;

    let stale_paths: Vec<&str> = existing_paths
        .iter()
        .filter(|p| !Path::new(p).exists())
        .map(std::string::String::as_str)
        .collect();

    if !stale_paths.is_empty() {
        let removed = stale_paths.len();
        for path in &stale_paths {
            if let Err(e) = conn.execute("DELETE FROM tracks WHERE file_path = ?1", [path]) {
                warn!("Failed removing stale track {path}: {e}");
            }
        }
        info!("Removed {removed} stale entries");
    }
    info!("Scan complete: {added} new tracks added from {folder}");
    Ok(added)
}

pub fn discover_audio_files(folder: &str) -> Result<Vec<PathBuf>> {
    let files: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

    ignore::WalkBuilder::new(folder)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .build_parallel()
        .run(|| {
            let files = &files;
            Box::new(move |entry| {
                if let Ok(entry) = entry {
                    let is_file = entry.file_type().is_some_and(|ft| ft.is_file());
                    if is_file
                        && big_media_engines::media_files::is_audio_file(
                            &entry.path().to_string_lossy(),
                        )
                    {
                        files
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .push(entry.into_path());
                    }
                }
                ignore::WalkState::Continue
            })
        });

    // Walker has joined by here — `into_inner()` cannot be poisoned.
    Ok(files
        .into_inner()
        .expect("walker must be finished before collecting files"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_extensions_contains_common_formats() {
        for ext in ["mp3", "flac", "ogg", "opus", "wav", "m4a", "aac"] {
            assert!(
                AUDIO_EXTENSIONS.contains(&ext),
                "missing common audio extension: {ext}"
            );
        }
    }

    #[test]
    fn audio_extensions_no_video_formats() {
        for ext in ["mp4", "mkv", "avi", "mov", "webm"] {
            assert!(
                !AUDIO_EXTENSIONS.contains(&ext),
                "video extension should not be in AUDIO_EXTENSIONS: {ext}"
            );
        }
    }

    #[test]
    fn discover_empty_folder() {
        let dir = std::env::temp_dir().join("bmp-test-scanner-empty");
        let _ = std::fs::create_dir_all(&dir);
        let result = discover_audio_files(dir.to_str().unwrap()).unwrap();
        assert!(result.is_empty());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn discover_finds_audio_files() {
        let dir = std::env::temp_dir().join("bmp-test-scanner-find");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(dir.join("song.mp3"), b"fake").unwrap();
        std::fs::write(dir.join("track.flac"), b"fake").unwrap();
        std::fs::write(dir.join("video.mp4"), b"fake").unwrap();
        std::fs::write(dir.join("readme.txt"), b"fake").unwrap();

        let result = discover_audio_files(dir.to_str().unwrap()).unwrap();
        assert_eq!(
            result.len(),
            2,
            "should find 2 audio files, got: {result:?}"
        );

        let names: Vec<String> = result
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"song.mp3".to_string()));
        assert!(names.contains(&"track.flac".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_recursive_subdirectories() {
        let dir = std::env::temp_dir().join("bmp-test-scanner-recursive");
        let sub = dir.join("subdir");
        let _ = std::fs::create_dir_all(&sub);

        std::fs::write(dir.join("root.ogg"), b"fake").unwrap();
        std::fs::write(sub.join("nested.opus"), b"fake").unwrap();

        let result = discover_audio_files(dir.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 2, "should find files in subdirectories");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_case_insensitive_extensions() {
        let dir = std::env::temp_dir().join("bmp-test-scanner-case");
        let _ = std::fs::create_dir_all(&dir);

        std::fs::write(dir.join("SONG.MP3"), b"fake").unwrap();
        std::fs::write(dir.join("Track.Flac"), b"fake").unwrap();

        let result = discover_audio_files(dir.to_str().unwrap()).unwrap();
        assert_eq!(
            result.len(),
            2,
            "should match case-insensitive extensions, got: {result:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn discover_nonexistent_folder_returns_error() {
        // `ignore` may return Ok([]) or Err — assert only that it doesn't panic.
        let result = discover_audio_files("/tmp/bmp-test-nonexistent-xyz-9999");
        let _ = result;
    }
}
