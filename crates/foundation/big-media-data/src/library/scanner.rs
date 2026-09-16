//! Streaming discovery; unavailable files are never an instruction to delete rows.
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use crate::{db::{queries, models::Track}, error::Result, library::metadata};
pub use big_media_engines::media_files::AUDIO_EXTENSIONS;

/// Partial commits are possible on later I/O failure; the caller must report that
/// a failed scan may already have imported earlier batches. No deletion occurs.
pub fn scan_library(conn: &Connection, folder: &str) -> Result<usize> {
    let mut added = 0; let mut batch: Vec<Track> = Vec::with_capacity(64);
    visit_audio_files(folder, |path| {
        let Some(path) = path.to_str() else {
            return Err(crate::Error::invalid_argument("path", "non-UTF-8 library path needs lossless schema support"));
        };
        let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE file_path=?1)", [path], |row| row.get(0))?;
        if exists { return Ok(()); }
        match metadata::read_track_metadata(path) {
            Ok(track) => batch.push(track),
            Err(_) => return Err(crate::Error::internal("metadata could not be read; earlier import batches were preserved")),
        }
        if batch.len() == 64 { added += insert_batch(conn, &batch)?; batch.clear(); }
        Ok(())
    })?;
    if !batch.is_empty() { added += insert_batch(conn, &batch)?; }
    Ok(added)
}
fn insert_batch(conn: &Connection, tracks: &[Track]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    for track in tracks { queries::insert_track(&tx, track)?; }
    tx.commit()?; Ok(tracks.len())
}
/// Visit audio paths without collecting them; traversal errors remain explicit.
pub fn visit_audio_files(folder: &str, mut visit: impl FnMut(&Path) -> Result<()>) -> Result<()> {
    if !std::fs::metadata(folder)?.is_dir() { return Err(crate::Error::invalid_argument("folder", "choose an available directory")); }
    for entry in ignore::WalkBuilder::new(folder).hidden(false).ignore(false)
        .git_ignore(false).git_global(false).git_exclude(false).follow_links(false).build() {
        let entry = entry.map_err(|_| crate::Error::internal("directory traversal incomplete; earlier import batches were preserved"))?;
        if entry.file_type().is_some_and(|ty| ty.is_file()) {
            if entry.path().extension().and_then(|ext| ext.to_str()).is_some_and(|ext| AUDIO_EXTENSIONS.iter().any(|known| ext.eq_ignore_ascii_case(known))) {
                visit(entry.path())?;
            }
        }
    }
    Ok(())
}
/// Compatibility collector for callers that explicitly need a list. Unlike the
/// scanner this API materializes paths and therefore has an explicit safety cap.
pub fn discover_audio_files(folder: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit_audio_files(folder, |path| {
        if files.len() >= 100_000 { return Err(crate::Error::internal("too many paths for a collected scan")); }
        files.push(path.to_owned()); Ok(())
    })?;
    files.sort_unstable(); files.dedup(); Ok(files)
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
        assert!(result.is_err());
    }
}
