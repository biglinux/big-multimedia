//! App-local OSD controls bar — recovers the old god-object `controls_box`
//! layout (the minimal `BigVideoTransport` regressed the design).
//!
//! Layout (mirrors `data/resources/ui/window.ui` `controls_box`):
//!   • seek row: position label · seek scale · duration label
//!   • center box:
//!       START  = volume / speed / voice hover popovers (shared builders)
//!       CENTER = previous · play/pause · next
//!       END    = subtitle MenuButton · audio-track MenuButton · fullscreen
//!
//! Push-model Component: buttons/scrub emit typed `VideoControlsBarOutput`
//! intents; the shell owns mpv/settings and pushes display state back via
//! `VideoControlsBarInput`. The volume/speed/voice popovers drive mpv
//! directly through callbacks captured at build time (the shell hands the bar a
//! live `mpv` accessor on `FileLoaded`), matching the old `popovers.rs` wiring.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::i18n::i18n;
use crate::player::mpv_backend::{MpvBackend, TrackInfo};
use crate::player::thumbnail::{ThumbnailGenerator, ThumbnailResult};
use crate::settings::Settings;
mod seek_preview;
mod signals;
mod tracks;
mod zones;

use seek_preview::{build_seek_preview, wire_seek_hover, wire_thumb_consumer};
use signals::wire_signals;
use zones::{build_center_box, build_end_box, build_start_box, ButtonLayoutWidgets};

/// Shared, interior-mutable handle to the live mpv backend (set by the shell
/// once the surface realizes; `None` before that). The popover callbacks read
/// it, so each can drive mpv without owning a clone of the backend.
type MpvSlot = Rc<RefCell<Option<Arc<MpvBackend>>>>;

/// Construction parameters for [`VideoControlsBar`].
pub struct VideoControlsBarInit {
    /// Player settings (volume max, stored noise strength, NR dialog).
    pub settings: Settings,
    /// Shared mpv handle the popover callbacks read at call time.
    pub mpv: MpvSlot,
}

/// Display state pushed into the bar by the shell.
#[derive(Debug)]
pub enum VideoControlsBarInput {
    /// Enable/disable the transport controls (play/prev/next/seek/scrub/tracks).
    SetControlsSensitive(bool),
    /// Reflect the paused state on the play button icon.
    SetPaused(bool),
    /// Set the elapsed-position label text.
    SetPosition(String),
    /// Set the total-duration label text.
    SetDuration(String),
    /// Set the scrub bar value (`0.0..=1.0`); does not emit a seek intent.
    SetSeekFraction(f64),
    /// Populate the subtitle + audio-track menus from the current mpv tracks.
    SetTracks(Vec<TrackInfo>),
    /// Set the media duration in seconds (drives the hover-preview time + the
    /// chapter marks, which are placed as `time/duration` fractions).
    SetDurationSecs(f64),
    /// Set the chapter list (`time_secs`, `title`); redraws the seek-bar marks
    /// and feeds the hover preview's chapter-name line.
    SetChapters(Vec<(f64, String)>),
    /// Set the path of the loaded media (so the hover preview can render a
    /// thumbnail frame); `None` clears it (e.g. a remote stream).
    SetCurrentFile(Option<String>),
    /// Trim mode: the `(start, end)` cut ranges to paint over the seek bar so the
    /// user edits the cuts on the real timeline (empty = clear).
    SetCutSegments(Vec<(f64, f64)>),
    /// Trim mode on/off — reveals the cut overlay + makes the seek bar taller.
    SetEditMode(bool),
}

/// User intents emitted by the bar for the shell to act on.
#[derive(Debug)]
pub enum VideoControlsBarOutput {
    /// Play/pause toggled.
    TogglePlay,
    /// Skip to the next item.
    Next,
    /// Skip to the previous item.
    Previous,
    /// Scrub bar moved by the user; carries the raw `0.0..=1.0` fraction.
    SeekChanged(f64),
    /// Fullscreen button clicked.
    ToggleFullscreen,
}

