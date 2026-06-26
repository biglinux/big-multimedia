//! `VideoTrimBar` — the trim toolbar: a single Mark control, the segment chips,
//! the fast/precise format choice, the re-encode-settings gear and Export.
//!
//! A Relm4 [`Component`]: it owns the [`TrimState`] and renders the chips; the
//! cut ranges themselves are drawn on the **existing transport seek bar** (the
//! shell forwards [`VideoTrimBarOutput::SegmentsChanged`] there), not on a
//! separate timeline. The shell owns the mpv handle, the export queue and the
//! encode-settings dialog.
//!
//! Marking: the bar caches the live playhead (fed via [`VideoTrimBarInput::SetPlayhead`])
//! so its Mark button marks at that position; the keyboard shortcut routes through
//! the shell with a frame-exact mpv time. Two marks make a segment in either
//! order (mark the end first, then the start — still `[min, max]`).

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use big_media_components::time::playback_time;
use big_relm4_components::feedback::tooltip::{
    self, icon_button, icon_toggle_button, labeled_icon_button,
};
use big_relm4_components::layout::widget_container::clear_box_children;
use gtk::glib;
use relm4::{gtk, Component, ComponentParts, ComponentSender};

use crate::i18n::i18n;

use super::state::{CutMode, TrimState};

/// Messages into the trim bar (from the shell / its own widgets).
#[derive(Debug)]
pub enum VideoTrimBarInput {
    /// Media duration changed (seconds).
    SetDuration(f64),
    /// Live playhead position (seconds) — cached for the Mark button.
    SetPlayhead(f64),
    /// Mark at this exact (mpv-read) position (keyboard `M`).
    Mark(f64),
    /// Mark at the bar's cached playhead (the on-screen Mark button).
    MarkHere,
    /// Delete the segment with this id.
    DeleteSegment(u64),
    /// Drop every segment.
    ClearSegments,
    /// Replace the times of a segment (from the manual edit form).
    SetSegmentTimes(u64, f64, f64),
    /// Switch fast (keep-format) vs precise (re-encode).
    SetMode(CutMode),
    /// Build the export request from the current state and emit it.
    RequestExport,
    /// Disable controls while an export runs (and re-enable after).
    SetBusy(bool),
}

/// Typed effects the shell acts on.
#[derive(Debug)]
pub enum VideoTrimBarOutput {
    /// The cut state changed — repaint the cut timeline. Carries each segment's
    /// `(id, start, end)` (for edge-drag addressing) plus the pending first mark.
    SegmentsChanged {
        /// Per-segment `(id, start, end)`.
        segments: Vec<(u64, f64, f64)>,
        /// First mark awaiting its pair (drawn as a green guide), if any.
        pending: Option<f64>,
    },
    /// Open the manual time-edit form for this segment (seeded with its times).
    EditSegment {
        /// Segment id.
        id: u64,
        /// Current start (seconds).
        start: f64,
        /// Current end (seconds).
        end: f64,
    },
    /// Toggle "play only the marked cuts" (loop/skip to play just the segments).
    PlaySelection(bool),
    /// Open the re-encode settings dialog (gear, precise mode only).
    OpenEncodeConfig,
    /// Export the marked ranges with the chosen mode.
    ExportRequested {
        /// `(start, end)` ranges, sorted + merged.
        ranges: Vec<(f64, f64)>,
        /// Fast (stream-copy) or precise (re-encode).
        mode: CutMode,
    },
    /// Leave trim mode.
    Exit,
}

/// The trim bar component.
pub struct VideoTrimBar {
    state: Rc<RefCell<TrimState>>,
    segments_strip: gtk::Box,
    segments_scroll: gtk::ScrolledWindow,
    hint_label: gtk::Label,
    count_label: gtk::Label,
    export_btn: gtk::Button,
    /// Controls greyed out while exporting.
    busy_sensitive: Vec<gtk::Widget>,
}

