//! `VideoTrimBar` — the interactive bottom trim bar: a segment timeline, the
//! mark/clear controls, the fast/precise format choice and the export button.
//!
//! A Relm4 [`Component`]: it owns the [`TrimState`] and renders it; the shell
//! feeds it playhead/duration and acts on its typed [`VideoTrimBarOutput`]
//! (seek preview / export / exit). The shell — not this bar — owns the mpv
//! handle and the export queue.
//!
//! Marking: the bar receives the live playhead via [`VideoTrimBarInput::SetPlayhead`]
//! every position tick, so its on-screen buttons mark at that cached position
//! (visually exact). The keyboard shortcuts route through the shell, which reads
//! mpv's `time-pos` directly and sends [`VideoTrimBarInput::MarkIn`] etc. with a
//! frame-exact time.

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
    /// Media duration changed (seconds) — rescales the timeline.
    SetDuration(f64),
    /// Live playhead position (seconds) — moves the timeline cursor.
    SetPlayhead(f64),
    /// Mark the in-point at this exact (mpv-read) position.
    MarkIn(f64),
    /// Mark the out-point at this exact position, committing a segment.
    MarkOut(f64),
    /// `M`: toggle in→out at this position.
    QuickMark(f64),
    /// Mark in / out / quick at the bar's cached playhead (on-screen buttons).
    MarkInHere,
    /// See [`Self::MarkInHere`].
    MarkOutHere,
    /// See [`Self::MarkInHere`].
    QuickMarkHere,
    /// Delete the chip/segment with this id.
    DeleteSegment(u64),
    /// Drop every segment.
    ClearSegments,
    /// Switch fast (keep-format) vs precise (re-encode).
    SetMode(CutMode),
    /// Build the export request from the current state and emit it.
    RequestExport,
    /// Disable controls while an export runs (and re-enable after).
    SetBusy(bool),
    /// Internal: the timeline was clicked at this fraction `[0,1]`.
    TimelineSeek(f64),
}

/// Typed effects the shell acts on.
#[derive(Debug)]
pub enum VideoTrimBarOutput {
    /// Seek the player to this position (timeline click / chip preview).
    SeekPreview(f64),
    /// Export the marked ranges with the chosen mode/output layout.
    ExportRequested {
        /// `(start, end)` ranges, sorted + merged.
        ranges: Vec<(f64, f64)>,
        /// Fast (stream-copy) or precise (re-encode).
        mode: CutMode,
        /// Each range to its own file vs joined into one.
        split: bool,
    },
    /// The fast/precise choice changed (the shell reveals the encode panel on
    /// precise).
    ModeChanged(CutMode),
    /// Leave trim mode.
    Exit,
}

/// The trim bar component.
pub struct VideoTrimBar {
    /// Shared with the timeline draw closure; mutated in `update`.
    state: Rc<RefCell<TrimState>>,
    timeline: gtk::DrawingArea,
    segments_strip: gtk::Box,
    segments_scroll: gtk::ScrolledWindow,
    hint_label: gtk::Label,
    count_label: gtk::Label,
    export_btn: gtk::Button,
    split_toggle: gtk::ToggleButton,
    /// Controls greyed out while exporting.
    busy_sensitive: Vec<gtk::Widget>,
}

