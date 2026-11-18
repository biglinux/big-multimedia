//! Shared Relm4 audio trim controls for the player transport.
//!
//! The component owns only trim UI state: mark pairs, selected cut ranges,
//! and small edit/delete chips. Playback, seeking, mpv, and export stay in the
//! host shell.

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use big_media_components::time;
use big_relm4_components::feedback::tooltip;

use crate::cut_list::{Cut, CutList};

/// Localized labels consumed by [`AudioTrimBar`].
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigAudioTrimBarLabels {
    pub mark_cut_point: String,
    pub clear_cuts: String,
    pub play_only_marked_cuts: String,
    pub cuts: String,
    pub cut_start_template: String,
    pub mark_more_cuts_or_export: String,
    pub mark_hint: String,
    pub export_cuts: String,
    pub export_cuts_tooltip: String,
    pub exit_trim_mode: String,
    pub edit_cut_times: String,
    pub edit_cut: String,
    pub remove_this_cut: String,
}

impl BigAudioTrimBarLabels {
    /// Translate every label through `text`.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            mark_cut_point: text("Mark cut point"),
            clear_cuts: text("Clear cuts"),
            play_only_marked_cuts: text("Play only marked cuts"),
            cuts: text("cuts"),
            cut_start_template: text("Cut start: {t} - mark the other end"),
            mark_more_cuts_or_export: text("Mark more cuts or export"),
            mark_hint: text("Press Mark at the start and end of each cut"),
            export_cuts: text("Export cuts"),
            export_cuts_tooltip: text("Export the marked cuts"),
            exit_trim_mode: text("Exit trim mode"),
            edit_cut_times: text("Edit cut times"),
            edit_cut: text("Edit cut"),
            remove_this_cut: text("Remove this cut"),
        }
    }
}

/// Messages into [`AudioTrimBar`].
#[derive(Debug)]
pub enum AudioTrimBarInput {
    /// Set current media duration.
    SetDuration(f64),
    /// Set live playhead time.
    SetPlayhead(f64),
    /// Mark the current playhead.
    MarkHere,
    /// Delete one segment.
    DeleteSegment(u64),
    /// Clear all segments.
    ClearSegments,
    /// Replace a segment's times.
    SetSegmentTimes(u64, f64, f64),
    /// Export current ranges.
    RequestExport,
}

/// User actions emitted by [`AudioTrimBar`].
#[derive(Debug)]
pub enum AudioTrimBarOutput {
    /// Cut state changed; repaint the shared timeline.
    SegmentsChanged {
        /// `(id, start, end)` per cut, sorted and disjoint.
        segments: Vec<(u64, f64, f64)>,
        /// Pending first mark.
        pending: Option<f64>,
    },
    /// Open keyboard-accessible time edit for a segment.
    EditSegment {
        /// Segment id.
        id: u64,
        /// Segment start.
        start: f64,
        /// Segment end.
        end: f64,
    },
    /// Toggle play-only-cuts preview.
    PlaySelection(bool),
    /// Export clicked.
    ExportRequested {
        /// Sorted, merged ranges.
        ranges: Vec<(f64, f64)>,
    },
    /// Exit trim mode.
    Exit,
}

/// Relm4 trim controls row for audio playback.
pub struct AudioTrimBar {
    labels: BigAudioTrimBarLabels,
    cuts: CutList,
    playhead: f64,
    segments_strip: gtk::Box,
    segments_scroll: gtk::ScrolledWindow,
    hint_label: gtk::Label,
    count_label: gtk::Label,
    export_button: gtk::Button,
}

impl AudioTrimBar {
    fn refresh(&self, sender: &ComponentSender<Self>) {
        while let Some(child) = self.segments_strip.first_child() {
            self.segments_strip.remove(&child);
        }

        let cuts = self.cuts.cuts();
        let has_segments = !cuts.is_empty();
        let count = cuts.len();
        let pending = self.cuts.pending();

        for cut in cuts {
            self.segments_strip
                .append(&build_segment_chip(cut, sender, &self.labels));
        }

        self.segments_scroll
            .set_visible(has_segments && pending.is_none());
        self.count_label.set_visible(has_segments);
        self.count_label
            .set_label(&format!("{count} {}", self.labels.cuts));
        self.hint_label
            .set_visible(!has_segments || pending.is_some());
        self.hint_label.set_label(&match pending {
            Some(mark) => self
                .labels
                .cut_start_template
                .replace("{t}", &time::playback_time(mark)),
            None if has_segments => self.labels.mark_more_cuts_or_export.clone(),
            None => self.labels.mark_hint.clone(),
        });
        self.export_button.set_sensitive(has_segments);

        let _ = sender.output(AudioTrimBarOutput::SegmentsChanged {
            segments: cuts
                .iter()
                .map(|cut| (cut.id, cut.start, cut.end))
                .collect(),
            pending,
        });
    }
}

