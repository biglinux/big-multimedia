//! In-player quick export for the audio player: convert or trim-and-export the
//! playing track via the shared converter engine (`big_quick_export::export`),
//! off the GTK main loop. No external converter process.

use std::path::{Path, PathBuf};

use big_media_runtime::converter::profiles::VideoFilters;
use big_media_runtime::converter::queue::{QueueMessage, SegmentOutputMode};
use big_quick_export::export::{
    QuickExportChoice, QuickExportFormat, build_single_export_queue, run_single_export_blocking,
};
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::input::preference_cards::BigSpinRange;
use big_relm4_components::input::spin_row::{BigSpinRow, BigSpinRowSpec};
use big_relm4_components::layout::dialog_footer::{BigDialogFooter, BigDialogFooterSpec};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use super::*;

/// `(label, codec)` for the audio output chooser; container is derived from the
/// codec by the export engine.
const AUDIO_FORMATS: [(&str, &str); 3] = [("MP3", "mp3"), ("M4A", "aac"), ("FLAC", "flac")];
const AUDIO_SEGMENT_OUTPUTS: [(&str, bool); 2] = [
    ("Join cuts into one file", false),
    ("Save each cut separately", true),
];

impl AudioShell {
    /// Convert the playing track to `codec` (mp3/aac/flac) beside it.
    pub(super) fn convert_track(&self, codec: &str) {
        let choice = QuickExportChoice {
            format: QuickExportFormat::Audio {
                codec: codec.to_string(),
                bitrate: "192k".into(),
            },
            filters: VideoFilters::default(),
        };
        self.run_track_export(choice, Vec::new(), &i18n("Converting…"));
    }