impl VideoTrimBar {
    fn refresh(&self, sender: &ComponentSender<Self>) {
        let state = self.state.borrow();
        self.timeline.queue_draw();

        while let Some(child) = self.segments_strip.first_child() {
            self.segments_strip.remove(&child);
        }
        for segment in &state.segments {
            let chip = build_segment_chip(segment.id, segment.start, segment.end, sender);
            self.segments_strip.append(&chip);
        }
        let has_segments = !state.segments.is_empty();
        self.segments_scroll.set_visible(has_segments);
        self.count_label.set_visible(has_segments);
        self.count_label
            .set_label(&format!("{} ✂", state.segments.len()));

        self.hint_label.set_label(&match state.pending_in {
            Some(start) => {
                i18n("In: {t} — set the out-point").replace("{t}", &playback_time(start))
            }
            None if has_segments => i18n("Mark more cuts or export"),
            None => i18n("Press I / O / M to mark a cut"),
        });
        self.export_btn
            .set_sensitive(state.is_exportable() && !state.busy);
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

        // ── timeline ────────────────────────────────────────────────────────
        let timeline = gtk::DrawingArea::builder()
            .height_request(38)
            .hexpand(true)
            .build();
        timeline.update_property(&[gtk::accessible::Property::Label(&i18n("Cut timeline"))]);
        install_timeline_draw(&timeline, &state);
        let click = gtk::GestureClick::new();
        click.connect_released(glib::clone!(
            #[strong]
            sender,
            #[weak]
            timeline,
            move |_, _, x, _| {
                let width = f64::from(timeline.width()).max(1.0);
                sender.input(VideoTrimBarInput::TimelineSeek((x / width).clamp(0.0, 1.0)));
            }
        ));
        timeline.add_controller(click);

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
        let in_btn = labeled_button("pan-start-symbolic", &i18n("Set In"), &["flat"]);
        let out_btn = labeled_button("pan-end-symbolic", &i18n("Set Out"), &["flat"]);
        let mark_btn = labeled_button("bookmark-new-symbolic", &i18n("Mark"), &["flat"]);
        let clear_btn = icon_button(
            "user-trash-symbolic",
            &i18n("Clear all cuts"),
            &["flat", "circular"],
        );
        in_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::MarkInHere)
        ));
        out_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::MarkOutHere)
        ));
        mark_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::QuickMarkHere)
        ));
        clear_btn.connect_clicked(glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(VideoTrimBarInput::ClearSegments)
        ));

        let count_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .build();
        let hint_label = gtk::Label::builder()
            .css_classes(["dim-label", "caption"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .xalign(0.0)
            .build();

        let mark_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        mark_row.append(&in_btn);
        mark_row.append(&out_btn);
        mark_row.append(&mark_btn);
        mark_row.append(&clear_btn);
        mark_row.append(&segments_scroll);
        mark_row.append(&count_label);

        // ── format choice + export ──────────────────────────────────────────
        let fast_btn = gtk::ToggleButton::builder()
            .label(i18n("Keep format (fast)"))
            .active(true)
            .build();
        let precise_btn = gtk::ToggleButton::builder()
            .label(i18n("Re-encode"))
            .build();
        precise_btn.set_group(Some(&fast_btn));
        // A grouped (radio) ToggleButton does not expose its text label as the
        // accessible name on its own — set it explicitly for screen readers.
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

        let split_toggle = gtk::ToggleButton::builder()
            .icon_name("view-paged-symbolic")
            .css_classes(["flat"])
            .build();
        split_toggle.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Save each cut as a separate file",
        ))]);

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
        export_row.append(&split_toggle);
        export_row.append(&export_btn);
        export_row.append(&exit_btn);

        root.append(&timeline);
        root.append(&mark_row);
        root.append(&export_row);

        let busy_sensitive: Vec<gtk::Widget> = vec![
            in_btn.upcast::<gtk::Widget>(),
            out_btn.upcast::<gtk::Widget>(),
            mark_btn.upcast::<gtk::Widget>(),
            clear_btn.upcast::<gtk::Widget>(),
            fast_btn.upcast::<gtk::Widget>(),
            precise_btn.upcast::<gtk::Widget>(),
            split_toggle.clone().upcast::<gtk::Widget>(),
        ];

        let model = VideoTrimBar {
            state,
            timeline,
            segments_strip,
            segments_scroll,
            hint_label,
            count_label,
            export_btn,
            split_toggle,
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
                self.timeline.queue_draw();
                return;
            }
            VideoTrimBarInput::MarkIn(t) => self.state.borrow_mut().set_in(t),
            VideoTrimBarInput::MarkOut(t) => {
                self.state.borrow_mut().set_out(t);
            }
            VideoTrimBarInput::QuickMark(t) => {
                self.state.borrow_mut().quick_mark(t);
            }
            VideoTrimBarInput::MarkInHere => {
                let t = self.state.borrow().playhead;
                self.state.borrow_mut().set_in(t);
            }
            VideoTrimBarInput::MarkOutHere => {
                let t = self.state.borrow().playhead;
                self.state.borrow_mut().set_out(t);
            }
            VideoTrimBarInput::QuickMarkHere => {
                let t = self.state.borrow().playhead;
                self.state.borrow_mut().quick_mark(t);
            }
            VideoTrimBarInput::DeleteSegment(id) => {
                self.state.borrow_mut().delete(id);
            }
            VideoTrimBarInput::ClearSegments => self.state.borrow_mut().clear(),
            VideoTrimBarInput::SetMode(mode) => {
                self.state.borrow_mut().mode = mode;
                let _ = sender.output(VideoTrimBarOutput::ModeChanged(mode));
            }
            VideoTrimBarInput::RequestExport => {
                let (ranges, mode) = {
                    let state = self.state.borrow();
                    (state.export_ranges(), state.mode)
                };
                if !ranges.is_empty() {
                    let _ = sender.output(VideoTrimBarOutput::ExportRequested {
                        ranges,
                        mode,
                        split: self.split_toggle.is_active(),
                    });
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
            VideoTrimBarInput::TimelineSeek(frac) => {
                let pos = frac * self.state.borrow().duration;
                let _ = sender.output(VideoTrimBarOutput::SeekPreview(pos));
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

/// Build one segment chip: "00:05 – 00:12" with a delete button; clicking the
/// label previews (seeks to) the segment start.
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

    let preview_btn = gtk::Button::builder()
        .label(format!("{} – {}", playback_time(start), playback_time(end)))
        .css_classes(["flat", "caption"])
        .build();
    preview_btn.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| {
            let _ = sender.output(VideoTrimBarOutput::SeekPreview(start));
        }
    ));

    let delete = icon_button("window-close-symbolic", &i18n("Remove this cut"), &["flat"]);
    delete.connect_clicked(glib::clone!(
        #[strong]
        sender,
        move |_| sender.input(VideoTrimBarInput::DeleteSegment(id))
    ));

    chip.append(&preview_btn);
    chip.append(&delete);
    chip
}

