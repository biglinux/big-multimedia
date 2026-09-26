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
