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
use big_relm4_components::feedback::tooltip::icon_button;
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
    /// The raw per-segment ranges changed — repaint the transport seek bar.
    SegmentsChanged(Vec<(f64, f64)>),
    /// Open the manual time-edit form for this segment (seeded with its times).
    EditSegment {
        /// Segment id.
        id: u64,
        /// Current start (seconds).
        start: f64,
        /// Current end (seconds).
        end: f64,
    },
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
    config_btn: gtk::Button,
    /// Controls greyed out while exporting.
    busy_sensitive: Vec<gtk::Widget>,
}

impl VideoTrimBar {
    fn refresh(&self, sender: &ComponentSender<Self>) {
        let (ranges, has_segments, count, pending, exportable, busy) = {
            let state = self.state.borrow();
            (
                state.segment_ranges(),
                !state.segments.is_empty(),
                state.segments.len(),
                state.pending,
                state.is_exportable(),
                state.busy,
            )
        };

        while let Some(child) = self.segments_strip.first_child() {
            self.segments_strip.remove(&child);
        }
        {
            let state = self.state.borrow();
            for segment in &state.segments {
                let chip = build_segment_chip(segment.id, segment.start, segment.end, sender);
                self.segments_strip.append(&chip);
            }
        }
        self.segments_scroll.set_visible(has_segments);
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
        // Tell the shell to repaint the seek-bar cut overlay.
        let _ = sender.output(VideoTrimBarOutput::SegmentsChanged(ranges));
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
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.add_css_class("osd");
        root.add_css_class("toolbar");
        root.add_css_class("video-trim-bar");
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_bottom(12);
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

        // ── mark controls ───────────────────────────────────────────────────
        let mark_btn = labeled_button("bookmark-new-symbolic", &i18n("Mark"), &["flat"]);
        let clear_btn = icon_button(
            "user-trash-symbolic",
            &i18n("Clear all cuts"),
            &["flat", "circular"],
        );
        mark_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::MarkHere)
        ));
        clear_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::ClearSegments)
        ));

        let count_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();

        let mark_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        mark_row.append(&mark_btn);
        mark_row.append(&clear_btn);
        mark_row.append(&segments_scroll);
        mark_row.append(&count_label);

        // ── format choice + gear + export ───────────────────────────────────
        let hint_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .xalign(0.0)
            .build();

        let fast_btn = gtk::ToggleButton::builder()
            .label(i18n("Keep format (fast)"))
            .active(true)
            .build();
        let precise_btn = gtk::ToggleButton::builder()
            .label(i18n("Re-encode"))
            .build();
        precise_btn.set_group(Some(&fast_btn));
        fast_btn.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Keep format — fast stream-copy cut",
        ))]);
        precise_btn.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Re-encode — precise cut with format options",
        ))]);
        let mode_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        mode_box.add_css_class("linked");
        mode_box.append(&fast_btn);
        mode_box.append(&precise_btn);
        fast_btn.connect_toggled(glib::clone!(
            #[strong]
            sender,
            move |b| {
                if b.is_active() {
                    sender.input(VideoTrimBarInput::SetMode(CutMode::Fast));
                }
            }
        ));
        precise_btn.connect_toggled(glib::clone!(
            #[strong]
            sender,
            move |b| {
                if b.is_active() {
                    sender.input(VideoTrimBarInput::SetMode(CutMode::Precise));
                }
            }
        ));

        // Gear: re-encode settings. Disabled while "Keep format" is selected
        // (a stream-copy has no encode settings); opens a dialog on precise.
        let config_btn = icon_button(
            "emblem-system-symbolic",
            &i18n("Re-encode settings"),
            &["flat", "circular"],
        );
        config_btn.set_sensitive(false);
        config_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(VideoTrimBarOutput::OpenEncodeConfig);
            }
        ));

        let exit_btn = icon_button(
            "window-close-symbolic",
            &i18n("Exit trim mode"),
            &["flat", "circular"],
        );
        exit_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(VideoTrimBarOutput::Exit);
            }
        ));

        let export_btn = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("document-save-symbolic")
                    .label(i18n("Export"))
                    .build(),
            )
            .css_classes(["suggested-action", "pill"])
            .sensitive(false)
            .build();
        export_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::RequestExport)
        ));

        let export_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        export_row.append(&hint_label);
        export_row.append(&mode_box);
        export_row.append(&config_btn);
        export_row.append(&export_btn);
        export_row.append(&exit_btn);

        root.append(&mark_row);
        root.append(&export_row);

        let busy_sensitive: Vec<gtk::Widget> = vec![
            mark_btn.upcast::<gtk::Widget>(),
            clear_btn.upcast::<gtk::Widget>(),
            fast_btn.upcast::<gtk::Widget>(),
            precise_btn.upcast::<gtk::Widget>(),
        ];

        let model = VideoTrimBar {
            state,
            segments_strip,
            segments_scroll,
            hint_label,
            count_label,
            export_btn,
            config_btn,
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
            VideoTrimBarInput::SetMode(mode) => {
                self.state.borrow_mut().mode = mode;
                self.config_btn
                    .set_sensitive(matches!(mode, CutMode::Precise));
            }
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

/// A flat icon+label button (novice-clear); the label is the accessible name.
fn labeled_button(icon: &str, label: &str, css: &[&str]) -> gtk::Button {
    let button = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name(icon)
                .label(label)
                .build(),
        )
        .build();
    for class in css {
        button.add_css_class(class);
    }
    button
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

    let edit_btn = gtk::Button::builder()
        .label(format!("{} – {}", playback_time(start), playback_time(end)))
        .css_classes(["flat", "caption"])
        .build();
    edit_btn.update_property(&[gtk::accessible::Property::Description(&i18n(
        "Edit cut times",
    ))]);
    edit_btn.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| {
            let _ = sender.output(VideoTrimBarOutput::EditSegment { id, start, end });
        }
    ));

    let delete = icon_button("window-close-symbolic", &i18n("Remove this cut"), &["flat"]);
    delete.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| sender.input(VideoTrimBarInput::DeleteSegment(id))
    ));

    chip.append(&edit_btn);
    chip.append(&delete);
    chip
}
