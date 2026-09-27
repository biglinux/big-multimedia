//! App-local OSD controls bar.
//!
//! Layout:
//!   • seek row: position label · seek scale · duration label
//!   • center box:
//!       START  = volume / speed / voice hover popovers (shared builders)
//!       CENTER = previous · play/pause · next
//!       END    = subtitle popover button · audio-track popover button · fullscreen
//!
//! Push-model Component: buttons/scrub emit typed `VideoControlsBarOutput`
//! intents; the shell owns mpv/settings and pushes display state back via
//! `VideoControlsBarInput`. The volume/speed/voice popovers drive mpv
//! directly through callbacks captured at build time (the shell hands the bar a
//! live `mpv` accessor on `FileLoaded`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use big_player_ui::cut_timeline::{install_cut_timeline_draw, wire_cut_timeline_interaction};

use crate::i18n::i18n;
use crate::player::mpv_backend::{MpvBackend, TrackMetadata};
use crate::player::thumbnail::{ThumbnailGenerator, ThumbnailResult};
use crate::settings::Settings;
mod seek_preview;
mod signals;
mod tracks;

mod zones;

use seek_preview::{
    build_seek_preview, wire_neighbour_preview, wire_seek_hover, wire_thumb_consumer,
};
use signals::wire_signals;
use zones::{
    ButtonLayoutWidgets, StartBoxWidgets, build_center_box, build_end_box, build_start_box,
};

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
    /// While a TV plays this player's session, volume and mute drive it.
    pub tv: Rc<crate::shared::tv_cast::TvLink>,
    /// The trim-bar widget, shown IN PLACE of the transport button row while trim
    /// mode is active (so the cut controls live inside the existing controls, not
    /// a separate bar).
    pub trim_content: gtk::Widget,
}

/// Display state pushed into the bar by the shell.
#[derive(Debug)]
pub enum VideoControlsBarInput {
    /// A file is open (shows the seek row, transport and per-file menus) or
    /// not (the welcome screen).
    SetHasMedia(bool),
    /// Reflect the paused state on the play button icon.
    SetPaused(bool),
    /// Set the elapsed-position label text.
    SetPosition(String),
    /// Set the total-duration label text.
    SetDuration(String),
    /// Set the scrub bar value (`0.0..=1.0`); does not emit a seek intent.
    SetSeekFraction(f64),
    /// Populate the subtitle + audio-track menus from the current mpv tracks.
    SetTracks(Vec<TrackMetadata>),
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
    /// Mirror an external mixer volume change (pavucontrol etc.) onto the volume
    /// slider + readout, without pushing the value back to the sound server.
    SetVolume(f64),
    /// Mirror an external mixer mute change onto the volume button icon.
    SetMuted(bool),
    /// Whose volume the control moves: the TV's while a TV has the controls.
    SetVolumeOfTv(bool),
    /// The PC's own volume (percent) and mute, beside the TV's, while the
    /// player plays along with the TV; `None` hides it.
    ShowPcVolume(Option<(f64, bool)>),
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
    /// The elapsed-time button was activated → open the "Go to time" dialog.
    GotoTimeClicked,
}

/// App-local OSD controls bar.
pub struct VideoControlsBar {
    position_label: gtk::Label,
    duration_label: gtk::Label,
    seek_bar: gtk::Scale,
    play_button: gtk::Button,
    subtitle_menu_button: gtk::Button,
    subtitle_menu_popover: gtk::Popover,
    audio_track_menu_button: gtk::Button,
    audio_track_menu_popover: gtk::Popover,
    /// Volume slider + button (bottom-left) and the guard that mirrors external
    /// mixer changes onto them without echoing back to the sound server.
    volume: zones::VolumeControl,
    /// The PC's own volume, shown beside the TV's while the player plays along.
    pc_volume: zones::VolumeControl,
    /// Guard so programmatic scrub updates do not echo as user seek intents.
    suppress_seek: Rc<std::cell::Cell<bool>>,
    /// Position · seek bar · duration: hidden while no file is open.
    seek_row: gtk::Box,
    /// For re-applying the button layout when a file opens.
    settings: Settings,
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
    thumb_gen: Rc<ThumbnailGenerator>,
    preview_popover: gtk::Popover,
}

