//! Thumbnail extraction (ffmpeg) with Freedesktop file-manager cache reuse.

use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use big_media_data::media_probe::input_protocols;
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

fn duration_probe_spec(video_path: &str) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffprobe")
        .args(input_protocols(video_path))
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
}

/// One frame of `video_path` at `seek_secs`, scaled to a card, into `output`.
fn card_frame_spec(video_path: &str, seek_secs: f64, output: &Path) -> BigSubprocessSpec {
    BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(["-ss", &format!("{seek_secs:.2}")])
        .args(input_protocols(video_path))
        .args([
            "-i",
            video_path,
            "-frames:v",
            "1",
            "-update",
            "1",
            "-vf",
            // Even box: 4:2:0 output rounds an odd pad height below the scaled
            // frame and the filter fails. Square pixels for anamorphic sources.
            "scale=240:136:force_original_aspect_ratio=decrease:force_divisible_by=2:reset_sar=1,\
             pad=240:136:(ow-iw)/2:(oh-ih)/2",
            "-y",
            "--",
        ])
        .arg(output)
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
}

/// Renders one frame of `video_path` at `seek_secs`, fitted into a 240x136 card,
/// into `target`, whose extension picks the image format. ffmpeg writes a staging
/// file beside `target` that replaces it only after a clean exit with a non-empty
/// file, so a killed or failed run never leaves a cached frame that `exists()`
/// would serve. Blocking; returns whether `target` was written. A network
/// stream is never opened: an IPTV panel counts each card against the account.
pub fn render_card_frame(video_path: &str, seek_secs: f64, target: &Path) -> bool {
    if big_os_kit::url::is_stream_uri(video_path) {
        return false;
    }
    let (Some(directory), Some(extension)) = (target.parent(), target.extension()) else {
        return false;
    };
    big_os_kit::storage::remove_stale_staging(directory);
    let mut partial_name = OsString::from(format!(
        "{}{}.",
        big_os_kit::storage::staging_prefix(),
        PARTIAL_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    partial_name.push(extension);
    let partial = directory.join(partial_name);
    let rendered = card_frame_spec(video_path, seek_secs, &partial)
        .run()
        .is_ok_and(|output| output.status.success());
    publish_frame(rendered, &partial, target)
}

fn publish_frame(rendered: bool, partial: &Path, target: &Path) -> bool {
    let whole = rendered && partial.metadata().is_ok_and(|m| m.len() > 0);
    if whole && std::fs::rename(partial, target).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(partial);
    false
}

/// 30 % into the video; the cached duration saves an ffprobe.
fn seek_position(video_path: &str) -> f64 {
    let cached = meta_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(video_path).and_then(|e| e.meta.duration_secs));
    let duration = cached.or_else(|| {
        let output = duration_probe_spec(video_path).run().ok()?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f64>()
            .ok()
    });
    duration.map_or(10.0, |dur| dur * 0.3)
}

/// Synchronous; semaphore-gated to cap concurrent ffmpeg children. `None`
/// without a private cache directory, and for a network stream, which
/// [`render_card_frame`] does not open.
fn get_or_generate_thumbnail(video_path: &str) -> Option<PathBuf> {
    if big_os_kit::url::is_stream_uri(video_path) {
        return None;
    }
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

    let published = render_card_frame(video_path, seek_position(video_path), &thumb_path);
    release_slot(&ACTIVE_THUMBS);
    published.then_some(thumb_path)
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
    fn local_videos_open_only_files() {
        let input = "/videos/concat:a|b.mp4";
        for spec in [
            duration_probe_spec(input),
            card_frame_spec(input, 1.0, Path::new("/cache/.x.png")),
        ] {
            let argv = spec.try_resolved().unwrap().argv;
            let whitelist_at = argv
                .windows(2)
                .position(|pair| pair == ["-protocol_whitelist", "file,pipe"])
                .unwrap_or(usize::MAX);
            let input_at = argv.iter().position(|arg| arg == input).unwrap();
            assert!(whitelist_at < input_at, "{argv:?}");
        }
    }

    /// A run that failed or timed out may have written part of the image; only
    /// a clean exit with bytes publishes it.
    #[test]
    fn frame_published_only_when_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (partial, frame) = (dir.path().join("partial.jpg"), dir.path().join("0.jpg"));
        std::fs::write(&partial, b"truncated").unwrap();
        assert!(!publish_frame(false, &partial, &frame));
        assert!(!frame.exists() && !partial.exists());
        std::fs::write(&partial, b"").unwrap();
        assert!(!publish_frame(true, &partial, &frame));
        assert!(!frame.exists() && !partial.exists());
        std::fs::write(&partial, b"whole").unwrap();
        assert!(publish_frame(true, &partial, &frame));
        assert_eq!(std::fs::read(&frame).unwrap(), b"whole");
    }

    #[test]
    fn cards_never_open_streams() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/live/1.ts", listener.local_addr().unwrap());
        let dir = tempfile::tempdir().unwrap();
        assert!(crate::media::playlist::probe_metadata_cached(&url).is_none());
        assert!(get_or_generate_thumbnail(&url).is_none());
        assert!(!render_card_frame(&url, 1.0, &dir.path().join("0.jpg")));
        let connection = listener.accept().map(|_| ()).map_err(|error| error.kind());
        assert_eq!(connection, Err(std::io::ErrorKind::WouldBlock));
    }

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
