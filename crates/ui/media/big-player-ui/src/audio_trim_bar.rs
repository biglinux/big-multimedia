//! Shared Relm4 audio trim controls for the player transport.
//!
//! The component owns only trim UI state: mark pairs, selected cut ranges,
//! and small edit/delete chips. Playback, seeking, mpv, and export stay in the
//! host shell.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use big_media_components::time;
use big_relm4_components::feedback::tooltip;

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

#[derive(Clone, Debug, PartialEq)]
struct AudioTrimSegment {
    id: u64,
    start: f64,
    end: f64,
}

#[derive(Debug)]
struct AudioTrimState {
    duration: f64,
    playhead: f64,
    pending_mark: Option<f64>,
    segments: Vec<AudioTrimSegment>,
    next_id: u64,
}

impl Default for AudioTrimState {
    fn default() -> Self {
        Self {
            duration: 0.0,
            playhead: 0.0,
            pending_mark: None,
            segments: Vec::new(),
            next_id: 1,
        }
    }
}

impl AudioTrimState {
    fn mark(&mut self, time: f64) {
        let time = self.clamp_time(time);
        if let Some(start) = self.pending_mark.take() {
            let (start, end) = ordered_pair(start, time);
            if end - start >= 0.1 {
                self.push_segment(start, end);
            }
        } else {
            self.pending_mark = Some(time);
        }
    }

    fn push_segment(&mut self, start: f64, end: f64) {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.segments.push(AudioTrimSegment { id, start, end });
    }

    fn set_segment_times(&mut self, id: u64, start: f64, end: f64) -> bool {
        let (start, end) = ordered_pair(self.clamp_time(start), self.clamp_time(end));
        if end - start < 0.1 {
            return false;
        }
        let Some(segment) = self.segments.iter_mut().find(|segment| segment.id == id) else {
            return false;
        };
        segment.start = start;
        segment.end = end;
        true
    }

    fn delete_segment(&mut self, id: u64) {
        self.segments.retain(|segment| segment.id != id);
    }

    fn clear(&mut self) {
        self.pending_mark = None;
        self.segments.clear();
    }

    fn export_ranges(&self) -> Vec<(f64, f64)> {
        let mut ranges: Vec<(f64, f64)> = self
            .segments
            .iter()
            .map(|segment| (segment.start, segment.end))
            .collect();
        ranges.sort_by(|a, b| a.0.total_cmp(&b.0));

        let mut merged: Vec<(f64, f64)> = Vec::with_capacity(ranges.len());
        for (start, end) in ranges {
            if let Some((_, last_end)) = merged.last_mut()
                && start <= *last_end
            {
                *last_end = (*last_end).max(end);
                continue;
            }
            merged.push((start, end));
        }
        merged
    }

    fn is_exportable(&self) -> bool {
        !self.segments.is_empty()
    }