impl Drop for VideoControlsBar {
    fn drop(&mut self) {
        // GtkStack disposes pages before Widget::destroy; the preview is not a page.
        if self.preview_popover.parent().is_some() {
            self.preview_popover.unparent();
        }
    }
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
        let (thumb_tx, thumb_rx) = async_channel::bounded::<ThumbnailResult>(1);
        let thumb_gen = Rc::new(ThumbnailGenerator::new(thumb_tx));

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
        let position_button = gtk::Button::builder()
            .child(&position_label)
            .css_classes(["flat", "time-label-clickable"])
            .build();
        position_button.update_property(&[gtk::accessible::Property::Label(&i18n("Go to time"))]);
        crate::shared::tooltip::set(&position_button, &i18n("Go to time"));
        position_button.set_cursor_from_name(Some("pointer"));
        // The elapsed-time counter opens the "Go to time" dialog (the menu entry
        // was moved here). Keep it a real button so AT-SPI and keyboard users
        // have the same path as pointer users.
        {
            let sender = sender.clone();
            position_button.connect_clicked(move |_| {
                sender.output(VideoControlsBarOutput::GotoTimeClicked).ok();
            });
        }
        seek_row.append(&position_button);

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
        let seek_sender = sender.clone();
        let segment_sender = sender.clone();
        wire_cut_timeline_interaction(
            &cut_timeline,
            &cut_segments,
            &duration_secs,
            move |fraction| {
                let _ = seek_sender.output(VideoControlsBarOutput::SeekChanged(fraction));
            },
            move |adjustment| {
                let _ = segment_sender.output(VideoControlsBarOutput::SegmentEdgeAdjusted {
                    id: adjustment.id,
                    start: adjustment.start,
                    end: adjustment.end,
                });
            },
        );
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
        wire_thumb_consumer(thumb_rx, &preview_thumb, &thumb_gen);

        // ── center box ──────────────────────────────────────────────────────
        let center = gtk::CenterBox::new();

        let StartBoxWidgets {
            start_box,
            volume_zone,
            speed_box,
            voice_box,
            hover_popovers,
            readout_labels,
            volume,
            pc_volume,
        } = build_start_box(&init.settings, &init.mpv, &init.tv);
        center.set_start_widget(Some(&start_box));

        let (controls_center, previous_button, play_button, next_button) = build_center_box();
        wire_neighbour_preview(&previous_button, &init.mpv, -1);
        wire_neighbour_preview(&next_button, &init.mpv, 1);
        center.set_center_widget(Some(&controls_center));

