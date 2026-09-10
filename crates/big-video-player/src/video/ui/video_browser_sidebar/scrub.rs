//! Hover-scrub previews for grid cards (creator triage): moving the pointer
//! across a card's thumbnail scrubs through frames sampled along the clip, so a
//! reviewer can judge content without opening the file.
//!
//! Frames are generated lazily on first hover (a handful of evenly-spaced ffmpeg
//! grabs, cached to disk), decoded into in-memory textures once, then swapped on
//! pointer motion with no per-event I/O. Leaving the card restores the original
//! thumbnail. All generation runs off the main thread.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use gtk::{gdk, gio, glib};

use super::BrowserState;
use crate::media::playlist;

/// Number of frames sampled across a clip.
const SCRUB_FRAME_COUNT: usize = 8;

/// Per-card scrub state, persisted across factory rebinds.
#[derive(Default)]
struct ScrubPreviewState {
    /// Path the cached textures belong to (`""` until first hover).
    path: String,
    /// Decoded frame textures (filled once generation completes).
    textures: Vec<gdk::Texture>,
    /// The thumbnail shown before scrubbing, restored on pointer-leave.
    original: Option<gdk::Paintable>,
}

/// Wire hover-scrub onto a grid card's picture. Resolves the bound video path
/// from the card's `widget_name` at motion time (so it follows rebinds).
pub(super) fn wire_card_hover(state: &Rc<BrowserState>, card: &gtk::Box, picture: &gtk::Picture) {
    let scrub_state: Rc<RefCell<ScrubPreviewState>> =
        Rc::new(RefCell::new(ScrubPreviewState::default()));
    let motion = gtk::EventControllerMotion::new();

    motion.connect_motion({
        let state = state.clone();
        let card = card.downgrade();
        let picture = picture.downgrade();
        let scrub_state = scrub_state.clone();
        move |_, pointer_x, _| {
            let (Some(card), Some(picture)) = (card.upgrade(), picture.upgrade()) else {
                return;
            };
            let Some(path) = video_path_of(&state, &card) else {
                return;
            };
            ensure_started(&scrub_state, &picture, &path);
            apply_frame(&scrub_state, &picture, &path, pointer_x);
        }
    });

    motion.connect_leave({
        let picture = picture.downgrade();
        let scrub_state = scrub_state.clone();
        move |_| {
            if let Some(picture) = picture.upgrade()
                && let Some(original) = scrub_state.borrow().original.clone()
            {
                picture.set_paintable(Some(&original));
            }
        }
    });

    card.add_controller(motion);
}

/// The bound video path for a card, via its `video:{index}` widget name.
fn video_path_of(state: &Rc<BrowserState>, card: &gtk::Box) -> Option<String> {
    let card_index = parse_card_index(&card.widget_name())?;
    state.active_items().get(card_index).cloned()
}

/// Card index encoded in a grid card's `video:{index}` widget name.
fn parse_card_index(name: &str) -> Option<usize> {
    name.strip_prefix("video:")?.parse::<usize>().ok()
}

/// On first hover for a path, snapshot the original thumbnail and kick off the
/// off-thread frame generation + decode.
fn ensure_started(
    scrub_state: &Rc<RefCell<ScrubPreviewState>>,
    picture: &gtk::Picture,
    path: &str,
) {
    {
        let mut scrub_state_borrow = scrub_state.borrow_mut();
        if scrub_state_borrow.path == path {
            return;
        }
        // New card binding → reset for this path.
        scrub_state_borrow.path = path.to_string();
        scrub_state_borrow.textures.clear();
        scrub_state_borrow.original = picture.paintable();
    }

    let path_owned = path.to_string();
    let scrub_state = scrub_state.clone();
    glib::spawn_future_local(async move {
        let generation_path = path_owned.clone();
        let frame_files = gio::spawn_blocking(move || ensure_frames(&generation_path))
            .await
            .unwrap_or_default();
        // Card may have been recycled to another path while generating.
        if scrub_state.borrow().path != path_owned {
            return;
        }
        let textures: Vec<gdk::Texture> = frame_files
            .iter()
            .filter_map(|frame_file| gdk::Texture::from_filename(frame_file).ok())
            .collect();
        let mut scrub_state_borrow = scrub_state.borrow_mut();
        if scrub_state_borrow.path == path_owned {
            scrub_state_borrow.textures = textures;
        }
    });
}

/// Map pointer x → frame index and show that frame, if generated.
fn apply_frame(
    scrub_state: &Rc<RefCell<ScrubPreviewState>>,
    picture: &gtk::Picture,
    path: &str,
    pointer_x: f64,
) {
    let scrub_state_borrow = scrub_state.borrow();
    if scrub_state_borrow.path != path {
        return;
    }
    let Some(frame_index) = frame_index_for_x(
        pointer_x,
        f64::from(picture.width()),
        scrub_state_borrow.textures.len(),
    ) else {
        return;
    };
    if let Some(texture) = scrub_state_borrow.textures.get(frame_index) {
        picture.set_paintable(Some(texture));
    }
}

/// Map pointer `x` within a `width`-wide picture to a frame index over
/// `frame_count` frames. `None` when the picture has no width or no frames yet.
/// `x` outside `[0, width]` clamps to the first/last frame.
fn frame_index_for_x(pointer_x: f64, width: f64, frame_count: usize) -> Option<usize> {
    if width <= 0.0 || frame_count == 0 {
        return None;
    }
    let pointer_fraction = (pointer_x / width).clamp(0.0, 1.0);
    Some(((pointer_fraction * frame_count as f64) as usize).min(frame_count - 1))
}