    fn clamp_time(&self, time: f64) -> f64 {
        time.clamp(0.0, self.duration.max(0.0))
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
        /// `(id, start, end)` segment tuples.
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
    state: Rc<RefCell<AudioTrimState>>,
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

        let (segments, has_segments, count, pending, exportable) = {
            let state = self.state.borrow();
            (
                state.segments.clone(),
                !state.segments.is_empty(),
                state.segments.len(),
                state.pending_mark,
                state.is_exportable(),
            )
        };

        for segment in &segments {
            self.segments_strip
                .append(&build_segment_chip(segment, sender, &self.labels));
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
        self.export_button.set_sensitive(exportable);

        let _ = sender.output(AudioTrimBarOutput::SegmentsChanged {
            segments: segments
                .iter()
                .map(|segment| (segment.id, segment.start, segment.end))
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
        let state = Rc::new(RefCell::new(AudioTrimState::default()));

        let mark_button = gtk::Button::builder()
            .label(&labels.mark_cut_point)
            .css_classes(["suggested-action"])
            .valign(gtk::Align::Center)
            .build();
        tooltip::set(&mark_button, &labels.mark_cut_point);
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
        tooltip::set(&export_button, &labels.export_cuts_tooltip);
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
            state,
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
            AudioTrimBarInput::SetDuration(duration) => {
                self.state.borrow_mut().duration = duration.max(0.0);
            }
            AudioTrimBarInput::SetPlayhead(playhead) => {
                let mut state = self.state.borrow_mut();
                state.playhead = state.clamp_time(playhead);
            }
            AudioTrimBarInput::MarkHere => {
                let playhead = self.state.borrow().playhead;
                self.state.borrow_mut().mark(playhead);
            }
            AudioTrimBarInput::DeleteSegment(id) => self.state.borrow_mut().delete_segment(id),
            AudioTrimBarInput::ClearSegments => self.state.borrow_mut().clear(),
            AudioTrimBarInput::SetSegmentTimes(id, start, end) => {
                self.state.borrow_mut().set_segment_times(id, start, end);
            }
            AudioTrimBarInput::RequestExport => {
                let ranges = self.state.borrow().export_ranges();
                if !ranges.is_empty() {
                    let _ = sender.output(AudioTrimBarOutput::ExportRequested { ranges });
                }
            }
        }
        self.refresh(&sender);
    }
}

fn build_segment_chip(
    segment: &AudioTrimSegment,
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
    tooltip::set(&edit_button, &labels.edit_cut_times);
    edit_button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}: {label}",
        labels.edit_cut
    ))]);
    {
        let sender = sender.clone();
        let segment = segment.clone();
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

fn ordered_pair(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

#[cfg(test)]
mod tests {
    use super::AudioTrimState;

    fn mark(state: &mut AudioTrimState, start: f64, end: f64) {
        state.mark(start);
        state.mark(end);
    }

    #[test]
    fn two_marks_create_ordered_segment() {
        let mut state = AudioTrimState {
            duration: 60.0,
            ..AudioTrimState::default()
        };
        mark(&mut state, 20.0, 5.0);
        assert_eq!(state.segments.len(), 1);
        assert_eq!(state.segments[0].start, 5.0);
        assert_eq!(state.segments[0].end, 20.0);
    }

    #[test]
    fn pending_mark_waits_for_second_mark() {
        let mut state = AudioTrimState {
            duration: 60.0,
            ..AudioTrimState::default()
        };
        state.mark(10.0);
        assert_eq!(state.pending_mark, Some(10.0));
        assert!(state.segments.is_empty());
    }

    #[test]
    fn clear_removes_segments_and_pending_mark() {
        let mut state = AudioTrimState {
            duration: 60.0,
            ..AudioTrimState::default()
        };
        state.mark(5.0);
        state.mark(12.0);
        state.mark(20.0);
        state.clear();
        assert!(state.segments.is_empty());
        assert_eq!(state.pending_mark, None);
    }

    #[test]
    fn export_ranges_merge_overlaps() {
        let mut state = AudioTrimState {
            duration: 60.0,
            ..AudioTrimState::default()
        };
        mark(&mut state, 5.0, 10.0);
        mark(&mut state, 8.0, 12.0);
        mark(&mut state, 20.0, 30.0);
        assert_eq!(state.export_ranges(), vec![(5.0, 12.0), (20.0, 30.0)]);
    }

    #[test]
    fn set_segment_times_clamps_and_rejects_empty() {
        let mut state = AudioTrimState {
            duration: 30.0,
            ..AudioTrimState::default()
        };
        mark(&mut state, 5.0, 20.0);
        let id = state.segments[0].id;
        assert!(state.set_segment_times(id, 40.0, -10.0));
        assert_eq!(state.segments[0].start, 0.0);
        assert_eq!(state.segments[0].end, 30.0);
        assert!(!state.set_segment_times(id, 4.0, 4.0));
    }
}