        let end_widgets = build_end_box();
        center.set_end_widget(Some(&end_widgets.end_box));
        let button_layout = ButtonLayoutWidgets {
            start_zone: start_box,
            center_zone: controls_center.clone(),
            end_zone: end_widgets.end_box,
            volume_box: volume_zone,
            speed_box,
            voice_box,
            previous_button: previous_button.clone(),
            play_button: play_button.clone(),
            next_button: next_button.clone(),
            export_menu_button: end_widgets.export_menu_button,
            export_menu_popover: end_widgets.export_menu_popover,
            subtitle_menu_button: end_widgets.subtitle_menu_button.clone(),
            audio_track_menu_button: end_widgets.audio_track_menu_button.clone(),
            tv_button: end_widgets.tv_button.clone(),
            fullscreen_button: end_widgets.fullscreen_button.clone(),
            has_media: Rc::new(std::cell::Cell::new(false)),
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
        // Responsive OSD: below a narrow width, hide the value readouts (volume %,
        // speed, voice level) so the end-zone buttons never spill off the edge —
        // the icons and controls stay usable in a small window.
        let responsive = adw::BreakpointBin::builder()
            .width_request(320)
            .height_request(36)
            .child(&center)
            .build();
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            700.0,
            adw::LengthUnit::Sp,
        ));
        for label in &readout_labels {
            breakpoint.add_setter(label, "visible", Some(&false.to_value()));
        }
        responsive.add_breakpoint(breakpoint);
        root.append(&responsive);

        wire_signals(
            &sender,
            &seek_bar,
            &previous_button,
            &play_button,
            &next_button,
            &end_widgets.fullscreen_button,
            &suppress_seek,
        );

        let model = Self {
            seek_row: seek_row.clone(),
            settings: init.settings.clone(),
            position_label,
            duration_label,
            seek_bar,
            play_button,
            subtitle_menu_button: end_widgets.subtitle_menu_button,
            subtitle_menu_popover: end_widgets.subtitle_menu_popover,
            audio_track_menu_button: end_widgets.audio_track_menu_button,
            audio_track_menu_popover: end_widgets.audio_track_menu_popover,
            volume,
            pc_volume,
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
            thumb_gen,
            preview_popover,
        };
        model.set_has_media(false);

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoControlsBarInput::SetHasMedia(on) => self.set_has_media(on),
            VideoControlsBarInput::SetPaused(paused) => {
                let icon = if paused {
                    "media-playback-start-symbolic"
                } else {
                    "media-playback-pause-symbolic"
                };
                self.play_button.set_icon_name(icon);
                // It names what a click does, as the icon shows.
                let action = if paused { i18n("Play") } else { i18n("Pause") };
                self.play_button.set_tooltip_text(Some(&action));
                self.play_button
                    .update_property(&[gtk::accessible::Property::Label(&action)]);
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
                self.thumb_gen.invalidate();
                self.preview_popover.popdown();
                *self
                    .current_file
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = path;
            }
            VideoControlsBarInput::SetVolume(volume) => {
                // Mirror the mixer onto the slider; the guard keeps the scale's
                // handler from pushing the value straight back to the server.
                self.volume.suppress.set(true);
                self.volume.scale.set_value(volume);
                self.volume.suppress.set(false);
            }
            VideoControlsBarInput::SetMuted(muted) => {
                self.volume.sound.show(muted, self.volume.scale.value());
            }
            VideoControlsBarInput::ShowPcVolume(pc) => {
                if let Some((volume, muted)) = pc {
                    self.pc_volume.suppress.set(true);
                    self.pc_volume.scale.set_value(volume);
                    self.pc_volume.suppress.set(false);
                    self.pc_volume.sound.show(muted, volume);
                }
                self.pc_volume.volume_box.set_visible(pc.is_some());
                self.volume.mark.set_visible(pc.is_some());
                self.pc_volume.mark.set_visible(pc.is_some());
            }
            VideoControlsBarInput::SetVolumeOfTv(tv) => {
                let label = if tv {
                    i18n("TV volume")
                } else {
                    i18n("Volume")
                };
                if let Some(title) = &self.volume.title {
                    title.set_label(&label);
                }
                self.volume
                    .scale
                    .update_property(&[gtk::accessible::Property::Label(&label)]);
            }
        }
    }
}

impl VideoControlsBar {
    /// Popovers the OSD auto-hide must inspect so it does not dismiss one
    /// mid-hover or mid-menu-pick. Long-lived (built once), safe to clone for
    /// the shell's `OsdState`.
    pub fn popovers(&self) -> Vec<gtk::Popover> {
        let mut popovers = self.hover_popovers.clone();
        popovers.extend([
            self.button_layout.export_menu_popover.clone(),
            self.subtitle_menu_popover.clone(),
            self.audio_track_menu_popover.clone(),
        ]);
        popovers
    }

    pub(crate) fn button_layout(&self) -> ButtonLayoutWidgets {
        self.button_layout.clone()
    }

    pub(crate) fn apply_button_layout(&self, settings: &Settings) {
        self.button_layout.apply_settings(settings);
    }

    /// Show what acts on the open file only while one is: the seek row, the
    /// transport and the per-file menus. Volume, speed, voice and fullscreen
    /// stay, as they hold for the next file too.
    fn set_has_media(&self, on: bool) {
        self.seek_row.set_visible(on);
        self.button_layout.has_media.set(on);
        self.apply_button_layout(&self.settings);
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
