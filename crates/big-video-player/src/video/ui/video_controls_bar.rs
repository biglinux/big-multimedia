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
use gtk::glib;
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
    /// The trim-bar widget, shown IN PLACE of the transport button row while trim
    /// mode is active (so the cut controls live inside the existing controls, not
    /// a separate bar).
    pub trim_content: gtk::Widget,
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
    /// Trim mode: each cut's `(id, start, end)` to paint on the cut timeline (and
    /// hit-test for edge dragging); empty = clear.
    SetCutSegments(Vec<(u64, f64, f64)>),
    /// Trim mode: the pending first mark (drawn as a green guide), if any.
    SetPendingMark(Option<f64>),
    /// Trim mode on/off — swaps the seek scale for the cut timeline + reveals the
    /// trim controls row.
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
    /// A cut's edge was dragged on the cut timeline → new `(start, end)`.
    SegmentEdgeAdjusted {
        /// Segment id.
        id: u64,
        /// New start (seconds).
        start: f64,
        /// New end (seconds).
        end: f64,
    },
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
    /// Trim-mode cuts `(id, start, end)` shown on the cut timeline + hit-tested
    /// for edge dragging.
    cut_segments: Rc<RefCell<Vec<(u64, f64, f64)>>>,
    /// Pending first mark (green guide) on the cut timeline.
    pending_mark: Rc<RefCell<Option<f64>>>,
    /// Live playhead fraction `0..=1` (drives the cut-timeline playhead).
    seek_fraction: Rc<Cell<f64>>,
    /// Dedicated cut timeline (a fully-owned DrawingArea) shown IN PLACE of the
    /// GtkScale during trim mode, so the cut ranges align exactly with the bar.
    cut_timeline: gtk::DrawingArea,
    /// Swaps the GtkScale ↔ the cut timeline in the seek row.
    bar_stack: gtk::Stack,
    /// Revealer for the trim controls — an EXTRA row above the transport (the
    /// transport play/volume stay; the cut controls are added, not swapped in).
    trim_controls_revealer: gtk::Revealer,
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

        // Trim mode swaps the GtkScale for a dedicated cut timeline (a fully-owned
        // DrawingArea) in the same seek row, so the cut ranges + playhead align
        // exactly with the bar (no GtkScale trough-geometry guessing) and the
        // timeline is the cut scrubber.
        let cut_segments: Rc<RefCell<Vec<(u64, f64, f64)>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_mark: Rc<RefCell<Option<f64>>> = Rc::new(RefCell::new(None));
        let seek_fraction = Rc::new(Cell::new(0.0_f64));
        let cut_timeline = gtk::DrawingArea::builder()
            .hexpand(true)
            .height_request(20)
            .build();
        cut_timeline.update_property(&[gtk::accessible::Property::Label(&i18n("Cut timeline"))]);
        install_cut_timeline_draw(
            &cut_timeline,
            &cut_segments,
            &pending_mark,
            &seek_fraction,
            &duration_secs,
        );
        wire_cut_timeline_interaction(&cut_timeline, &cut_segments, &duration_secs, &sender);
        let bar_stack = gtk::Stack::builder().hexpand(true).build();
        bar_stack.add_named(&seek_bar, Some("seek"));
        bar_stack.add_named(&cut_timeline, Some("cut"));
        bar_stack.set_visible_child_name("seek");
        seek_row.append(&bar_stack);

        let duration_label = gtk::Label::builder()
            .label("0:00")
            .width_chars(7)
            .xalign(0.0)
            .css_classes(["time-label"])
            .build();
        seek_row.append(&duration_label);
        root.append(&seek_row);

        // ── hover preview: thumbnail frame + time (+ chapter) at the cursor ──
        // Parented to the shared bar-stack so it works for BOTH the seek scale and
        // the cut timeline (the cut timeline keeps the original hover preview).
        let (preview_popover, preview_thumb, preview_label) =
            build_seek_preview(bar_stack.upcast_ref());
        for hover_anchor in [
            seek_bar.upcast_ref::<gtk::Widget>(),
            cut_timeline.upcast_ref(),
        ] {
            wire_seek_hover(
                hover_anchor,
                &preview_popover,
                &preview_thumb,
                &preview_label,
                &duration_secs,
                &chapters,
                &current_file,
                &thumb_gen,
                &init.settings,
            );
        }
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

        // Trim controls = an EXTRA row above the transport (revealed in trim mode).
        // The transport (play / volume / …) stays put — the cut controls are added,
        // not swapped in.
        let trim_controls_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(false)
            .child(&init.trim_content)
            .build();
        root.append(&trim_controls_revealer);
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
            pending_mark,
            seek_fraction,
            cut_timeline,
            bar_stack,
            trim_controls_revealer,
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
                self.seek_fraction.set(value);
                self.cut_timeline.queue_draw();
            }
            VideoControlsBarInput::SetTracks(tracks) => self.populate_track_menus(&tracks),
            VideoControlsBarInput::SetDurationSecs(secs) => {
                self.duration_secs.set(secs);
                self.render_chapter_marks();
                self.cut_timeline.queue_draw();
            }
            VideoControlsBarInput::SetCutSegments(segments) => {
                self.cut_segments.replace(segments);
                self.cut_timeline.queue_draw();
            }
            VideoControlsBarInput::SetPendingMark(mark) => {
                self.pending_mark.replace(mark);
                self.cut_timeline.queue_draw();
            }
            VideoControlsBarInput::SetEditMode(on) => {
                // Swap the GtkScale for the dedicated cut timeline + reveal the
                // trim controls row above the (kept) transport.
                self.bar_stack
                    .set_visible_child_name(if on { "cut" } else { "seek" });
                self.trim_controls_revealer.set_reveal_child(on);
                self.cut_timeline.queue_draw();
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

/// Draw the cut timeline: track + played portion + cut ranges (warning colour) +
/// the pending first mark (green guide) + the playhead. Fully owned, exact
/// geometry, so the cuts align with the bar.
fn install_cut_timeline_draw(
    area: &gtk::DrawingArea,
    cut_segments: &Rc<RefCell<Vec<(u64, f64, f64)>>>,
    pending_mark: &Rc<RefCell<Option<f64>>>,
    seek_fraction: &Rc<Cell<f64>>,
    duration_secs: &Rc<Cell<f64>>,
) {
    let cut_segments = cut_segments.clone();
    let pending_mark = pending_mark.clone();
    let seek_fraction = seek_fraction.clone();
    let duration_secs = duration_secs.clone();
    area.set_draw_func(move |area, cr, width, height| {
        let duration = duration_secs.get().max(0.001);
        let w = f64::from(width);
        let h = f64::from(height);
        let track_h = (h * 0.5).clamp(6.0, 12.0);
        let track_top = (h - track_h) / 2.0;
        let r = track_h / 2.0;
        let frac = seek_fraction.get().clamp(0.0, 1.0);
        let x_for = |t: f64| (t / duration).clamp(0.0, 1.0) * w;

        // Unplayed track: themed foreground at low alpha → a clearly visible
        // distinct background for the not-yet-played region (theme-aware).
        let fg = area.color();
        cr.set_source_rgba(
            f64::from(fg.red()),
            f64::from(fg.green()),
            f64::from(fg.blue()),
            0.28,
        );
        rounded_rect(cr, 0.0, track_top, w, track_h, r);
        let _ = cr.fill();
        // Played portion (accent blue).
        cr.set_source_rgba(0.20, 0.52, 0.89, 0.95);
        rounded_rect(cr, 0.0, track_top, frac * w, track_h, r);
        let _ = cr.fill();
        // Cut ranges (Adwaita warning colour) — square edges so the boundaries
        // read as exact cut points (not rounded).
        for (_, start, end) in cut_segments.borrow().iter() {
            let x0 = x_for(*start);
            let x1 = x_for(*end);
            cr.set_source_rgba(0.898, 0.647, 0.039, 0.95);
            cr.rectangle(x0, track_top, (x1 - x0).max(2.0), track_h);
            let _ = cr.fill();
        }
        // Pending first mark — a green guide line awaiting its pair.
        if let Some(mark) = *pending_mark.borrow() {
            cr.set_source_rgba(0.18, 0.76, 0.35, 1.0);
            cr.rectangle(x_for(mark) - 1.0, 0.0, 2.0, h);
            let _ = cr.fill();
        }
        // Playhead cursor.
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.rectangle(frac * w - 1.0, 0.0, 2.0, h);
        let _ = cr.fill();
    });
}

const EDGE_GRAB_PX: f64 = 8.0;

/// Which part of which cut is being dragged on the timeline.
#[derive(Clone, Copy)]
enum CutDrag {
    None,
    Start(u64),
    End(u64),
    Body {
        id: u64,
        anchor: f64,
        orig_start: f64,
        orig_end: f64,
    },
}

/// Cut-timeline pointer interaction (mirrors the audio editor's waveform): drag a
/// cut's start/end handle or its whole body to adjust it — the PLAYBACK POSITION
/// follows the drag so the exact cut point is visible — otherwise click/drag to
/// seek. The cursor shows a resize/move handle. The drag state persists for the
/// whole gesture, so a tiny movement no longer ends it.
fn wire_cut_timeline_interaction(
    area: &gtk::DrawingArea,
    cut_segments: &Rc<RefCell<Vec<(u64, f64, f64)>>>,
    duration_secs: &Rc<Cell<f64>>,
    sender: &ComponentSender<VideoControlsBar>,
) {
    let drag_state: Rc<Cell<CutDrag>> = Rc::new(Cell::new(CutDrag::None));

    // Press → seek, unless it lands on a cut handle/body (which starts a drag).
    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_pressed(glib::clone!(
        #[strong]
        sender,
        #[strong]
        cut_segments,
        #[strong]
        duration_secs,
        #[weak]
        area,
        move |_, _, x, _| {
            let w = f64::from(area.width()).max(1.0);
            let dur = duration_secs.get().max(0.001);
            let segs = cut_segments.borrow();
            if hit_handle(&segs, x, w, dur, EDGE_GRAB_PX).is_some()
                || hit_body(&segs, x, w, dur).is_some()
            {
                return;
            }
            let _ = sender.output(VideoControlsBarOutput::SeekChanged((x / w).clamp(0.0, 1.0)));
        }
    ));
    area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    drag.connect_drag_begin(glib::clone!(
        #[strong]
        drag_state,
        #[strong]
        cut_segments,
        #[strong]
        duration_secs,
        #[weak]
        area,
        move |gesture, start_x, _| {
            // Claim the sequence so a drag on the timeline scrubs / adjusts a cut
            // instead of bubbling to the video's window-move gesture.
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let w = f64::from(area.width()).max(1.0);
            let dur = duration_secs.get().max(0.001);
            let segs = cut_segments.borrow();
            let mode = if let Some((id, is_end)) = hit_handle(&segs, start_x, w, dur, EDGE_GRAB_PX)
            {
                if is_end {
                    CutDrag::End(id)
                } else {
                    CutDrag::Start(id)
                }
            } else if let Some(id) = hit_body(&segs, start_x, w, dur) {
                let (_, s, e) = segs
                    .iter()
                    .find(|(sid, _, _)| *sid == id)
                    .copied()
                    .unwrap_or((id, 0.0, 0.0));
                CutDrag::Body {
                    id,
                    anchor: (start_x / w).clamp(0.0, 1.0) * dur,
                    orig_start: s,
                    orig_end: e,
                }
            } else {
                CutDrag::None
            };
            drag_state.set(mode);
        }
    ));
    drag.connect_drag_update(glib::clone!(
        #[strong]
        sender,
        #[strong]
        drag_state,
        #[strong]
        cut_segments,
        #[strong]
        duration_secs,
        #[weak]
        area,
        move |gesture, offset_x, _| {
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let w = f64::from(area.width()).max(1.0);
            let dur = duration_secs.get().max(0.001);
            let frac = ((start_x + offset_x) / w).clamp(0.0, 1.0);
            let t = frac * dur;
            {
                let mut segs = cut_segments.borrow_mut();
                match drag_state.get() {
                    CutDrag::Start(id) => {
                        if let Some((_, s, e)) = segs.iter_mut().find(|(sid, _, _)| *sid == id) {
                            *s = t.min(*e - 0.1).max(0.0);
                        }
                    }
                    CutDrag::End(id) => {
                        if let Some((_, s, e)) = segs.iter_mut().find(|(sid, _, _)| *sid == id) {
                            *e = t.max(*s + 0.1).min(dur);
                        }
                    }
                    CutDrag::Body {
                        id,
                        anchor,
                        orig_start,
                        orig_end,
                    } => {
                        if let Some((_, s, e)) = segs.iter_mut().find(|(sid, _, _)| *sid == id) {
                            let len = (orig_end - orig_start).max(0.1);
                            let (mut ns, mut ne) =
                                (orig_start + (t - anchor), orig_end + (t - anchor));
                            if ns < 0.0 {
                                ns = 0.0;
                                ne = len;
                            }
                            if ne > dur {
                                ne = dur;
                                ns = (dur - len).max(0.0);
                            }
                            *s = ns;
                            *e = ne;
                        }
                    }
                    CutDrag::None => {}
                }
            }
            // Playback follows the drag (or scrubs when not on a cut).
            let _ = sender.output(VideoControlsBarOutput::SeekChanged(frac));
            area.queue_draw();
        }
    ));
    drag.connect_drag_end(glib::clone!(
        #[strong]
        sender,
        #[strong]
        drag_state,
        #[strong]
        cut_segments,
        #[weak]
        area,
        move |_, _, _| {
            let id = match drag_state.replace(CutDrag::None) {
                CutDrag::Start(id) | CutDrag::End(id) | CutDrag::Body { id, .. } => Some(id),
                CutDrag::None => None,
            };
            if let Some(id) = id {
                if let Some((_, s, e)) = cut_segments.borrow().iter().find(|(sid, _, _)| *sid == id)
                {
                    let _ = sender.output(VideoControlsBarOutput::SegmentEdgeAdjusted {
                        id,
                        start: *s,
                        end: *e,
                    });
                }
            }
            area.queue_draw();
        }
    ));
    area.add_controller(drag);

    // Cursor: resize on a handle, move on a body.
    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(glib::clone!(
        #[strong]
        cut_segments,
        #[strong]
        duration_secs,
        #[weak]
        area,
        move |_, x, _| {
            let w = f64::from(area.width()).max(1.0);
            let dur = duration_secs.get().max(0.001);
            let segs = cut_segments.borrow();
            let cursor = if let Some((_, is_end)) = hit_handle(&segs, x, w, dur, EDGE_GRAB_PX) {
                if is_end {
                    "e-resize"
                } else {
                    "w-resize"
                }
            } else if hit_body(&segs, x, w, dur).is_some() {
                "move"
            } else {
                "default"
            };
            area.set_cursor_from_name(Some(cursor));
        }
    ));
    area.add_controller(motion);
}

/// The cut handle `(id, is_end)` within `thresh` px of `x`, nearest first.
fn hit_handle(
    segments: &[(u64, f64, f64)],
    x: f64,
    w: f64,
    duration: f64,
    thresh: f64,
) -> Option<(u64, bool)> {
    let mut best: Option<(u64, bool, f64)> = None;
    for (id, start, end) in segments {
        for (is_end, t) in [(false, *start), (true, *end)] {
            let edge_x = (t / duration).clamp(0.0, 1.0) * w;
            let dist = (x - edge_x).abs();
            if dist <= thresh && best.is_none_or(|(_, _, best_dist)| dist < best_dist) {
                best = Some((*id, is_end, dist));
            }
        }
    }
    best.map(|(id, is_end, _)| (id, is_end))
}

/// The id of the cut whose body contains `x` (strictly between its handles).
fn hit_body(segments: &[(u64, f64, f64)], x: f64, w: f64, duration: f64) -> Option<u64> {
    segments.iter().find_map(|(id, start, end)| {
        let x0 = (start / duration).clamp(0.0, 1.0) * w;
        let x1 = (end / duration).clamp(0.0, 1.0) * w;
        (x > x0 && x < x1).then_some(*id)
    })
}

/// Append a rounded-rectangle sub-path to `cr` (used for the cut-timeline track).
fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, radius: f64) {
    use std::f64::consts::PI;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}
