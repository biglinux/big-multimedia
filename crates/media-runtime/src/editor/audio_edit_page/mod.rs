//! Audio editor page — audio-only playback, waveform preview and trim segments.

#![allow(clippy::needless_borrows_for_generic_args)]

mod actions;
mod bindings;
mod build;
mod component;
mod playback;
mod render;
mod segments;
mod waveform;

pub use component::{AudioEditor, AudioEditorInit, AudioEditorInput, AudioEditorOutput};

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::converter::ffmpeg::TrimRange;
use crate::player::mpv_backend::{AudioEffects, MpvBackend, PlayerEvent};

type Callback<T> = Rc<RefCell<Option<Rc<dyn Fn(T)>>>>;

fn new_cb<T>() -> Callback<T> {
    Rc::new(RefCell::new(None))
}

// Clone-out-then-fire so a callback that re-registers itself doesn't panic on a borrow alias.
fn fire<T: Clone>(cb: &Callback<T>, value: T) {
    let f = cb.borrow().as_ref().cloned();
    if let Some(f) = f {
        f(value);
    }
}

#[derive(Clone, Debug)]
pub(super) struct AudioSegment {
    pub(super) start: f64,
    pub(super) end: f64,
}

#[derive(Clone, Debug)]
pub(super) enum DragMode {
    None,
    StartHandle {
        idx: usize,
    },
    EndHandle {
        idx: usize,
    },
    SegmentBody {
        idx: usize,
        anchor_time: f64,
        orig_start: f64,
        orig_end: f64,
    },
}

/// Audio-mode editor: waveform preview, trim segments, playback
/// transport, and integration with the cleanup/equalizer dialogs.
///
/// Pushed onto the main NavigationView when the user activates the
/// "Edit" action on an audio queue row. The mpv backend is lazily
/// allocated on the first call to `load`.
///
/// Fields are widget and shared-state handles consumed by sibling
/// modules in this crate and are not part of the supported external
/// surface.
#[allow(missing_docs)]
pub struct AudioEditPage {
    toolbar: adw::ToolbarView,
    header: adw::HeaderBar,
    root: gtk::Box,
    controls: gtk::Box,
    trim_group: adw::PreferencesGroup,
    play_button: gtk::Button,
    play_selection_button: gtk::ToggleButton,
    auto_advance_button: gtk::ToggleButton,
    volume_button: gtk::Button,
    volume_label: gtk::Label,
    speed_button: gtk::Button,
    speed_label: gtk::Label,
    waveform: gtk::DrawingArea,
    timeline: gtk::Scale,
    position_label: gtk::Label,
    duration_label: gtk::Label,
    segments_list: gtk::ListBox,
    segments_placeholder: adw::ActionRow,
    segments_row: adw::ActionRow,
    mpv: Rc<RefCell<Option<MpvBackend>>>,
    duration: Rc<Cell<f64>>,
    position: Rc<Cell<f64>>,
    timeline_syncing: Rc<Cell<bool>>,
    suppress_auto_advance_after_manual_seek: Rc<Cell<bool>>,
    segments: Rc<RefCell<Vec<AudioSegment>>>,
    hover_segment: Rc<Cell<Option<usize>>>,
    hover_delete_button: Rc<Cell<bool>>,
    drag_mode: Rc<RefCell<DragMode>>,
    pending_mark: Rc<Cell<Option<f64>>>,
    waveform_data: Rc<RefCell<Option<Vec<f32>>>>,
    waveform_generation: Rc<Cell<u64>>,
    on_apply_trim: Callback<Vec<TrimRange>>,
    on_convert: Callback<()>,
    on_show_audio_settings: Callback<()>,
    on_show_equalizer: Callback<()>,
    on_playback_state_changed: Callback<bool>,
    on_previous_track: Callback<()>,
    on_next_track: Callback<()>,
    cut_mode_active: Rc<Cell<bool>>,
    cut_revealer: gtk::Revealer,
    zoom_label: gtk::Label,
    zoom_level: Rc<Cell<f64>>,
}

impl Default for AudioEditPage {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEditPage {
    /// Construct the audio-mode inline editor (waveform, segments,
    /// playback transport). Widget-only; the mpv backend is created on
    /// the first call to `load`.
    #[must_use]
    pub fn new() -> Self {
        build::build_audio_edit_page()
    }
}
