//! Video editor page: gtk::Paned with adjustment/trim sidebar on the left
//! and the mpv preview + transport + crop overlay on the right. Pushed
//! onto `main_stack` when the user clicks "Edit" on a queue item.

#![allow(clippy::cast_lossless, clippy::needless_borrows_for_generic_args)]

mod actions;
mod bindings;
mod build;
mod component;
mod crop;
mod gl;
mod render;
mod rotation;
mod segment_list;
mod segments;
mod sidebar;
mod time_format;
mod video_panel;

pub use component::{VideoEditResult, VideoEditor, VideoEditorInput, VideoEditorOutput};

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib;

use crate::converter::ffmpeg::TrimRange;
use crate::player::mpv_backend::MpvBackend;
use crate::player::mpv_backend::fast_render::FastRenderContext;

type Callback<T> = Rc<RefCell<Option<Box<dyn Fn(T)>>>>;

fn new_cb<T>() -> Callback<T> {
    Rc::new(RefCell::new(None))
}

fn fire<T: Clone>(cb: &Callback<T>, val: T) {
    if let Some(ref f) = *cb.borrow() {
        f(val);
    }
}

/// Inclusive start/end (seconds) of a trim range edited on the video
/// editor timeline.
#[derive(Clone, Debug)]
pub struct TrimSegment {
    /// Segment start in seconds from the beginning of the media.
    pub start: f64,
    /// Segment end in seconds; must satisfy `end >= start`.
    pub end: f64,
}

// Speed labels now live in `crate::ui_labels::speed_labels()`.
const SPEED_VALUES: &[f64] = &[0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
const DEFAULT_SIDEBAR_POS: i32 = 380;

/// Video editor page: trim/crop sidebar, mpv preview, transport
/// overlay, and segment list rendered as a `gtk::Paned`.
///
/// Pushed onto the main NavigationView when the user activates the
/// "Edit" action on a queue row. Cheap to construct — the mpv backend
/// is allocated lazily on the first call to `load_file`.
///
/// Most fields are widget handles kept alive for binding/closures and
/// are not part of the supported external surface.
// Many fields hold widget refs alive; only a subset is touched after build.
#[allow(dead_code, missing_docs)]
pub struct EditPage {
    toolbar_view: adw::ToolbarView,
    header: adw::HeaderBar,
    convert_button: gtk::Button,
    back_button: gtk::Button,

    paned: gtk::Paned,

    brightness_scale: gtk::Scale,
    contrast_scale: gtk::Scale,
    saturation_scale: gtk::Scale,
    hue_scale: gtk::Scale,
    rotation: Rc<Cell<i32>>,
    flip_h: Rc<Cell<bool>>,
    flip_v: Rc<Cell<bool>>,
    segments_listbox: gtk::ListBox,
    segments_placeholder: adw::ActionRow,
    segments_list_row: adw::ActionRow,
    output_dropdown: gtk::DropDown,
    segment_overlay: gtk::DrawingArea,

    video_area: gtk::GLArea,
    overlay: gtk::Overlay,
    transport_overlay_box: gtk::Box,
    timeline: gtk::Scale,
    position_label: gtk::Label,
    duration_label: gtk::Label,
    frame_label: gtk::Label,

    play_button: gtk::Button,
    mark_button: gtk::Button,
    speed_button: gtk::Button,
    volume_button: gtk::Button,
    volume_scale: gtk::Scale,
    fullscreen_button: gtk::Button,
    audio_track_button: gtk::Button,
    subtitle_track_button: gtk::Button,
    audio_track_box: gtk::Box,
    subtitle_track_box: gtk::Box,

    crop_left: gtk::SpinButton,
    crop_right: gtk::SpinButton,
    crop_top: gtk::SpinButton,
    crop_bottom: gtk::SpinButton,
    crop_overlay_area: gtk::DrawingArea,
    show_grid: Rc<Cell<bool>>,
    grid_button: gtk::ToggleButton,

    media_metadata_resolution: gtk::Label,
    media_metadata_codec: gtk::Label,
    media_metadata_file_size: gtk::Label,
    media_metadata_duration: gtk::Label,

    segments: Rc<RefCell<Vec<TrimSegment>>>,
    mark_point: Rc<Cell<Option<f64>>>,
    duration: Rc<Cell<f64>>,
    position: Rc<Cell<f64>>,
    seeking: Rc<Cell<bool>>,
    is_fullscreen: Rc<Cell<bool>>,
    video_width: Rc<Cell<i64>>,
    video_height: Rc<Cell<i64>>,
    file_path: Rc<RefCell<Option<String>>>,

    mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    render_ctx: Rc<RefCell<Option<FastRenderContext>>>,
    // Mirrors the player offload path: hw-plane scanout on Wayland+MESA, GLArea fallback otherwise.
    video_backend: Rc<RefCell<Option<crate::player::mpv_backend::offload_renderer::VideoBackend>>>,
    // caveman: one-shot guard; load_file re-runs setup_gl_rendering on every
    // open and we must not stack realize handlers (each would re-install the
    // backend, duplicating offload renderers and re-triggering the GLArea
    // reparent on every realize cycle).
    realize_wired: Rc<Cell<bool>>,

    // 150 ms debounce so slider drag doesn't thrash mpv af.
    af_timer: Rc<Cell<Option<glib::SourceId>>>,
    hide_timer: Rc<Cell<Option<glib::SourceId>>>,

    on_apply_trim: Callback<Vec<TrimRange>>,
    on_segments_changed: Callback<Vec<TrimSegment>>,
    on_fullscreen: Callback<bool>,
    on_convert: Callback<()>,
    on_show_audio_settings: Callback<()>,
    on_back: Callback<()>,
}

impl Default for EditPage {
    fn default() -> Self {
        Self::new()
    }
}

impl EditPage {
    /// Construct the full-screen video editor page (timeline, segments,
    /// crop overlay, GLArea, header bar). Cheap GTK widget allocation;
    /// no mpv backend is created until [`EditPage::load_file`] is called.
    #[must_use]
    pub fn new() -> Self {
        build::build_edit_page()
    }
}
