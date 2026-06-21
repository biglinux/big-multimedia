//! Shell-side trim glue (`impl VideoShell`): enter/exit trim mode, mark cuts at
//! the live mpv position, paint the cuts on the transport seek bar, and drive the
//! export through the shared conversion queue. Also hosts the two trim dialogs:
//! the manual segment time-edit form and the re-encode settings dialog.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::glib;
use relm4::{ComponentController, ComponentSender};

use big_converter_ui::export::{build_single_export_queue, ui_progress_info, QuickExportChoice};
use big_media_components::converter::progress::{
    BigConverterProgressInput as ProgIn, BigConverterProgressOutput as ProgOut,
};
use big_media_runtime::converter::queue::{QueueMessage, SegmentOutputMode};
use big_relm4_components::input::slider_row::{BigSliderRow, BigSliderRowSpec};

use crate::i18n::i18n;
use crate::video::ui::video_controls_bar::VideoControlsBarInput;

use super::super::{VideoShell, VideoShellInput};
use super::bar::{VideoTrimBarInput, VideoTrimBarOutput};
use super::state::CutMode;

impl VideoShell {
    /// Enter the interactive trim mode (no-op without a local file playing).
    pub(in crate::video::ui::video_shell) fn enter_trim_mode(&mut self) {
        if self.current_local_input().is_none() {
            self.notify(&i18n("Open a local video to cut it"));
            return;
        }
        self.trim_active.set(true);
        // Pin the OSD so the transport (scrub/play) stays available while cutting
        // (the OSD shares this same flag) and turn the seek bar into the cut
        // editor.
        self.osd.show();
        self.controls.emit(VideoControlsBarInput::SetEditMode(true));
        self.trim_revealer.set_reveal_child(true);
        self.trim
            .emit(VideoTrimBarInput::SetDuration(self.trim_duration()));
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(
                mpv.get_property_f64("time-pos"),
            ));
        }
    }

    /// Leave trim mode and restore the normal OSD auto-hide + seek bar.
    pub(in crate::video::ui::video_shell) fn exit_trim_mode(&mut self) {
        self.trim_active.set(false);
        self.trim_revealer.set_reveal_child(false);
        self.controls
            .emit(VideoControlsBarInput::SetEditMode(false));
        self.osd.show_and_schedule_hide();
    }

    /// Push the live playhead to the trim bar (called from the position tick
    /// while trim mode is active).
    pub(in crate::video::ui::video_shell) fn feed_trim_playhead(&self, pos: f64) {
        if self.trim_active.get() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(pos));
        }
    }

    /// Push a new duration to the trim bar (called on `DurationChanged`).
    pub(in crate::video::ui::video_shell) fn feed_trim_duration(&self, dur: f64) {
        if self.trim_active.get() {
            self.trim.emit(VideoTrimBarInput::SetDuration(dur));
        }
    }

    /// Keyboard mark (`M`): read mpv's exact `time-pos` and mark there. Two marks
    /// (in any order) make a segment.
    pub(in crate::video::ui::video_shell) fn trim_mark(&self) {
        if !self.trim_active.get() {
            return;
        }
        let t = self
            .mpv
            .borrow()
            .as_ref()
            .map_or(0.0, |mpv| mpv.get_property_f64("time-pos"));
        self.trim.emit(VideoTrimBarInput::Mark(t));
    }

    /// Act on a typed effect from the trim bar.
    pub(in crate::video::ui::video_shell) fn on_trim_bar(
        &mut self,
        out: VideoTrimBarOutput,
        root: Option<&adw::ApplicationWindow>,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            VideoTrimBarOutput::SegmentsChanged(ranges) => {
                self.controls
                    .emit(VideoControlsBarInput::SetCutSegments(ranges));
            }
            VideoTrimBarOutput::EditSegment { id, start, end } => {
                if let Some(root) = root {
                    self.show_segment_edit_dialog(id, start, end, root);
                }
            }
            VideoTrimBarOutput::OpenEncodeConfig => {
                if let Some(root) = root {
                    self.show_encode_config_dialog(root);
                }
            }
            VideoTrimBarOutput::Exit => self.exit_trim_mode(),
            VideoTrimBarOutput::ExportRequested { ranges, mode } => {
                self.start_trim_export(&ranges, mode, sender);
            }
        }
    }

    /// Build + start the clip export, show the progress page, and stream queue
    /// messages into it until completion.
    fn start_trim_export(
        &mut self,
        ranges: &[(f64, f64)],
        mode: CutMode,
        sender: &ComponentSender<Self>,
    ) {
        let Some(input) = self.current_local_input() else {
            self.notify(&i18n("Open a local video to cut it"));
            return;
        };
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        let choice = match mode {
            CutMode::Fast => QuickExportChoice::keep_format(&input),
            // Precise: re-encode with the SAME container/codec/quality options the
            // full converter offers (shared `EncodeOptions`) plus the editor
            // colour adjustments from the settings dialog.
            CutMode::Precise => self
                .trim_encode
                .choice()
                .to_quick_choice(self.trim_filters.borrow().clone()),
        };

        let (mut queue, id, output) =
            build_single_export_queue(input.clone(), output_dir, &choice, ranges);
        if output.as_os_str().is_empty() {
            self.notify(&i18n("Export failed"));
            return;
        }
        if self.trim_split.get() {
            queue.set_pending_segment_mode(SegmentOutputMode::Split);
        }

        // Progress page: one row for this file.
        self.trim_progress.emit(ProgIn::Reset);
        self.trim_progress
            .emit(ProgIn::SetItems(vec![(id, input.clone())]));
        self.trim_progress.emit(ProgIn::SetCurrentFile(
            input
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string(),
        ));
        self.trim_progress_overlay.set_visible(true);
        self.trim.emit(VideoTrimBarInput::SetBusy(true));

        queue.start();
        let receiver = queue.receiver().clone();
        *self.trim_queue.borrow_mut() = Some(queue);

        let progress = self.trim_progress.sender().clone();
        let shell = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let mut outcome: TrimExportOutcome = TrimExportOutcome::Ok;
            while let Ok(msg) = receiver.recv().await {
                match msg {
                    QueueMessage::Started { item_id, command } => {
                        let _ = progress.send(ProgIn::SetCommand(item_id, command));
                    }
                    QueueMessage::Progress { item_id, info } => {
                        let _ =
                            progress.send(ProgIn::UpdateProgress(item_id, ui_progress_info(&info)));
                    }
                    QueueMessage::StderrLine { item_id, line } => {
                        let _ = progress.send(ProgIn::AppendStderrLine(item_id, line));
                    }
                    QueueMessage::Done { item_id } => {
                        let _ = progress.send(ProgIn::MarkDone(item_id));
                    }
                    QueueMessage::Failed { item_id, error } => {
                        let _ = progress.send(ProgIn::MarkFailed(item_id, error.clone()));
                        outcome = TrimExportOutcome::Failed(error);
                    }
                    QueueMessage::Cancelled { item_id } => {
                        let _ = progress.send(ProgIn::MarkCancelled(item_id));
                        outcome = TrimExportOutcome::Cancelled;
                    }
                    QueueMessage::AllDone => {
                        let _ = progress.send(ProgIn::ShowComplete);
                        break;
                    }
                    _ => {}
                }
            }
            let _ = shell.send(VideoShellInput::TrimExportFinished(outcome));
        });
    }

    /// Manual time-edit form for one cut: two hundredth-second spin rows seeded
    /// with the segment's current bounds; Apply pushes the new times back.
    fn show_segment_edit_dialog(
        &self,
        id: u64,
        start: f64,
        end: f64,
        root: &adw::ApplicationWindow,
    ) {
        let duration = self.trim_duration();
        let start_row = adw::SpinRow::builder()
            .title(i18n("Start (seconds)"))
            .digits(2)
            .adjustment(&gtk::Adjustment::new(start, 0.0, duration, 0.1, 5.0, 0.0))
            .build();
        let end_row = adw::SpinRow::builder()
            .title(i18n("End (seconds)"))
            .digits(2)
            .adjustment(&gtk::Adjustment::new(end, 0.0, duration, 0.1, 5.0, 0.0))
            .build();
        let group = adw::PreferencesGroup::builder()
            .title(i18n("Cut times"))
            .description(i18n("Hundredth-second precision; either order is fine"))
            .build();
        group.add(&start_row);
        group.add(&end_row);

        let apply = gtk::Button::builder()
            .label(i18n("Apply"))
            .css_classes(["suggested-action", "pill"])
            .halign(gtk::Align::Center)
            .margin_top(12)
            .build();

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        page.append(&group);
        page.append(&apply);

        let header = adw::HeaderBar::new();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&page));
        let window = adw::Window::builder()
            .title(i18n("Edit Cut"))
            .modal(true)
            .transient_for(root)
            .default_width(380)
            .content(&toolbar)
            .build();

        let trim = self.trim.sender().clone();
        apply.connect_clicked(glib::clone!(
            #[weak]
            window,
            #[weak]
            start_row,
            #[weak]
            end_row,
            move |_| {
                let _ = trim.send(VideoTrimBarInput::SetSegmentTimes(
                    id,
                    start_row.value(),
                    end_row.value(),
                ));
                window.close();
            }
        ));
        window.present();
    }

    /// Re-encode settings dialog (the gear): container/codec/quality (shared with
    /// the converter) + colour adjustments (editor sliders) + output layout.
    /// Built once and reused (the shared `EncodeOptions` group stays parented).
    fn show_encode_config_dialog(&self, root: &adw::ApplicationWindow) {
        if let Some(window) = self.trim_encode_dialog.borrow().as_ref() {
            window.present();
            return;
        }

        let page = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        page.append(self.trim_encode.group());

        // Colour adjustments (imported editor slider component) → VideoFilters.
        let adjust = adw::PreferencesGroup::builder()
            .title(i18n("Adjustments"))
            .build();
        let (brightness_row, brightness_scale, _) = BigSliderRow::new(BigSliderRowSpec::new(
            i18n("Brightness"),
            -1.0,
            1.0,
            0.0,
            0.05,
            i18n("Reset"),
        ))
        .into_parts();
        let (saturation_row, saturation_scale, _) = BigSliderRow::new(BigSliderRowSpec::new(
            i18n("Saturation"),
            0.0,
            2.0,
            1.0,
            0.05,
            i18n("Reset"),
        ))
        .into_parts();
        let (hue_row, hue_scale, _) = BigSliderRow::new(BigSliderRowSpec::new(
            i18n("Hue"),
            -1.0,
            1.0,
            0.0,
            0.05,
            i18n("Reset"),
        ))
        .into_parts();
        adjust.add(&brightness_row);
        adjust.add(&saturation_row);
        adjust.add(&hue_row);
        page.append(&adjust);

        let filters = self.trim_filters.clone();
        brightness_scale.connect_value_changed(glib::clone!(
            #[strong]
            filters,
            move |s| filters.borrow_mut().brightness = s.value()
        ));
        saturation_scale.connect_value_changed(glib::clone!(
            #[strong]
            filters,
            move |s| filters.borrow_mut().saturation = s.value()
        ));
        hue_scale.connect_value_changed(glib::clone!(
            #[strong]
            filters,
            move |s| filters.borrow_mut().hue = s.value()
        ));

        // Output layout: join the cuts into one file or save each separately.
        let output = adw::PreferencesGroup::builder()
            .title(i18n("Output"))
            .build();
        let output_row = adw::ComboRow::builder()
            .title(i18n("When there are multiple cuts"))
            .model(&gtk::StringList::new(&[
                &i18n("Join into one file"),
                &i18n("Save each cut separately"),
            ]))
            .build();
        let split = self.trim_split.clone();
        output_row.connect_selected_notify(move |row| split.set(row.selected() == 1));
        output.add(&output_row);
        page.append(&output);

        let header = adw::HeaderBar::new();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&page)
                .build(),
        ));
        let window = adw::Window::builder()
            .title(i18n("Re-encode settings"))
            .modal(true)
            .transient_for(root)
            .default_width(420)
            .default_height(520)
            .content(&toolbar)
            .build();
        // Hide (not destroy) on close so the shared group stays parented + the
        // settings persist for the next open.
        window.connect_close_request(|window| {
            window.set_visible(false);
            glib::Propagation::Stop
        });
        *self.trim_encode_dialog.borrow_mut() = Some(window.clone());
        window.present();
    }

    /// React to the progress page's own buttons (Back dismisses; Cancel aborts).
    pub(in crate::video::ui::video_shell) fn on_trim_progress(&mut self, out: ProgOut) {
        match out {
            ProgOut::Back => self.trim_progress_overlay.set_visible(false),
            ProgOut::CancelAll | ProgOut::CancelItem(_) => {
                if let Some(queue) = self.trim_queue.borrow().as_ref() {
                    queue.cancel();
                }
            }
        }
    }

    /// Finish an export: re-enable the bar, drop the queue, toast the outcome.
    pub(in crate::video::ui::video_shell) fn finish_trim_export(
        &mut self,
        outcome: TrimExportOutcome,
    ) {
        self.trim.emit(VideoTrimBarInput::SetBusy(false));
        *self.trim_queue.borrow_mut() = None;
        match outcome {
            TrimExportOutcome::Ok => self.notify(&i18n("Clip exported")),
            TrimExportOutcome::Cancelled => self.notify(&i18n("Export cancelled")),
            TrimExportOutcome::Failed(error) => {
                self.notify(&format!("{}: {error}", i18n("Export failed")));
            }
        }
        // The progress page keeps its completion banner up; the user dismisses it
        // with its Back button (→ on_trim_progress hides the overlay).
    }

    /// Cancel any in-flight export (used on teardown so a closing window does not
    /// leave an ffmpeg worker running).
    pub(in crate::video::ui::video_shell) fn cancel_trim_export(&self) {
        if let Some(queue) = self.trim_queue.borrow().as_ref() {
            queue.cancel();
        }
    }

    /// The current local (non-stream) input path, or `None`.
    fn current_local_input(&self) -> Option<PathBuf> {
        let path = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())?;
        if super::super::playback::is_remote_uri(&path) {
            return None;
        }
        Some(PathBuf::from(path))
    }

    /// Best-known duration (mpv `duration` falls back when the cached value is
    /// not yet populated).
    fn trim_duration(&self) -> f64 {
        if self.duration_secs > 1.0 {
            self.duration_secs
        } else {
            self.mpv
                .borrow()
                .as_ref()
                .map_or(0.0, |mpv| mpv.get_property_f64("duration"))
                .max(1.0)
        }
    }
}

/// Terminal state of a clip export, carried back to the shell. Public because it
/// travels in the public [`VideoShellInput`] enum.
#[derive(Debug)]
pub enum TrimExportOutcome {
    /// Completed successfully.
    Ok,
    /// The user cancelled.
    Cancelled,
    /// FFmpeg / queue failed with this message.
    Failed(String),
}