/// App-local OSD controls bar (recovers the old `controls_box`).
pub struct VideoControlsBar {
    position_label: gtk::Label,
    duration_label: gtk::Label,
    seek_bar: gtk::Scale,
    play_btn: gtk::Button,
    prev_btn: gtk::Button,
    next_btn: gtk::Button,
    subtitle_btn: gtk::MenuButton,
    audio_track_btn: gtk::MenuButton,
    /// Guard so programmatic scrub updates do not echo as user seek intents.
    suppress_seek: Rc<std::cell::Cell<bool>>,
    /// Live mpv handle shared with the popover callbacks (for track menus).
    mpv: MpvSlot,
    /// The hover popovers (volume / speed / voice) built in the start box.
    /// Exposed so the shell's OSD auto-hide can keep the OSD shown while any of
    /// them is open (else the auto-hide timer dismisses the popover mid-hover).
    hover_popovers: Vec<gtk::Popover>,
    /// Bottom OSD button-zone handles used by the Appearance dialog's live
    /// layout editor.
    button_layout: ButtonLayoutWidgets,
    /// Trim-mode cut ranges painted over the seek bar (shared with the overlay
    /// draw closure).
    cut_segments: Rc<RefCell<Vec<(f64, f64)>>>,
    /// Input-transparent overlay painting [`Self::cut_segments`] on the seek bar.
    cut_overlay: gtk::DrawingArea,
    /// Media duration in seconds (hover-preview time + chapter-mark fractions).
    duration_secs: Rc<Cell<f64>>,
    /// Chapter list (`time_secs`, `title`): seek-bar marks + hover chapter name.
    chapters: Rc<RefCell<Vec<(f64, String)>>>,
    /// Path of the loaded media, shared with the thumbnail worker thread.
    current_file: Arc<Mutex<Option<String>>>,
    /// Hover-preview thumbnail generator (ffmpeg-backed, on a worker thread).
    /// Kept alive for the bar's lifetime; dropping it stops the worker.
    _thumb_gen: Rc<ThumbnailGenerator>,
}

