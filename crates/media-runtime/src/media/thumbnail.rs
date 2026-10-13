//! Thumbnail extraction (ffmpeg) with Freedesktop file-manager cache reuse.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use gtk::glib;

use big_relm4_components::display::texture::{RgbaImage, pixbuf_to_rgba, rgba_to_texture};

use super::metadata_cache::{acquire_slot, meta_cache, release_slot};

const MAX_THUMBS: u32 = 6;

static ACTIVE_THUMBS: AtomicU32 = AtomicU32::new(0);
static PARTIAL_SEQ: AtomicU64 = AtomicU64::new(0);

fn thumbnail_cache_dir(cache_root: &Path) -> Option<PathBuf> {
    let dir = cache_root.join("big-video-player/thumbnails");
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
/// Spec: `$XDG_CACHE_HOME/thumbnails/{size}/{md5(file URI)}.png`, the URI
/// percent-encoded.
fn find_freedesktop_thumbnail(cache_root: &Path, video_path: &str) -> Option<PathBuf> {
    let uri = glib::filename_to_uri(video_path, None).ok()?;
    let checksum = glib::compute_checksum_for_string(glib::ChecksumType::Md5, &uri)?;
    let filename = format!("{checksum}.png");
    let base = cache_root.join("thumbnails");

    // `large` (256 px) covers the 240 px cards; bigger ones are only scaled
    // down, and `normal` (128 px) is the last resort.
    for subdir in ["large", "x-large", "xx-large", "normal"] {
        let path = base.join(subdir).join(&filename);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 30 % into the video, as `-ss` text; the cached duration saves an ffprobe.
fn seek_position(video_path: &str) -> String {
    let cached = meta_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(video_path).and_then(|e| e.meta.duration_secs));
    let duration = cached.or_else(|| {
        let output = BigSubprocessSpec::builder()
            .program("ffprobe")
            .args([
                "-v",
                "quiet",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
                // A file name starting with `-` must not read as a flag.
                "--",
                video_path,
            ])
            .build()
            .run()
            .ok()?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f64>()
            .ok()
    });
    duration.map_or_else(|| "10".into(), |dur| format!("{:.1}", dur * 0.3))
}

/// Synchronous; semaphore-gated to cap concurrent ffmpeg children. `None`
/// without a private cache directory.
fn get_or_generate_thumbnail(video_path: &str) -> Option<PathBuf> {
    let cache_root = big_os_kit::xdg_dirs::cache_dir()?;
    let cache = thumbnail_cache_dir(&cache_root)?;
    let hash = thumb_hash(video_path);
    let thumb_path = cache.join(format!("{hash}.png"));

    if thumb_path.exists() {
        return Some(thumb_path);
    }

    if let Some(fd_thumb) = find_freedesktop_thumbnail(&cache_root, video_path) {
        return Some(fd_thumb);
    }

    acquire_slot(&ACTIVE_THUMBS, MAX_THUMBS);

    let seek_pos = seek_position(video_path);

    // ffmpeg writes beside the final name and the rename publishes it whole:
    // a killed or concurrent run never leaves a truncated PNG that `exists()`
    // would serve from then on.
    let partial = cache.join(format!(
        ".{hash}.{}.{}.png",
        std::process::id(),
        PARTIAL_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let status = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-ss",
            seek_pos.as_str(),
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
            "--",
        ])
        .arg(&partial)
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run()
        .ok();

    release_slot(&ACTIVE_THUMBS);

    let published = status.is_some_and(|s| s.status.success())
        && std::fs::rename(&partial, &thumb_path).is_ok();
    if published {
        Some(thumb_path)
    } else {
        let _ = std::fs::remove_file(&partial);
        None
    }
}

const MAX_TEXTURE_CACHE: usize = 96;

/// Longest edge a thumbnail is decoded to: the cards draw it 240 px wide, and a
/// file-manager `xx-large` thumbnail decoded whole is 1024 px, ~2.4 MiB a card.
const THUMB_DECODE_EDGE: i32 = 256;

/// Worker side: the thumbnail file for `video_path`, decoded no larger than the
/// cards show it. Build the texture on the main loop with [`thumbnail_texture`].
#[must_use]
pub fn load_thumbnail(video_path: &str) -> Option<(PathBuf, RgbaImage)> {
    let thumb = get_or_generate_thumbnail(video_path)?;
    let pixbuf = gtk::gdk_pixbuf::Pixbuf::from_file_at_scale(
        &thumb,
        THUMB_DECODE_EDGE,
        THUMB_DECODE_EDGE,
        true,
    )
    .ok()?;
    let image = pixbuf_to_rgba(&pixbuf)?;
    Some((thumb, image))
}

thread_local! {
    /// Main-thread cache of decoded thumbnail textures, keyed by the thumbnail
    /// file path (which already encodes the source path + mtime via `thumb_hash`,
    /// so it self-invalidates on edit/rename). The playlist grid and queue views
    /// rebuild on every file-open and re-bind the same handful of videos. Every
    /// distinct `GdkTexture` costs a GSK GL-texture upload the renderer never
    /// reclaims, so one canonical texture per thumb lets the renderer reuse its
    /// cached GL texture and keeps memory flat across file-opens.
    static TEXTURE_CACHE: std::cell::RefCell<(
        std::collections::HashMap<PathBuf, gtk::gdk::Texture>,
        std::collections::VecDeque<PathBuf>,
    )> = std::cell::RefCell::new((
        std::collections::HashMap::new(),
        std::collections::VecDeque::new(),
    ));
}

/// Main-loop side of [`load_thumbnail`]: the one cached texture for
/// `thumb_path`, built from `image` only on a miss. Every card showing the same
/// thumb then feeds the renderer one identical `GdkTexture` (one GL upload, not
/// one per bind). Bounded LRU so a long browse session cannot grow it without
/// limit.
#[must_use]
pub fn thumbnail_texture(thumb_path: PathBuf, image: RgbaImage) -> gtk::gdk::Texture {
    TEXTURE_CACHE.with(|cell| {
        let (map, order) = &mut *cell.borrow_mut();
        if let Some(existing) = map.get(&thumb_path) {
            return existing.clone();
        }
        let texture = rgba_to_texture(image);
        map.insert(thumb_path.clone(), texture.clone());
        order.push_back(thumb_path);
        while order.len() > MAX_TEXTURE_CACHE {
            if let Some(evicted) = order.pop_front() {
                map.remove(&evicted);
            }
        }
        texture
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_hash_is_stable() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("video.mp4");
        std::fs::write(&file, b"data").unwrap();
        let file = file.to_str().unwrap();
        assert_eq!(thumb_hash(file), thumb_hash(file));
        assert_eq!(thumb_hash(file).len(), 16);
    }

    #[test]
    fn thumb_hash_differs_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.mp4");
        let second = dir.path().join("b.mp4");
        std::fs::write(&first, b"data").unwrap();
        std::fs::write(&second, b"data").unwrap();
        assert_ne!(
            thumb_hash(first.to_str().unwrap()),
            thumb_hash(second.to_str().unwrap())
        );
    }

    #[test]
    fn freedesktop_thumbnail_uses_encoded_uri() {
        let cache = tempfile::tempdir().unwrap();
        let video = "/videos/My Film ç.mkv";
        let checksum = glib::compute_checksum_for_string(
            glib::ChecksumType::Md5,
            "file:///videos/My%20Film%20%C3%A7.mkv",
        )
        .unwrap();
        let large = cache.path().join("thumbnails/large");
        std::fs::create_dir_all(&large).unwrap();
        std::fs::write(large.join(format!("{checksum}.png")), b"png").unwrap();
        assert_eq!(
            find_freedesktop_thumbnail(cache.path(), video),
            Some(large.join(format!("{checksum}.png")))
        );
    }

    #[test]
    fn freedesktop_thumbnail_needs_absolute_path() {
        let cache = tempfile::tempdir().unwrap();
        assert!(find_freedesktop_thumbnail(cache.path(), "relative/path.mp4").is_none());
    }
}