    /// Enter visual trim mode on the current local track.
    pub(super) fn enter_trim_mode(&mut self) {
        if self.current_local_input().is_none() {
            self.notify(&i18n("No local track is playing"));
            return;
        }
        self.trim_active = true;
        self.trim_play_selection = false;
        let duration = self.duration_secs.max(
            self.mpv
                .as_ref()
                .map_or(0.0, |mpv| mpv.get_property_f64("duration")),
        );
        let playhead = self
            .mpv
            .as_ref()
            .map_or(0.0, |mpv| mpv.get_property_f64("time-pos"));
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetDurationSecs(duration));
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetEditMode(true));
        self.trim.emit(AudioTrimBarInput::SetDuration(duration));
        self.trim.emit(AudioTrimBarInput::SetPlayhead(playhead));
        self.notify(&i18n("Trim mode enabled"));
    }

    /// Leave visual trim mode and restore the normal seek bar.
    pub(super) fn exit_trim_mode(&mut self) {
        self.trim_active = false;
        self.trim_play_selection = false;
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetEditMode(false));
    }

    /// Handle a typed output from the visual trim bar.
    pub(super) fn handle_trim(
        &mut self,
        output: AudioTrimBarOutput,
        sender: &ComponentSender<Self>,
    ) {
        match output {
            AudioTrimBarOutput::SegmentsChanged { segments, pending } => {
                let mut ranges: Vec<(f64, f64)> = segments
                    .iter()
                    .map(|(_, start, end)| (*start, *end))
                    .collect();
                ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
                self.trim_ranges = ranges;
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetCutSegments(segments));
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetPendingMark(pending));
            }
            AudioTrimBarOutput::EditSegment { id, start, end } => {
                self.show_segment_edit_dialog(id, start, end);
            }
            AudioTrimBarOutput::PlaySelection(active) => {
                self.trim_play_selection = active;
                if active
                    && let (Some((start, _)), Some(mpv)) =
                        (self.trim_ranges.first().copied(), self.mpv.as_ref())
                {
                    mpv.seek_absolute(start);
                }
            }
            AudioTrimBarOutput::ExportRequested { ranges } => {
                self.show_trim_export_options(ranges, sender);
            }
            AudioTrimBarOutput::Exit => self.exit_trim_mode(),
        }
    }

    /// Apply a segment drag emitted by the shared playback timeline.
    pub(super) fn adjust_trim_segment(&self, id: u64, start: f64, end: f64) {
        self.trim
            .emit(AudioTrimBarInput::SetSegmentTimes(id, start, end));
    }

    /// Feed live playhead updates to the trim bar and apply play-selection mode.
    pub(super) fn feed_trim_playhead(&self, position: f64) {
        if !self.trim_active {
            return;
        }
        self.trim.emit(AudioTrimBarInput::SetPlayhead(position));
        if self.trim_play_selection {
            self.apply_trim_play_selection(position);
        }
    }

    /// Feed duration changes to the trim bar and cut timeline.
    pub(super) fn feed_trim_duration(&self, duration: f64) {
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetDurationSecs(duration));
        if self.trim_active {
            self.trim.emit(AudioTrimBarInput::SetDuration(duration));
        }
    }

    /// Re-enable the trim controls after a background export completes.
    pub(super) fn finish_trim_export(&self, result: Result<PathBuf, String>) {
        self.trim.emit(AudioTrimBarInput::SetBusy(false));
        self.toast_overlay
            .add_toast(adw::Toast::new(&export_message(&result)));
    }

    fn show_segment_edit_dialog(&self, id: u64, start: f64, end: f64) {
        let duration = self.duration_secs.max(1.0);
        let range = BigSpinRange::new(0.0, duration, 1.0, 10.0);
        let start_row = BigSpinRow::new(
            BigSpinRowSpec::new(i18n("Start (seconds)"), start, range)
                .digits(2)
                .width_chars(7),
        );
        let end_row = BigSpinRow::new(
            BigSpinRowSpec::new(i18n("End (seconds)"), end, range)
                .digits(2)
                .width_chars(7),
        );
        start_row
            .row()
            .add_suffix(&playhead_button(&self.mpv, start_row.spin()));
        end_row
            .row()
            .add_suffix(&playhead_button(&self.mpv, end_row.spin()));
        let start_spin = start_row.spin().clone();
        let end_spin = end_row.spin().clone();

        let group = BigPreferencesGroupSpec::new(i18n("Cut range"))
            .description(i18n(
                "Use the clock buttons to copy the current playback position",
            ))
            .build();
        group.add(start_row.row());
        group.add(end_row.row());

        let error_label = error_label(&i18n("End must be after start"));
        let dialog = adw::Dialog::builder()
            .title(i18n("Edit cut"))
            .content_width(440)
            .content_height(360)
            .build();
        let page = BigDialogPage::scrolled_clamped(420, 320);
        let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Apply")));
        page.content().set_spacing(12);
        page.content().append(&group);
        page.content().append(&error_label);
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
            #[weak]
            start_spin,
            #[weak]
            end_spin,
            #[weak]
            error_label,
            move |_| {
                let start = start_spin.value();
                let end = end_spin.value();
                if end <= start {
                    error_label.set_visible(true);
                    end_spin.grab_focus();
                    return;
                }
                let _ = trim.send(AudioTrimBarInput::SetSegmentTimes(id, start, end));
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        ));

        dialog.present(self.window().as_ref());
    }

    fn show_trim_export_options(&self, ranges: Vec<(f64, f64)>, sender: &ComponentSender<Self>) {
        let Some(input) = self.current_local_input() else {
            self.notify(&i18n("No local track is playing"));
            return;
        };
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let format_row = BigDropdownRow::new(BigDropdownRowSpec::new(
            i18n("Format"),
            AUDIO_FORMATS.iter().map(|(label, _)| *label),
        ));
        let output_row = BigDropdownRow::new(BigDropdownRowSpec::new(
            i18n("Output"),
            AUDIO_SEGMENT_OUTPUTS.iter().map(|(label, _)| i18n(label)),
        ));
        output_row.root().set_subtitle(&i18n(
            "Join creates one file; separate creates one file per cut",
        ));

        let group = BigPreferencesGroupSpec::new(i18n("Export marked cuts"))
            .description(format!("{} {}", ranges.len(), i18n("selected cut ranges")))
            .build();
        group.add(format_row.root());
        group.add(output_row.root());
        let resume_note = gtk::Label::new(Some(&i18n(
            "Retrying this export reuses only complete files whose source, settings and contents match their saved receipts. Other existing files are never replaced. Keep the receipts beside the outputs.",
        )));
        resume_note.set_wrap(true);
        resume_note.set_xalign(0.0);
        group.add(&resume_note);

        let dialog = adw::Dialog::builder()
            .title(i18n("Export Cuts"))
            .content_width(460)
            .content_height(360)
            .build();
        let page = BigDialogPage::scrolled_clamped(430, 320);
        let footer =
            BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Export")));
        page.content().append(&group);
        page.root().add_bottom_bar(footer.root());
        dialog.set_child(Some(&page.into_root()));

        let dialog_ref = dialog.downgrade();
        footer.cancel_button().connect_clicked(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        });

        let format_dropdown = format_row.dropdown().clone();
        let output_dropdown = output_row.dropdown().clone();
        let input_sender = sender.input_sender().clone();
        let trim = self.trim.sender().clone();
        let dialog_ref = dialog.downgrade();
        footer.accept_button().connect_clicked(glib::clone!(
            #[weak]
            format_dropdown,
            #[weak]
            output_dropdown,
            move |_| {
                let codec = AUDIO_FORMATS
                    .get(format_dropdown.selected() as usize)
                    .map_or("mp3", |(_, codec)| *codec);
                let split = AUDIO_SEGMENT_OUTPUTS
                    .get(output_dropdown.selected() as usize)
                    .is_some_and(|(_, split)| *split);
                let choice = QuickExportChoice {
                    format: QuickExportFormat::Audio {
                        codec: codec.to_string(),
                        bitrate: "192k".into(),
                    },
                    filters: VideoFilters::default(),
                };
                let _ = trim.send(AudioTrimBarInput::SetBusy(true));
                let input = input.clone();
                let output_dir = output_dir.clone();
                let ranges = ranges.clone();
                let input_sender = input_sender.clone();
                big_media_components::player_chrome::spawn_blocking_result(
                    move || run_segment_export_blocking(input, output_dir, &choice, &ranges, split),
                    move |result| {
                        let _ = input_sender.send(AudioShellInput::TrimExportFinished(result));
                    },
                );
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        ));

        dialog.present(self.window().as_ref());
    }

    fn apply_trim_play_selection(&self, position: f64) {
        let Some((first_start, _)) = self.trim_ranges.first().copied() else {
            return;
        };
        let Some(mpv) = &self.mpv else {
            return;
        };
        let epsilon = 0.02;
        if let Some((idx, (_, end))) = self
            .trim_ranges
            .iter()
            .enumerate()
            .find(|(_, (start, end))| position >= *start - epsilon && position <= *end + epsilon)
        {
            if position >= end - epsilon {
                let next = self
                    .trim_ranges
                    .get(idx + 1)
                    .map_or(first_start, |(start, _)| *start);
                mpv.seek_absolute(next);
            }
        } else if let Some((next_start, _)) = self
            .trim_ranges
            .iter()
            .find(|(start, _)| *start > position)
            .copied()
        {
            mpv.seek_absolute(next_start);
        } else {
            mpv.seek_absolute(first_start);
        }
    }

    /// Resolve the current local track, then run `choice` (with optional trim
    /// `segments`) on a worker thread and toast the outcome.
    fn run_track_export(
        &self,
        choice: QuickExportChoice,
        segments: Vec<(f64, f64)>,
        busy_message: &str,
    ) {
        let Some(input) = self.current_local_input() else {
            self.notify(&i18n("No local track is playing"));
            return;
        };
        let output_dir = input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        self.notify(busy_message);
        self.notify(&i18n("Verified completed outputs can be reused when retrying. Existing files without a matching receipt are kept."));
        let toast_overlay = self.toast_overlay.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || run_single_export_blocking(input, output_dir, &choice, &segments),
            move |result| toast_overlay.add_toast(adw::Toast::new(&export_message(&result))),
        );
    }

    fn current_local_input(&self) -> Option<PathBuf> {
        let input = self
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())
            .unwrap_or_else(|| self.current_file.clone());
        local_input_path(&input)
    }
}