/// Cache directory for scrub frames (`~/.cache/big-video-player/scrub`).
fn cache_root() -> Option<PathBuf> {
    let cache_directory = glib::user_cache_dir().join("big-video-player/scrub");
    std::fs::create_dir_all(&cache_directory).ok()?;
    Some(cache_directory)
}

/// Per-clip frame directory key, salted with the file's mtime so edits
/// invalidate stale frames.
fn clip_key(path: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    if let Ok(metadata) = std::fs::metadata(path)
        && let Ok(modified_time) = metadata.modified()
        && let Ok(modified_since_epoch) = modified_time.duration_since(std::time::UNIX_EPOCH)
    {
        modified_since_epoch.as_secs().hash(&mut hasher);
    }
    hasher.finish()
}

/// Generate (or reuse cached) scrub frames for `path`, returning the existing
/// frame files in order. Blocking — call off the main thread.
fn ensure_frames(path: &str) -> Vec<PathBuf> {
    let Some(root) = cache_root() else {
        return Vec::new();
    };
    let clip_directory = root.join(format!("{:016x}", clip_key(path)));
    let _ = std::fs::create_dir_all(&clip_directory);

    let duration = playlist::probe_metadata_cached(path)
        .and_then(|m| m.duration_secs)
        .filter(|duration_secs| *duration_secs > 0.0)
        .unwrap_or(0.0);

    let mut frames = Vec::with_capacity(SCRUB_FRAME_COUNT);
    for frame_index in 0..SCRUB_FRAME_COUNT {
        let frame = clip_directory.join(format!("{frame_index}.jpg"));
        if !frame.exists() {
            let time = sample_time_secs(frame_index, duration);
            let Some(output_path) = frame.to_str() else {
                continue;
            };
            let _ = BigSubprocessSpec::builder()
                .program("ffmpeg")
                .args([
                    "-ss",
                    &format!("{time:.2}"),
                    "-i",
                    path,
                    "-vframes",
                    "1",
                    "-update",
                    "1",
                    "-vf",
                    "scale=240:135:force_original_aspect_ratio=decrease,\
                     pad=240:135:(ow-iw)/2:(oh-ih)/2",
                    "-y",
                    "--",
                    output_path,
                ])
                .stdout(BigSubprocessOutputMode::Null)
                .stderr(BigSubprocessOutputMode::Null)
                .build()
                .run();
        }
        if frame.exists() {
            frames.push(frame);
        }
    }
    frames
}

/// Sample time (seconds) for frame `i` of [`SCRUB_FRAME_COUNT`]: the center of each equal
/// segment when `duration` is known, else one frame per second as a fallback.
fn sample_time_secs(frame_index: usize, duration: f64) -> f64 {
    if duration > 0.0 {
        duration * (frame_index as f64 + 0.5) / SCRUB_FRAME_COUNT as f64
    } else {
        f64::from(frame_index as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_card_index_accepts_prefixed_number_only() {
        assert_eq!(parse_card_index("video:0"), Some(0));
        assert_eq!(parse_card_index("video:42"), Some(42));
        assert_eq!(parse_card_index("video:"), None);
        assert_eq!(parse_card_index("video:-1"), None);
        assert_eq!(parse_card_index("video:1.5"), None);
        assert_eq!(parse_card_index("audio:3"), None);
        assert_eq!(parse_card_index("3"), None);
    }

    #[test]
    fn frame_index_none_without_width_or_frames() {
        assert_eq!(frame_index_for_x(10.0, 0.0, 8), None);
        assert_eq!(frame_index_for_x(10.0, -5.0, 8), None);
        assert_eq!(frame_index_for_x(10.0, 100.0, 0), None);
    }

    #[test]
    fn frame_index_maps_across_width() {
        // 8 frames over a 80px card: each frame spans 10px.
        assert_eq!(frame_index_for_x(0.0, 80.0, 8), Some(0));
        assert_eq!(frame_index_for_x(5.0, 80.0, 8), Some(0));
        assert_eq!(frame_index_for_x(10.0, 80.0, 8), Some(1));
        assert_eq!(frame_index_for_x(79.0, 80.0, 8), Some(7));
    }

    #[test]
    fn frame_index_clamps_out_of_range_x() {
        // Negative and past-end x clamp to first/last frame, never panic.
        assert_eq!(frame_index_for_x(-50.0, 80.0, 8), Some(0));
        assert_eq!(frame_index_for_x(80.0, 80.0, 8), Some(7));
        assert_eq!(frame_index_for_x(1000.0, 80.0, 8), Some(7));
        assert_eq!(frame_index_for_x(40.0, 80.0, 1), Some(0));
    }

    #[test]
    fn sample_time_centers_segments_when_duration_known() {
        // 8 frames over 80s → segment centers at 5, 15, …, 75.
        assert!((sample_time_secs(0, 80.0) - 5.0).abs() < 1e-9);
        assert!((sample_time_secs(1, 80.0) - 15.0).abs() < 1e-9);
        assert!((sample_time_secs(7, 80.0) - 75.0).abs() < 1e-9);
    }

    #[test]
    fn sample_time_falls_back_to_index_seconds_without_duration() {
        assert!((sample_time_secs(0, 0.0) - 0.0).abs() < 1e-9);
        assert!((sample_time_secs(3, 0.0) - 3.0).abs() < 1e-9);
        // Negative/garbage duration uses the same fallback.
        assert!((sample_time_secs(5, -1.0) - 5.0).abs() < 1e-9);
    }
}
