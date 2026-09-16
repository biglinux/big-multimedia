//! Shell-side trim glue (`impl VideoShell`): enter/exit trim mode, mark cuts at
//! the live mpv position, paint the cuts on the transport seek bar, and drive the
//! export through the shared conversion queue. Also hosts the two trim dialogs:
//! the manual segment time-edit form and the re-encode settings dialog.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use gtk::glib;
use relm4::{ComponentController, ComponentSender};

use big_media_components::time::playback_time;
use big_media_runtime::converter::ffmpeg::progress::FfmpegProgressSnapshot;
use big_media_runtime::converter::queue::{QueueMessage, SegmentOutputMode};
use big_quick_export::export::{QuickExportChoice, build_single_export_queue};
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::input::inline_spin_set::{BigInlineSpinFieldSpec, BigInlineSpinSet};
use big_relm4_components::input::preference_cards::BigSpinRange;
use big_relm4_components::input::slider_row::{BigSliderRow, BigSliderRowSpec};
use big_relm4_components::layout::advanced_disclosure::advanced_disclosure;
use big_relm4_components::layout::dialog_footer::{BigDialogFooter, BigDialogFooterSpec};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::layout::preferences_dialog::{
    BigPreferencesDialog, BigPreferencesDialogSpec,
};
use big_relm4_components::list::progress_row::BigProgressRowSpec;

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
        *self.trim_filters.borrow_mut() = self.current_video_filters();
        self.trim_active.set(true);
        // Pin the OSD so the transport (scrub/play) stays available while cutting
        // (the OSD shares this same flag) and turn the seek bar into the cut
        // editor.
        self.osd.show();
        // SetEditMode reveals the trim controls row (above the kept transport) and
        // turns the seek bar into the dedicated cut timeline.
        self.controls.emit(VideoControlsBarInput::SetEditMode(true));
        self.trim
            .emit(VideoTrimBarInput::SetDuration(self.trim_duration()));
        // Inset the video so the full frame shows above/below the pinned bars
        // (deferred so the trim row's reveal animation has settled → real heights).
        {
            let main_overlay = self.main_overlay.clone();
            let controls = self.osd.controls_revealer.clone();
            let header = self.osd.header_revealer.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
                apply_video_inset(
                    &main_overlay,
                    header.height().max(0),
                    controls.height().max(0),
                );
            });
        }
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(
                mpv.get_property_f64("time-pos"),
            ));
        }
    }

    /// Leave trim mode and restore the normal OSD auto-hide + seek bar.
    pub(in crate::video::ui::video_shell) fn exit_trim_mode(&mut self) {
        self.trim_active.set(false);
        self.trim_play_selection.set(false);
        self.controls
            .emit(VideoControlsBarInput::SetEditMode(false));
        // Restore edge-to-edge video.
        apply_video_inset(&self.main_overlay, 0, 0);
        self.osd.show_and_schedule_hide();
    }

    /// Apply a cut's new times after the user dragged its edge on the cut
    /// timeline (forwarded from the controls bar).
    pub(in crate::video::ui::video_shell) fn adjust_trim_segment(
        &self,
        id: u64,
        start: f64,
        end: f64,
    ) {
        self.trim
            .emit(VideoTrimBarInput::SetSegmentTimes(id, start, end));
    }

    /// Push the live playhead to the trim bar (called from the position tick
    /// while trim mode is active).
    pub(in crate::video::ui::video_shell) fn feed_trim_playhead(&self, pos: f64) {
        if self.trim_active.get() {
            self.trim.emit(VideoTrimBarInput::SetPlayhead(pos));
        }
    }

    /// "Play only the marked cuts": when the playhead leaves / reaches the end of
    /// a cut, jump to the next cut (or restart at the first). Mirrors the audio
    /// editor's play-selection. Called every position tick while trim is active.
    pub(in crate::video::ui::video_shell) fn apply_play_selection(&self, pos: f64) {
        if !self.trim_play_selection.get() {
            return;
        }
        let ranges = self.trim_ranges.borrow();
        let Some(&(first_start, _)) = ranges.first() else {
            return;
        };
        let mpv_ref = self.mpv.borrow();
        let Some(mpv) = mpv_ref.as_ref() else {
            return;
        };
        const EPS: f64 = 0.05;
        if let Some((idx, &(_, end))) = ranges
            .iter()
            .enumerate()
            .find(|&(_, &(s, e))| pos >= s - EPS && pos <= e + EPS)
        {
            if pos >= end - EPS {
                let next = ranges.get(idx + 1).map_or(first_start, |&(s, _)| s);
                mpv.seek_absolute(next);
            }
        } else if let Some(&(next, _)) = ranges.iter().find(|&&(s, _)| s > pos) {
            mpv.seek_absolute(next);
        } else {
            mpv.seek_absolute(first_start);
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
            VideoTrimBarOutput::SegmentsChanged { segments, pending } => {
                let mut ranges: Vec<(f64, f64)> =
                    segments.iter().map(|(_, s, e)| (*s, *e)).collect();
                ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
                *self.trim_ranges.borrow_mut() = ranges;
                self.controls
                    .emit(VideoControlsBarInput::SetCutSegments(segments));
                self.controls
                    .emit(VideoControlsBarInput::SetPendingMark(pending));
            }
            VideoTrimBarOutput::PlaySelection(on) => {
                self.trim_play_selection.set(on);
                if on && let Some(mpv) = self.mpv.borrow().as_ref() {
                    if let Some((start, _)) = self.trim_ranges.borrow().first().copied() {
                        mpv.seek_absolute(start);
                    }
                    mpv.play();
                }
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

        // Slim strip: start indeterminate, reveal it above the transport. The
        // video keeps playing behind it.
        self.notify(&i18n("Verified completed outputs can be reused when retrying. Existing files without a matching receipt are kept."));
        let title = i18n("Generating cut…");
        self.trim_progress_row
            .update(&BigProgressRowSpec::indeterminate(
                title.clone(),
                String::new(),
            ));
        self.trim_progress_revealer.set_reveal_child(true);
        self.trim.emit(VideoTrimBarInput::SetBusy(true));

        queue.start();
        let receiver = queue.receiver().clone();
        *self.trim_queue.borrow_mut() = Some(queue);

        // Drive the strip directly (local future = main thread; the row holds GTK
        // widgets). No per-file rows / command log — just the live cut progress.
        let row = self.trim_progress_row.clone();
        let shell = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let mut outcome = TrimExportOutcome::Failed(i18n("The export ended without confirming completion"));
            while let Ok(msg) = receiver.recv().await {
                match msg {
                    QueueMessage::Progress {
                        progress_snapshot, ..
                    } => {
                        let (fraction, status) = trim_progress_status(&progress_snapshot);
                        row.update(&BigProgressRowSpec::determinate(
                            title.clone(),
                            fraction,
                            status,
                        ));
                    }
                    QueueMessage::Done { entry_id } if entry_id == id => {
                        outcome = TrimExportOutcome::Ok;
                    }
                    QueueMessage::Failed { entry_id, error } if entry_id == id => {
                        outcome = TrimExportOutcome::Failed(error);
                    }
                    QueueMessage::Cancelled { entry_id } if entry_id == id => {
                        outcome = TrimExportOutcome::Cancelled;
                    }
                    QueueMessage::AllDone => break,
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
        let spin_range = BigSpinRange::new(0.0, duration, 0.1, 5.0);
        let spin_set = BigInlineSpinSet::new(&[
            BigInlineSpinFieldSpec::new("start", i18n("Start (seconds)"), start, spin_range)
                .digits(2)
                .width_chars(7)
                .unit(i18n("s")),
            BigInlineSpinFieldSpec::new("end", i18n("End (seconds)"), end, spin_range)
                .digits(2)
                .width_chars(7)
                .unit(i18n("s")),
        ]);
        let Some(start_spin) = spin_set.spin("start").cloned() else {
            log::warn!("trim edit dialog ignored because the start spin is missing");
            return;
        };
        let Some(end_spin) = spin_set.spin("end").cloned() else {
            log::warn!("trim edit dialog ignored because the end spin is missing");
            return;
        };

        let section = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .build();
        let title = gtk::Label::builder()
            .label(i18n("Cut times"))
            .xalign(0.0)
            .css_classes(["heading"])
            .build();
        let description = gtk::Label::builder()
            .label(i18n("Hundredth-second precision; either order is fine"))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        section.append(&title);
        section.append(&description);
        section.append(spin_set.root());

        let dialog = adw::Dialog::builder()
            .title(i18n("Edit Cut"))
            .content_width(380)
            .content_height(340)
            .build();
        let page = BigDialogPage::scrolled_clamped(360, 320);
        let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Apply")));
        page.content().append(&section);
        page.root().add_bottom_bar(footer.root());
        dialog.set_child(Some(&page.into_root()));

        let dialog_ref = dialog.downgrade();
        footer.cancel_button().connect_clicked(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        });

        let trim = self.trim.sender().clone();
        let dialog_ref = dialog.downgrade();
        footer.accept_button().connect_clicked(glib::clone!(
            #[strong]
            dialog_ref,
            #[weak]
            start_spin,
            #[weak]
            end_spin,
            move |_| {
                let _ = trim.send(VideoTrimBarInput::SetSegmentTimes(
                    id,
                    start_spin.value(),
                    end_spin.value(),
                ));
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        ));
        dialog.present(Some(root));
    }

    /// Re-encode settings dialog (the gear): container/codec/quality (shared with
    /// the converter) + colour adjustments (editor sliders) + output layout.
    /// Built once and reused (the shared `EncodeOptions` group stays parented).
    fn show_encode_config_dialog(&self, root: &adw::ApplicationWindow) {
        if let Some(window) = self.trim_encode_dialog.borrow().as_ref() {
            window.present();
            return;
        }

        let dialog = BigPreferencesDialog::new(
            &BigPreferencesDialogSpec::new(i18n("Clip export settings")).size(560, 656),
        );
        dialog.action_bar().set_visible(false);
        dialog.add_group(self.trim_encode.group());

        // Colour adjustments (imported editor slider component) → VideoFilters.
        let adjust = BigPreferencesGroupSpec::untitled().build();
        let adjust_row = advanced_disclosure(
            &i18n("Image adjustments"),
            &i18n("Brightness, contrast, color intensity, and color balance"),
            false,
        );
        let current_filters = self.trim_filters.borrow().clone();
        let (brightness_row, brightness_scale, brightness_reset) =
            BigSliderRow::new(BigSliderRowSpec::new(
                i18n("Brightness"),
                -1.0,
                1.0,
                current_filters.brightness,
                0.05,
                i18n("Reset"),
            ))
            .into_parts();
        let (contrast_row, contrast_scale, contrast_reset) =
            BigSliderRow::new(BigSliderRowSpec::new(
                i18n("Contrast"),
                0.0,
                2.0,
                current_filters.contrast,
                0.05,
                i18n("Reset"),
            ))
            .into_parts();
        let (saturation_row, saturation_scale, saturation_reset) =
            BigSliderRow::new(BigSliderRowSpec::new(
                i18n("Color intensity"),
                0.0,
                2.0,
                current_filters.saturation,
                0.05,
                i18n("Reset"),
            ))
            .into_parts();
        let (hue_row, hue_scale, hue_reset) = BigSliderRow::new(BigSliderRowSpec::new(
            i18n("Color balance"),
            -1.0,
            1.0,
            current_filters.hue,
            0.05,
            i18n("Reset"),
        ))
        .into_parts();
        adjust_row.add_row(&brightness_row);
        adjust_row.add_row(&contrast_row);
        adjust_row.add_row(&saturation_row);
        adjust_row.add_row(&hue_row);
        adjust.add(&adjust_row);
        dialog.add_group(&adjust);

        let filters = self.trim_filters.clone();
        brightness_scale.connect_value_changed(glib::clone!(
            #[strong]
            filters,
            move |s| filters.borrow_mut().brightness = s.value()
        ));
        contrast_scale.connect_value_changed(glib::clone!(
            #[strong]
            filters,
            move |s| filters.borrow_mut().contrast = s.value()
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
        brightness_reset.connect_clicked(glib::clone!(
            #[weak]
            brightness_scale,
            move |_| brightness_scale.set_value(0.0)
        ));
        contrast_reset.connect_clicked(glib::clone!(
            #[weak]
            contrast_scale,
            move |_| contrast_scale.set_value(1.0)
        ));
        saturation_reset.connect_clicked(glib::clone!(
            #[weak]
            saturation_scale,
            move |_| saturation_scale.set_value(1.0)
        ));
        hue_reset.connect_clicked(glib::clone!(
            #[weak]
            hue_scale,
            move |_| hue_scale.set_value(0.0)
        ));

        // Output layout: join the cuts into one file or save each separately.
        let output = BigPreferencesGroupSpec::new(i18n("Output files")).build();
        let output_row = BigDropdownRow::new(
            BigDropdownRowSpec::new(
                i18n("When there are multiple cuts"),
                [i18n("Join into one file"), i18n("Save each cut separately")],
            )
            .subtitle(i18n(
                "One file is easier to share; separate files are easier to edit later",
            ))
            .selected(u32::from(self.trim_split.get())),
        );
        let split = self.trim_split.clone();
        output_row
            .dropdown()
            .connect_selected_notify(move |dropdown| split.set(dropdown.selected() == 1));
        output.add(output_row.root());
        dialog.add_group(&output);

        let window = dialog.window().clone();
        // Hide (not destroy) on close so the shared group stays parented + the
        // settings persist for the next open.
        window.connect_close_request(|window| {
            window.set_visible(false);
            glib::Propagation::Stop
        });
        *self.trim_encode_dialog.borrow_mut() = Some(window.clone());
        dialog.present(root);
    }

    /// Finish an export: hide the strip, re-enable the bar, drop the queue,
    /// toast the outcome.
    pub(in crate::video::ui::video_shell) fn finish_trim_export(
        &mut self,
        outcome: TrimExportOutcome,
    ) {
        self.trim_progress_revealer.set_reveal_child(false);
        self.trim.emit(VideoTrimBarInput::SetBusy(false));
        *self.trim_queue.borrow_mut() = None;
        match outcome {
            TrimExportOutcome::Ok => self.notify(&i18n("Clip exported")),
            TrimExportOutcome::Cancelled => self.notify(&i18n("Export cancelled")),
            TrimExportOutcome::Failed(error) => {
                self.notify(&format!("{}: {error}", i18n("Export failed")));
            }
        }
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

/// Build the slim strip's `(fraction, status)` from a queue progress sample:
/// `42% · 1.8× · ETA 0:12`. Speed/ETA are dropped when unknown (e.g. the
/// near-instant stream-copy of the fast path).
fn trim_progress_status(progress: &FfmpegProgressSnapshot) -> (f64, String) {
    let fraction = (progress.percent / 100.0).clamp(0.0, 1.0);
    let mut parts = vec![format!("{:.0}%", progress.percent)];
    if progress.speed > 0.0 {
        parts.push(format!("{:.1}×", progress.speed));
    }
    if progress.eta_secs > 0.0 {
        parts.push(format!("ETA {}", playback_time(progress.eta_secs)));
    }
    (fraction, parts.join(" · "))
}

/// Inset the video layers (GraphicsOffload / GLArea / software Picture) of
/// `overlay` by `top`/`bottom` margins so the full frame shows clear of the
/// pinned trim bars; `(0, 0)` restores edge-to-edge. The OSD bars + welcome /
/// buffering boxes are left untouched.
fn apply_video_inset(overlay: &gtk::Overlay, top: i32, bottom: i32) {
    let mut child = overlay.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if widget.is::<gtk::GLArea>()
            || widget.is::<gtk::Picture>()
            || widget.is::<gtk::GraphicsOffload>()
        {
            widget.set_margin_top(top);
            widget.set_margin_bottom(bottom);
        }
        child = next;
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