fn local_input_path(input: &str) -> Option<PathBuf> {
    if input.trim().is_empty() {
        return None;
    }
    if let Some((scheme, rest)) = input.split_once("://") {
        if !scheme.eq_ignore_ascii_case("file") {
            return None;
        }
        let authority = rest.split('/').next()?;
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return None;
        }
        use gtk::gio::prelude::FileExt;
        return gtk::gio::File::for_uri(input).path();
    }
    Some(PathBuf::from(input))
}

fn run_segment_export_blocking(
    input: PathBuf,
    output_dir: PathBuf,
    choice: &QuickExportChoice,
    segments: &[(f64, f64)],
    split: bool,
) -> Result<PathBuf, String> {
    big_quick_export::export::validate_export_segments(segments)?;
    let (mut queue, id, output) = build_single_export_queue(input, output_dir, choice, segments);
    if output.as_os_str().is_empty() {
        return Err("export item vanished before start".to_string());
    }
    if split {
        queue.set_pending_segment_mode(SegmentOutputMode::Split);
    }

    queue.start();
    let mut completed = false;
    loop {
        match queue.receiver().recv_blocking() {
            Ok(QueueMessage::Done { entry_id }) if entry_id == id => completed = true,
            Ok(QueueMessage::Cancelled { entry_id }) if entry_id == id => {
                return Err(i18n("Export cancelled"));
            }
            Ok(QueueMessage::Failed { entry_id, error }) if entry_id == id => return Err(error),
            Ok(QueueMessage::AllDone) => {
                if !completed {
                    return Err(i18n("The export ended without a completed result"));
                }
                // Split output has no file at the joined output name. Report the
                // directory containing the separately validated/published files.
                let result = if split && segments.len() > 1 {
                    output
                        .parent()
                        .unwrap_or(std::path::Path::new("."))
                        .to_path_buf()
                } else {
                    output
                };
                let metadata = std::fs::metadata(&result)
                    .map_err(|_| i18n("Could not verify exported files"))?;
                if split && segments.len() > 1 {
                    if !metadata.is_dir() {
                        return Err(i18n("Could not verify exported files"));
                    }
                } else if !metadata.is_file() || metadata.len() == 0 {
                    return Err(i18n("Could not verify exported files"));
                }
                return Ok(result);
            }
            Ok(_) => {}
            Err(_) => return Err(i18n("The export worker stopped before completion")),
        }
    }
}

