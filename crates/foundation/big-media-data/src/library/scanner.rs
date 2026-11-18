//! Streaming discovery; unavailable files are never an instruction to delete rows.
use crate::error::Result;
pub use big_media_engines::media_files::AUDIO_EXTENSIONS;
use std::path::Path;

/// Cancellation is checked between directory entries; a kernel filesystem call
/// itself is not forcibly interruptible by this cooperative API.
pub fn visit_audio_files_cancellable(
    folder: &str,
    cancelled: &std::sync::atomic::AtomicBool,
    mut visit: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        return Err(crate::Error::Cancelled);
    }
    if !std::fs::metadata(folder)?.is_dir() {
        return Err(crate::Error::invalid_argument(
            "folder",
            "choose an available directory",
        ));
    }
    for entry in ignore::WalkBuilder::new(folder)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .filter_entry(|entry| !big_os_kit::storage::is_staging_name(entry.file_name()))
        .build()
    {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) {
            return Err(crate::Error::Cancelled);
        }
        let entry = entry.map_err(|_| {
            crate::Error::internal(
                "directory traversal incomplete; earlier import batches were preserved",
            )
        })?;
        if entry.file_type().is_some_and(|ty| ty.is_file())
            && entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    AUDIO_EXTENSIONS
                        .iter()
                        .any(|known| ext.eq_ignore_ascii_case(known))
                })
        {
            visit(entry.path())?;
        }
    }
    Ok(())
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

    /// Files a killed writer left mid-write must never become tracks.
    #[test]
    fn scan_skips_staging_leftovers() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("song.mp3"), b"x").unwrap();
        std::fs::create_dir(folder.path().join(".big-media-1-0a1b")).unwrap();
        std::fs::write(folder.path().join(".big-media-1-0a1b/song.mp3"), b"x").unwrap();
        std::fs::write(folder.path().join(".big-media-1-x7Yz.mp3"), b"x").unwrap();
        std::fs::write(folder.path().join(".big-media-tags-x7Yz.flac"), b"x").unwrap();
        let mut found = Vec::new();
        visit_audio_files_cancellable(
            folder.path().to_str().unwrap(),
            &std::sync::atomic::AtomicBool::new(false),
            |path| {
                found.push(path.strip_prefix(folder.path()).unwrap().to_path_buf());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(found, [Path::new("song.mp3")]);
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
}