impl Component for AudioTrimBar {
    type Init = BigAudioTrimBarLabels;
    type Input = AudioTrimBarInput;
    type Output = AudioTrimBarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        root.add_css_class("audio-trim-bar");
        root.set_margin_start(12);
        root.set_margin_end(12);
        root.set_margin_top(6);
        root.set_margin_bottom(4);
        root
    }

    fn init(
        labels: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mark_button = gtk::Button::builder()
            .label(&labels.mark_cut_point)
            .css_classes(["suggested-action"])
            .valign(gtk::Align::Center)
            .build();
        mark_button.set_tooltip_text(Some(&labels.mark_cut_point));
        mark_button.update_property(&[gtk::accessible::Property::Label(&labels.mark_cut_point)]);
        mark_button.connect_clicked(gtk::glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(AudioTrimBarInput::MarkHere)
        ));

        let clear_button = tooltip::icon_button("edit-clear-all-symbolic", &labels.clear_cuts, &[]);
        clear_button.connect_clicked(gtk::glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(AudioTrimBarInput::ClearSegments)
        ));

        let play_selection_button = tooltip::icon_toggle_button(
            "media-playlist-consecutive-symbolic",
            &labels.play_only_marked_cuts,
            false,
            &[],
        );
        play_selection_button.connect_toggled(gtk::glib::clone!(
            #[strong]
            sender,
            move |button| {
                let _ = sender.output(AudioTrimBarOutput::PlaySelection(button.is_active()));
            }
        ));

        let count_label = gtk::Label::builder()
            .css_classes(["caption"])
            .valign(gtk::Align::Center)
            .visible(false)
            .build();

        let hint_label = gtk::Label::builder()
            .label(&labels.mark_hint)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .hexpand(true)
            .xalign(0.0)
            .valign(gtk::Align::Center)
            .css_classes(["caption"])
            .build();

        let segments_strip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let segments_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .min_content_width(220)
            .max_content_height(36)
            .hexpand(true)
            .visible(false)
            .child(&segments_strip)
            .build();

        let middle = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        middle.set_hexpand(true);
        middle.append(&count_label);
        middle.append(&hint_label);
        middle.append(&segments_scroll);

        let export_button = gtk::Button::builder()
            .label(&labels.export_cuts)
            .css_classes(["suggested-action"])
            .valign(gtk::Align::Center)
            .sensitive(false)
            .build();
        export_button.update_property(&[gtk::accessible::Property::Label(&labels.export_cuts)]);
        export_button.set_tooltip_text(Some(&labels.export_cuts_tooltip));
        export_button.connect_clicked(gtk::glib::clone!(
            #[strong]
            sender,
            move |_| sender.input(AudioTrimBarInput::RequestExport)
        ));

        let exit_button =
            tooltip::icon_button("window-close-symbolic", &labels.exit_trim_mode, &[]);
        exit_button.connect_clicked(gtk::glib::clone!(
            #[strong]
            sender,
            move |_| {
                let _ = sender.output(AudioTrimBarOutput::Exit);
            }
        ));

        root.append(&mark_button);
        root.append(&clear_button);
        root.append(&play_selection_button);
        root.append(&middle);
        root.append(&export_button);
        root.append(&exit_button);

        let model = Self {
            labels,
            cuts: CutList::default(),
            playhead: 0.0,
            segments_strip,
            segments_scroll,
            hint_label,
            count_label,
            export_button,
        };
        model.refresh(&sender);

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            AudioTrimBarInput::SetDuration(duration) => self.cuts.set_duration(duration),
            AudioTrimBarInput::SetPlayhead(playhead) => self.playhead = playhead,
            AudioTrimBarInput::MarkHere => {
                self.cuts.mark(self.playhead);
            }
            AudioTrimBarInput::DeleteSegment(id) => {
                self.cuts.delete(id);
            }
            AudioTrimBarInput::ClearSegments => self.cuts.clear(),
            AudioTrimBarInput::SetSegmentTimes(id, start, end) => {
                self.cuts.set_cut_times(id, start, end);
            }
            AudioTrimBarInput::RequestExport => {
                let ranges = self.cuts.ranges();
                if !ranges.is_empty() {
                    let _ = sender.output(AudioTrimBarOutput::ExportRequested { ranges });
                }
            }
        }
        self.refresh(&sender);
    }
}

fn build_segment_chip(
    segment: &Cut,
    sender: &ComponentSender<AudioTrimBar>,
    labels: &BigAudioTrimBarLabels,
) -> gtk::Box {
    let chip = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    chip.add_css_class("linked");

    let label = format!(
        "{} - {}",
        time::playback_time(segment.start),
        time::playback_time(segment.end)
    );
    let edit_button = gtk::Button::builder().label(&label).build();
    edit_button.set_tooltip_text(Some(&labels.edit_cut_times));
    edit_button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}: {label}",
        labels.edit_cut
    ))]);
    {
        let sender = sender.clone();
        let segment = *segment;
        edit_button.connect_clicked(move |_| {
            let _ = sender.output(AudioTrimBarOutput::EditSegment {
                id: segment.id,
                start: segment.start,
                end: segment.end,
            });
        });
    }

    let delete_button =
        tooltip::icon_button("window-close-symbolic", &labels.remove_this_cut, &["flat"]);
    {
        let sender = sender.clone();
        let id = segment.id;
        delete_button.connect_clicked(move |_| sender.input(AudioTrimBarInput::DeleteSegment(id)));
    }

    chip.append(&edit_button);
    chip.append(&delete_button);
    chip
}
