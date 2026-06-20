//! Pure (GTK-free) M3U/M3U8/PLS import parser for the Relm4 `AudioShell`.
//!
//! Ported out of the god-object `window::playlists` so the shell can parse an
//! imported playlist off-thread, then resolve the paths against the library and
//! insert them into the queue. Returns raw file paths in playlist order.

use std::path::Path;

/// Hard cap to bound memory of a hostile / corrupted M3U/PLS file. 16 MiB fits
/// any realistic personal playlist (≈ 200 000 entries at 80 chars/line).
const MAX_PLAYLIST_BYTES: u64 = 16 * 1024 * 1024;

/// Parse an `.m3u`/`.m3u8`/`.pls` file into its referenced file paths. Relative
/// M3U entries are resolved against the playlist's directory. Oversize or
/// unreadable files yield an empty list.
pub(crate) fn parse_playlist_file_for_import(path: &Path) -> Vec<String> {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > MAX_PLAYLIST_BYTES {
            log::warn!(
                "playlist import skip: {} bytes exceeds MAX_PLAYLIST_BYTES",
                meta.len()
            );
            return Vec::new();
        }
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut file_paths = Vec::new();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if ext == "pls" {
        for line in content.lines() {
            if let Some(path_val) = line.strip_prefix("File") {
                if let Some((_num, fpath)) = path_val.split_once('=') {
                    file_paths.push(fpath.to_string());
                }
            }
        }
    } else {
        let base_dir = path.parent().map(Path::to_path_buf);
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let resolved = if Path::new(line).is_absolute() {
                line.to_string()
            } else if let Some(ref base) = base_dir {
                base.join(line).to_string_lossy().to_string()
            } else {
                line.to_string()
            };
            file_paths.push(resolved);
        }
    }

    file_paths
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn import_rejects_oversize_playlist() {
        let path = std::env::temp_dir().join("bap-import-oversize.m3u");
        let mut f = std::fs::File::create(&path).expect("create temp");
        // Seek past the cap to avoid materializing 16 MiB.
        use std::io::Seek;
        f.seek(std::io::SeekFrom::Start(MAX_PLAYLIST_BYTES))
            .expect("seek");
        f.write_all(b"x").expect("write trailing");
        drop(f);

        assert!(parse_playlist_file_for_import(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_accepts_small_playlist() {
        let path = std::env::temp_dir().join("bap-import-small.m3u");
        std::fs::write(&path, b"/music/a.mp3\n/music/b.flac\n").expect("write");
        assert_eq!(
            parse_playlist_file_for_import(&path),
            vec!["/music/a.mp3".to_string(), "/music/b.flac".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn import_parses_pls_file_entries() {
        let path = std::env::temp_dir().join("bap-import.pls");
        std::fs::write(&path, b"[playlist]\nFile1=/m/a.mp3\nFile2=/m/b.ogg\n").expect("write");
        assert_eq!(
            parse_playlist_file_for_import(&path),
            vec!["/m/a.mp3".to_string(), "/m/b.ogg".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }
}
