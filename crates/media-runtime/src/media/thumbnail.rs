//! Thumbnail extraction (ffmpeg) with Freedesktop file-manager cache reuse.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU32;

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use gtk::glib;

use super::metadata_cache::{acquire_slot, meta_cache, release_slot};

const MAX_THUMBS: u32 = 6;

static ACTIVE_THUMBS: AtomicU32 = AtomicU32::new(0);

fn thumbnail_cache_dir() -> Option<PathBuf> {
    let dir = big_os_kit::xdg_dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("big-video-player/thumbnails");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// `path + mtime` so renames or edits invalidate the cached thumb.
fn thumb_hash(path: &str) -> String {
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_or(0, |t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    mtime.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Reuses thumbs from Dolphin / Nautilus / Nemo / Thunar.
/// Spec: `~/.cache/thumbnails/{size}/{md5(file://uri)}.png`.
fn find_freedesktop_thumbnail(video_path: &str) -> Option<PathBuf> {
    let p = Path::new(video_path);
    if !p.is_absolute() {
        return None;
    }
    let uri = format!("file://{video_path}");
    let checksum = glib::compute_checksum_for_string(glib::ChecksumType::Md5, &uri)?;
    let filename = format!("{checksum}.png");

    let base = big_os_kit::xdg_dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("thumbnails");

    // Larger variants first → better quality.
    for subdir in ["x-large", "xx-large", "large", "normal"] {
        let path = base.join(subdir).join(&filename);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// Synchronous; semaphore-gated to cap concurrent ffmpeg children.
pub fn get_or_generate_thumbnail(video_path: &str) -> Option<PathBuf> {
    let cache = thumbnail_cache_dir()?;
    let hash = thumb_hash(video_path);
    let thumb_path = cache.join(format!("{hash}.png"));

    if thumb_path.exists() {
        return Some(thumb_path);
    }

    if let Some(fd_thumb) = find_freedesktop_thumbnail(video_path) {
        return Some(fd_thumb);
    }

    acquire_slot(&ACTIVE_THUMBS, MAX_THUMBS);

    // Cached duration → no extra ffprobe.
    let seek_pos = meta_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(video_path).and_then(|e| e.meta.duration_secs))
        .map_or_else(
            || {
                BigSubprocessSpec::builder()
                    .program("ffprobe")
                    .args([
                        "-v",
                        "quiet",
                        "-show_entries",
                        "format=duration",
                        "-of",
                        "csv=p=0",
                        // `--` defends against filenames starting with `-`.
                        "--",
                        video_path,
                    ])
                    .build()
                    .run()
                    .ok()
                    .and_then(|o| {
                        String::from_utf8_lossy(&o.stdout)
                            .trim()
                            .parse::<f64>()
                            .ok()
                    })
                    .map_or_else(|| "10".into(), |dur| format!("{:.1}", dur * 0.3))
            },
            |dur| format!("{:.1}", dur * 0.3),
        );

    let status = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-ss",
            &seek_pos,
            "-i",
            video_path,
            "-frames:v",
            "1",
            "-update",
            "1",
            "-vf",
            // reset_sar: PNG has no pixel aspect, so anamorphic frames need
            // square pixels.
            "scale=240:135:force_original_aspect_ratio=decrease:reset_sar=1,\
             pad=240:135:(ow-iw)/2:(oh-ih)/2",
            "-y",
            // `--` end-of-options: defend output path even though it is
            // generated from the cache dir + hash (never starts with `-`).
            "--",
            thumb_path.to_str().unwrap_or("/dev/null"),
        ])
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run()
        .ok();

    release_slot(&ACTIVE_THUMBS);

    if status.is_some_and(|s| s.status.success()) && thumb_path.exists() {
        Some(thumb_path)
    } else {
        None
    }
}

const MAX_TEXTURE_CACHE: usize = 96;

thread_local! {
    /// Main-thread cache of decoded thumbnail textures, keyed by the thumbnail
    /// FILE path (which already encodes the source path + mtime via `thumb_hash`,
    /// so it self-invalidates on edit/rename). The playlist grid and queue views
    /// rebuild on every file-open and re-bind the same handful of videos; without
    /// interning, each bind wrapped a FRESH `GdkTexture`, and every distinct
    /// texture forced a new GSK GL-texture upload the renderer never reclaimed —
    /// anonymous RSS grew ~1 MiB per file-open with no plateau. Returning one
    /// canonical `GdkTexture` per thumb lets the renderer reuse its cached GL
    /// texture instead.
    static TEXTURE_CACHE: std::cell::RefCell<(
        std::collections::HashMap<PathBuf, gtk::gdk::Texture>,
        std::collections::VecDeque<PathBuf>,
    )> = std::cell::RefCell::new((
        std::collections::HashMap::new(),
        std::collections::VecDeque::new(),
    ));
}

/// Return the canonical cached texture for `thumb_path`, inserting `fresh` (the
/// off-thread-decoded texture) on a miss. On a hit the `fresh` texture is dropped
/// and the shared one returned, so every card showing the same thumb feeds the
/// renderer one identical `GdkTexture` (one GL upload, not one per bind). Bounded
/// LRU so a long browse session cannot grow the cache without limit.
#[must_use]
pub fn intern_thumbnail_texture(
    thumb_path: PathBuf,
    fresh: gtk::gdk::Texture,
) -> gtk::gdk::Texture {
    TEXTURE_CACHE.with(|cell| {
        let (map, order) = &mut *cell.borrow_mut();
        if let Some(existing) = map.get(&thumb_path) {
            return existing.clone();
        }
        map.insert(thumb_path.clone(), fresh.clone());
        order.push_back(thumb_path);
        while order.len() > MAX_TEXTURE_CACHE {
            if let Some(evicted) = order.pop_front() {
                map.remove(&evicted);
            }
        }
        fresh
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_hash_deterministic() {
        // Same path + same mtime → same hash
        let dir = std::env::temp_dir().join("bmp-test-thumb-hash");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("test-video.mp4");
        std::fs::write(&file, b"fake video data").unwrap();

        let h1 = thumb_hash(file.to_str().unwrap());
        let h2 = thumb_hash(file.to_str().unwrap());
        assert_eq!(h1, h2, "hash should be deterministic for same file");
        assert_eq!(h1.len(), 16, "hash should be 16 hex chars");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumb_hash_different_paths() {
        let dir = std::env::temp_dir().join("bmp-test-thumb-hash-diff");
        let _ = std::fs::create_dir_all(&dir);
        let f1 = dir.join("a.mp4");
        let f2 = dir.join("b.mp4");
        std::fs::write(&f1, b"data").unwrap();
        std::fs::write(&f2, b"data").unwrap();

        let h1 = thumb_hash(f1.to_str().unwrap());
        let h2 = thumb_hash(f2.to_str().unwrap());
        assert_ne!(h1, h2, "different paths should produce different hashes");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thumb_hash_nonexistent_file_uses_zero_mtime() {
        // Should not panic, uses mtime=0 fallback
        let h = thumb_hash("/tmp/bmp-nonexistent-file-xyz.mp4");
        assert_eq!(h.len(), 16);
    }

    #[test]
    fn find_freedesktop_relative_path_returns_none() {
        assert!(find_freedesktop_thumbnail("relative/path.mp4").is_none());
    }

    #[test]
    fn find_freedesktop_nonexistent_returns_none() {
        assert!(find_freedesktop_thumbnail("/tmp/bmp-nonexistent-video-xyz.mp4").is_none());
    }

    #[test]
    fn thumbnail_cache_dir_creates_directory() {
        let dir = thumbnail_cache_dir();
        assert!(dir.is_some(), "cache dir should be creatable");
        let path = dir.unwrap();
        assert!(path.exists());
        assert!(
            path.to_str()
                .unwrap()
                .contains("big-video-player/thumbnails")
        );
    }

    #[test]
    fn get_or_generate_nonexistent_returns_none() {
        // Non-existent video file → no thumbnail generated
        let result = get_or_generate_thumbnail("/tmp/bmp-nonexistent-video-xyz-9999.mp4");
        assert!(result.is_none());
    }
}