/// Install the cairo draw function painting the timeline track, the committed
/// segment ranges, the pending in→playhead selection and the playhead cursor.
fn install_timeline_draw(area: &gtk::DrawingArea, state: &Rc<RefCell<TrimState>>) {
    let state = state.clone();
    area.set_draw_func(move |_area, cr, width, height| {
        let state = state.borrow();
        let w = f64::from(width);
        let h = f64::from(height);
        let duration = state.duration.max(0.001);
        let x_for = |t: f64| (t / duration).clamp(0.0, 1.0) * w;

        cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
        rounded_rect(cr, 0.0, h * 0.30, w, h * 0.40, 4.0);
        let _ = cr.fill();

        for segment in &state.segments {
            let x0 = x_for(segment.start);
            let x1 = x_for(segment.end);
            cr.set_source_rgba(0.20, 0.52, 0.89, 0.85);
            rounded_rect(cr, x0, h * 0.22, (x1 - x0).max(2.0), h * 0.56, 4.0);
            let _ = cr.fill();
        }

        if let Some(start) = state.pending_in {
            let x0 = x_for(start);
            let x1 = x_for(state.playhead.max(start));
            cr.set_source_rgba(0.20, 0.52, 0.89, 0.35);
            cr.rectangle(x0, h * 0.18, (x1 - x0).max(1.0), h * 0.64);
            let _ = cr.fill();
            cr.set_source_rgba(0.20, 0.52, 0.89, 1.0);
            cr.rectangle(x0, 0.0, 2.0, h);
            let _ = cr.fill();
        }

        let px = x_for(state.playhead);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
        cr.rectangle(px - 1.0, 0.0, 2.0, h);
        let _ = cr.fill();
    });
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    use std::f64::consts::PI;
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}