impl Component for VideoControlsBar {
    type Init = VideoControlsBarInit;
    type Input = VideoControlsBarInput;
    type Output = VideoControlsBarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 4);
        root.add_css_class("osd-controls");
        root
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let suppress_seek = Rc::new(std::cell::Cell::new(false));

        let duration_secs = Rc::new(Cell::new(0.0_f64));
        let chapters: Rc<RefCell<Vec<(f64, String)>>> = Rc::new(RefCell::new(Vec::new()));
        let current_file: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

        // Hover-preview thumbnail worker (ffmpeg-backed). Results stream back on
        // an async channel and paint the preview image while it is visible.
        let (thumb_tx, thumb_rx) = async_channel::bounded::<ThumbnailResult>(4);
        let thumb_gen = Rc::new(ThumbnailGenerator::new(thumb_tx, current_file.clone()));

        // ── seek row: position · scale · duration ───────────────────────────
        let seek_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(4)
            .margin_end(4)
            .build();

        let position_label = gtk::Label::builder()
            .label("0:00")
            .width_chars(7)
            .xalign(1.0)
            .css_classes(["time-label"])
            .build();
        position_label
            .update_property(&[gtk::accessible::Property::Label(&i18n("Playback position"))]);
        seek_row.append(&position_label);

        let seek_adj = gtk::Adjustment::new(0.0, 0.0, 1.0, 0.01, 0.1, 0.0);
        let seek_bar = gtk::Scale::builder()
            .adjustment(&seek_adj)
            .hexpand(true)
            .draw_value(false)
            .css_classes(["seek-scale"])
            .build();
        seek_bar.update_property(&[gtk::accessible::Property::Label(&i18n("Seek Position"))]);

        // Trim-mode cut overlay: an input-transparent DrawingArea stacked over the
        // seek bar that paints the marked cut ranges, so the user reads + edits the
        // cuts on the real timeline instead of a separate one. Hidden until trim
        // mode reveals it.
        let cut_segments: Rc<RefCell<Vec<(f64, f64)>>> = Rc::new(RefCell::new(Vec::new()));
        let cut_overlay = gtk::DrawingArea::new();
        cut_overlay.set_can_target(false);
        cut_overlay.set_visible(false);
        install_cut_overlay_draw(&cut_overlay, &cut_segments, &duration_secs);
        let seek_overlay = gtk::Overlay::builder()
            .hexpand(true)
            .child(&seek_bar)
            .build();
        seek_overlay.add_overlay(&cut_overlay);
        seek_row.append(&seek_overlay);

        let duration_label = gtk::Label::builder()
            .label("0:00")
            .width_chars(7)
            .xalign(0.0)
            .css_classes(["time-label"])
            .build();
        seek_row.append(&duration_label);
        root.append(&seek_row);

        // ── hover preview: thumbnail frame + time (+ chapter) at the cursor ──
        let (preview_popover, preview_thumb, preview_label) = build_seek_preview(&seek_bar);
        wire_seek_hover(
            &seek_bar,
            &preview_popover,
            &preview_thumb,
            &preview_label,
            &duration_secs,
            &chapters,
            &current_file,
            &thumb_gen,
            &init.settings,
        );
        wire_thumb_consumer(thumb_rx, &preview_popover, &preview_thumb);

        // ── center box ──────────────────────────────────────────────────────
        let center = gtk::CenterBox::new();

        let (start_box, volume_box, speed_box, voice_box, hover_popovers) =
            build_start_box(&init.settings, &init.mpv);
        center.set_start_widget(Some(&start_box));

        let (controls_center, prev_btn, play_btn, next_btn) = build_center_box();
        center.set_center_widget(Some(&controls_center));

        let (end_box, subtitle_btn, audio_track_btn, fullscreen_btn) = build_end_box();
        center.set_end_widget(Some(&end_box));
        let button_layout = ButtonLayoutWidgets {
            start_zone: start_box,
            center_zone: controls_center.clone(),
            end_zone: end_box,
            volume_box,
            speed_box,
            voice_box,
            prev_btn: prev_btn.clone(),
            play_btn: play_btn.clone(),
            next_btn: next_btn.clone(),
            subtitle_btn: subtitle_btn.clone(),
            audio_track_btn: audio_track_btn.clone(),
            fullscreen_btn: fullscreen_btn.clone(),
        };

        root.append(&center);

        wire_signals(
            &sender,
            &seek_bar,
            &prev_btn,
            &play_btn,
            &next_btn,
            &fullscreen_btn,
            &suppress_seek,
        );

        let model = Self {
            position_label,
            duration_label,
            seek_bar,
            play_btn,
            prev_btn,
            next_btn,
            subtitle_btn,
            audio_track_btn,
            suppress_seek,
            mpv: init.mpv,
            hover_popovers,
            button_layout,
            cut_segments,
            cut_overlay,
            duration_secs,
            chapters,
            current_file,
            _thumb_gen: thumb_gen,
        };
        model.set_sensitive(false);
        model.apply_button_layout(&init.settings);

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoControlsBarInput::SetControlsSensitive(on) => self.set_sensitive(on),
            VideoControlsBarInput::SetPaused(paused) => {
                let icon = if paused {
                    "media-playback-start-symbolic"
                } else {
                    "media-playback-pause-symbolic"
                };
                self.play_btn.set_icon_name(icon);
            }
            VideoControlsBarInput::SetPosition(text) => self.position_label.set_label(&text),
            VideoControlsBarInput::SetDuration(text) => self.duration_label.set_label(&text),
            VideoControlsBarInput::SetSeekFraction(value) => {
                self.suppress_seek.set(true);
                self.seek_bar.set_value(value);
                self.suppress_seek.set(false);
            }
            VideoControlsBarInput::SetTracks(tracks) => self.populate_track_menus(&tracks),
            VideoControlsBarInput::SetDurationSecs(secs) => {
                self.duration_secs.set(secs);
                self.render_chapter_marks();
                self.cut_overlay.queue_draw();
            }
            VideoControlsBarInput::SetCutSegments(segments) => {
                self.cut_segments.replace(segments);
                self.cut_overlay.queue_draw();
            }
            VideoControlsBarInput::SetEditMode(on) => {
                self.cut_overlay.set_visible(on);
                if on {
                    self.seek_bar.add_css_class("trim-editing");
                } else {
                    self.seek_bar.remove_css_class("trim-editing");
                }
                self.cut_overlay.queue_draw();
            }
            VideoControlsBarInput::SetChapters(chapters) => {
                self.chapters.replace(chapters);
                self.render_chapter_marks();
            }
            VideoControlsBarInput::SetCurrentFile(path) => {
                *self
                    .current_file
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = path;
            }
        }
    }
}