/// Human-readable toast for a finished export.
fn export_message(result: &Result<PathBuf, String>) -> String {
    match result {
        Ok(path) => format!(
            "{} {}",
            i18n("Saved"),
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
        ),
        Err(error) => format!("{}: {error}", i18n("Export failed")),
    }
}

fn error_label(label: &str) -> gtk::Label {
    let error = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .margin_start(12)
        .margin_end(12)
        .wrap(true)
        .visible(false)
        .build();
    error.add_css_class("error");
    error.update_property(&[gtk::accessible::Property::Label(label)]);
    error
}

/// A flat "current position" suffix button that snaps `row` to mpv's `time-pos`.
fn playhead_button(mpv: &Option<Arc<MpvBackend>>, spin: &gtk::SpinButton) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("preferences-system-time-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&button, i18n("Use current position"));
    button.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Use current playback position",
    ))]);
    let mpv = mpv.clone();
    let spin = spin.downgrade();
    button.connect_clicked(move |_| {
        if let (Some(mpv), Some(spin)) = (mpv.as_ref(), spin.upgrade()) {
            spin.set_value(mpv.get_property_f64("time-pos"));
        }
    });
    button
}

#[cfg(test)]
mod tests {
    use super::local_input_path;
    use std::path::PathBuf;

    #[test]
    fn local_input_path_accepts_plain_paths_and_file_uris() {
        assert_eq!(
            local_input_path("/tmp/song.flac"),
            Some(PathBuf::from("/tmp/song.flac"))
        );
        assert_eq!(
            local_input_path("file:///tmp/song.flac"),
            Some(PathBuf::from("/tmp/song.flac"))
        );
        assert_eq!(
            local_input_path("file://localhost/tmp/song.flac"),
            Some(PathBuf::from("/tmp/song.flac"))
        );
        assert_eq!(
            local_input_path("file:///tmp/My%20Song.flac"),
            Some(PathBuf::from("/tmp/My Song.flac"))
        );
    }

    #[test]
    fn local_input_path_rejects_empty_values_and_streams() {
        assert_eq!(local_input_path(""), None);
        assert_eq!(local_input_path("   "), None);
        assert_eq!(local_input_path("file://remote-host/tmp/song.flac"), None);
        assert_eq!(local_input_path("https://example.test/song.flac"), None);
        assert_eq!(local_input_path("rtmp://example.test/live"), None);
    }
}