impl VideoTrimBar {
    fn refresh(&self, sender: &ComponentSender<Self>) {
        let (segments, has_segments, count, pending, exportable, busy) = {
            let state = self.state.borrow();
            (
                state
                    .segments
                    .iter()
                    .map(|s| (s.id, s.start, s.end))
                    .collect::<Vec<_>>(),
                !state.segments.is_empty(),
                state.segments.len(),
                state.pending,
                state.is_exportable(),
                state.busy,
            )
        };

        clear_box_children(&self.segments_strip);
        {
            let state = self.state.borrow();
            for segment in &state.segments {
                let chip = build_segment_chip(segment.id, segment.start, segment.end, sender);
                self.segments_strip.append(&chip);
            }
        }
        // Chips and the hint share the middle slot: chips once cuts exist, the
        // hint (incl. the pending-mark feedback) otherwise.
        let show_chips = has_segments && pending.is_none();
        self.segments_scroll.set_visible(show_chips);
        self.hint_label.set_visible(!show_chips);
        self.count_label.set_visible(has_segments);
        self.count_label.set_label(&format!("{count} ✂"));

        self.hint_label.set_label(&match pending {
            Some(start) => {
                i18n("Cut start: {t} — mark the other end").replace("{t}", &playback_time(start))
            }
            None if has_segments => i18n("Mark more cuts or export"),
            None => i18n("Press M (or Mark) at the start and end of a cut"),
        });
        self.export_btn.set_sensitive(exportable && !busy);
        // Tell the shell to repaint the cut timeline (segments + pending mark).
        let _ = sender.output(VideoTrimBarOutput::SegmentsChanged { segments, pending });
    }
}

impl Component for VideoTrimBar {
    type Init = ();
    type Input = VideoTrimBarInput;
    type Output = VideoTrimBarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        // Flat, full-width bar docked above the transport (NOT a floating card):
        // styling lives in `.video-trim-bar` (no `.osd` rounded/translucent card,
        // no margins) so it reads as embedded in the player's bottom controls.
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("video-trim-bar");
        root
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let state = Rc::new(RefCell::new(TrimState::default()));

        // ── segment chips ───────────────────────────────────────────────────
        let segments_strip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let segments_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .hexpand(true)
            .visible(false)
            .child(&segments_strip)
            .build();