impl VideoControlsBar {
    /// The hover popovers (volume / speed / voice) the OSD auto-hide must
    /// inspect so it does not dismiss one mid-hover. Long-lived (built once),
    /// safe to clone for the shell's `OsdState`.
    pub fn popovers(&self) -> Vec<gtk::Popover> {
        self.hover_popovers.clone()
    }

    /// The subtitle + audio-track menu buttons whose open popover must keep the
    /// OSD shown — else the auto-hide timer dismisses the track menu mid-pick,
    /// so the selector appears to "only work while paused" (paused keeps the OSD
    /// shown). Mirrors the old `hide_controls` bail on
    /// `subtitle_btn.is_active() || audio_track_btn.is_active()`.
    pub fn track_menu_buttons(&self) -> Vec<gtk::MenuButton> {
        vec![self.subtitle_btn.clone(), self.audio_track_btn.clone()]
    }

    pub(crate) fn button_layout(&self) -> ButtonLayoutWidgets {
        self.button_layout.clone()
    }

    pub(crate) fn apply_button_layout(&self, settings: &Settings) {
        self.button_layout.apply_settings(settings);
    }

    fn set_sensitive(&self, on: bool) {
        self.play_btn.set_sensitive(on);
        self.prev_btn.set_sensitive(on);
        self.next_btn.set_sensitive(on);
        self.seek_bar.set_sensitive(on);
        self.subtitle_btn.set_sensitive(on);
        self.audio_track_btn.set_sensitive(on);
    }

    /// Redraw the seek-bar chapter marks. The scale runs `0.0..=1.0`, so each
    /// chapter is placed at `time/duration`. No-op until the duration is known.
    fn render_chapter_marks(&self) {
        self.seek_bar.clear_marks();
        let duration = self.duration_secs.get();
        if duration <= 0.0 {
            return;
        }
        for (time, _title) in self.chapters.borrow().iter() {
            self.seek_bar.add_mark(
                (time / duration).clamp(0.0, 1.0),
                gtk::PositionType::Top,
                None,
            );
        }
    }
}

/// Paint the trim-mode cut ranges as accent bands over the seek bar, scaled by
/// the media duration. Input-transparent, so the seek scale still scrubs.
fn install_cut_overlay_draw(
    area: &gtk::DrawingArea,
    cut_segments: &Rc<RefCell<Vec<(f64, f64)>>>,
    duration_secs: &Rc<Cell<f64>>,
) {
    let cut_segments = cut_segments.clone();
    let duration_secs = duration_secs.clone();
    area.set_draw_func(move |_area, cr, width, height| {
        let duration = duration_secs.get().max(0.001);
        let w = f64::from(width);
        let h = f64::from(height);
        // Recolour the seek trough itself across each cut range, in the Adwaita
        // warning colour (#e5a50a). The band spans most of the seek widget height
        // so it covers the trough wherever GTK centres it (the slider's negative
        // margin shifts the trough off the widget centre), reading as part of the
        // bar rather than a separate track.
        let band_top = h * 0.15;
        let band_h = h * 0.70;
        let x_for = |t: f64| (t / duration).clamp(0.0, 1.0) * w;
        for (start, end) in cut_segments.borrow().iter() {
            let x0 = x_for(*start);
            let x1 = x_for(*end);
            cr.set_source_rgba(0.898, 0.647, 0.039, 0.85);
            cr.rectangle(x0, band_top, (x1 - x0).max(2.0), band_h);
            let _ = cr.fill();
        }
    });
}