        // ── mark controls (every control carries a tooltip + accessible label
        // via the shared tooltip helpers) ───────────────────────────────────
        let mark_button = labeled_icon_button(
            "bookmark-new-symbolic",
            &i18n("Mark"),
            &i18n("Mark a cut point at the current position (M)"),
            &["flat"],
        );
        let clear_button = icon_button(
            "user-trash-symbolic",
            &i18n("Clear all cuts"),
            &["flat", "circular"],
        );
        mark_button.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::MarkHere)
        ));
        clear_button.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::ClearSegments)
        ));
        // Play only the marked cuts (loops/skips to play just the segments).
        let play_selection_button = icon_toggle_button(
            "media-playlist-repeat-symbolic",
            &i18n("Play only the marked cuts"),
            false,
            &["flat", "circular"],
        );
        play_selection_button.connect_toggled(glib::clone!(
            #[strong]
            sender,
            move |button| {
                let _ = sender.output(VideoTrimBarOutput::PlaySelection(button.is_active()));
            }
        ));

        let count_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();

        // Hint / chips share the expanding middle slot (one visible at a time).
        let hint_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .xalign(0.0)
            .build();

        // Format choice: an Adwaita ToggleGroup (segmented control) — proper
        // exclusivity + accessible names out of the box (grouped GtkToggleButtons
        // do not expose their text label as an accessible name).
        let mode_group = adw::ToggleGroup::new();
        mode_group.add(
            adw::Toggle::builder()
                .label(i18n("Keep format (fast)"))
                .build(),
        );
        mode_group.add(adw::Toggle::builder().label(i18n("Re-encode")).build());
        mode_group.set_active(0);
        tooltip::set(
            &mode_group,
            &i18n("Keep format copies the cut without re-encoding (fast); Re-encode applies the cut settings"),
        );
        mode_group.connect_active_notify(glib::clone!(
            #[strong]
            sender,
            move |group| {
                let mode = if group.active() == 1 {
                    CutMode::Precise
                } else {
                    CutMode::Fast
                };
                sender.input(VideoTrimBarInput::SetMode(mode));
            }
        ));

        // Gear: re-encode settings. Disabled while "Keep format" is selected
        // (a stream-copy has no encode settings); opens a dialog on precise.
        // Settings (gear) — always available: even in fast/keep-format mode the
        // user needs the output-layout choice (join cuts vs separate files). The
        // dialog shows that always + the encode options for precise.
        let cut_settings_button = icon_button(
            "emblem-system-symbolic",
            &i18n("Cut settings"),
            &["flat", "circular"],
        );
        cut_settings_button.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(VideoTrimBarOutput::OpenEncodeConfig);
            }
        ));

        let exit_trim_mode_button = icon_button(
            "window-close-symbolic",
            &i18n("Exit trim mode"),
            &["flat", "circular"],
        );
        exit_trim_mode_button.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(VideoTrimBarOutput::Exit);
            }
        ));

        let export_button = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("document-save-symbolic")
                    .label(i18n("Export"))
                    .build(),
            )
            .css_classes(["suggested-action", "pill"])
            .sensitive(false)
            .build();
        export_button.update_property(&[gtk::accessible::Property::Label(&i18n("Export"))]);
        tooltip::set(&export_button, &i18n("Export the marked cuts"));
        export_button.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::RequestExport)
        ));

        // One compact row: Mark · Clear · [hint/chips] · count · format · gear ·
        // Export · exit — halves the vertical space versus two rows.
        let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        middle.set_hexpand(true);
        middle.append(&hint_label);
        middle.append(&segments_scroll);

        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.set_valign(gtk::Align::Center);
        row.append(&mark_button);
        row.append(&clear_button);
        row.append(&play_selection_button);
        row.append(&middle);
        row.append(&count_label);
        row.append(&mode_group);
        row.append(&cut_settings_button);
        row.append(&export_button);
        row.append(&exit_trim_mode_button);
        root.append(&row);

        let busy_sensitive: Vec<gtk::Widget> = vec![
            mark_button.upcast::<gtk::Widget>(),
            clear_button.upcast::<gtk::Widget>(),
            mode_group.upcast::<gtk::Widget>(),
            cut_settings_button.clone().upcast::<gtk::Widget>(),
        ];

        let model = VideoTrimBar {
            state,
            segments_strip,
            segments_scroll,
            hint_label,
            count_label,
            export_btn: export_button,
            busy_sensitive,
        };
        model.refresh(&sender);
        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoTrimBarInput::SetDuration(dur) => self.state.borrow_mut().duration = dur,
            VideoTrimBarInput::SetPlayhead(pos) => {
                self.state.borrow_mut().playhead = pos;
                return;
            }
            VideoTrimBarInput::Mark(t) => {
                self.state.borrow_mut().mark(t);
            }
            VideoTrimBarInput::MarkHere => {
                let t = self.state.borrow().playhead;
                self.state.borrow_mut().mark(t);
            }
            VideoTrimBarInput::DeleteSegment(id) => {
                self.state.borrow_mut().delete(id);
            }
            VideoTrimBarInput::ClearSegments => self.state.borrow_mut().clear(),
            VideoTrimBarInput::SetSegmentTimes(id, a, b) => {
                self.state.borrow_mut().set_segment_times(id, a, b);
            }
            VideoTrimBarInput::SetMode(mode) => self.state.borrow_mut().mode = mode,
            VideoTrimBarInput::RequestExport => {
                let (ranges, mode) = {
                    let state = self.state.borrow();
                    (state.export_ranges(), state.mode)
                };
                if !ranges.is_empty() {
                    let _ = sender.output(VideoTrimBarOutput::ExportRequested { ranges, mode });
                }
                return;
            }
            VideoTrimBarInput::SetBusy(busy) => {
                self.state.borrow_mut().busy = busy;
                for widget in &self.busy_sensitive {
                    widget.set_sensitive(!busy);
                }
                self.export_btn
                    .set_sensitive(!busy && self.state.borrow().is_exportable());
                return;
            }
        }
        self.refresh(&sender);
    }
}

/// Build one segment chip: "00:05 – 00:12" (click to edit the times) + delete.
fn build_segment_chip(
    id: u64,
    start: f64,
    end: f64,
    sender: &ComponentSender<VideoTrimBar>,
) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    chip.add_css_class("card");
    chip.set_margin_top(2);
    chip.set_margin_bottom(2);

    let edit_segment_button = gtk::Button::builder()
        .label(format!("{} – {}", playback_time(start), playback_time(end)))
        .css_classes(["flat", "caption"])
        .build();
    edit_segment_button.update_property(&[gtk::accessible::Property::Description(&i18n(
        "Edit cut times",
    ))]);
    tooltip::set(&edit_segment_button, &i18n("Edit cut times"));
    edit_segment_button.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| {
            let _ = sender.output(VideoTrimBarOutput::EditSegment { id, start, end });
        }
    ));

    let delete_segment_button =
        icon_button("window-close-symbolic", &i18n("Remove this cut"), &["flat"]);
    delete_segment_button.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| sender.input(VideoTrimBarInput::DeleteSegment(id))
    ));

    chip.append(&edit_segment_button);
    chip.append(&delete_segment_button);
    chip
}
