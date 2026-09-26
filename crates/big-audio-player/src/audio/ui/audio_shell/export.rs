//! Trim & Export and Convert for the audio player: mark cuts on the playing
//! track, then hand the track (and the cuts) to the audio converter GUI. The
//! player converts nothing itself.

use std::path::PathBuf;

use big_relm4_components::input::preference_cards::BigSpinRange;
use big_relm4_components::input::spin_row::{BigSpinRow, BigSpinRowSpec};
use big_relm4_components::layout::dialog_footer::{BigDialogFooter, BigDialogFooterSpec};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use super::*;

impl AudioShell {
    /// Open the current local track in the audio converter, keeping only `cuts`
    /// when any.
    pub(super) fn open_in_converter(&self, cuts: &[(f64, f64)]) {
        let Some(input) = self.current_local_input() else {
            self.notify(&i18n("No local track is playing"));
            return;
        };
        if let Some(failure) = converter_handoff::open(
            converter_handoff::AUDIO_CONVERTER,
            &[input.to_string_lossy().into_owned()],
            cuts,
            None,
        ) {
            self.notify(&failure);
        }
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
    }

    /// Leave visual trim mode and restore the normal seek bar.
    pub(super) fn exit_trim_mode(&mut self) {
        self.trim_active = false;
        self.trim_play_selection = false;
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetEditMode(false));
    }

    /// Handle a typed output from the visual trim bar.
    pub(super) fn handle_trim(&mut self, output: AudioTrimBarOutput) {
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
            AudioTrimBarOutput::ExportRequested { ranges } => self.open_in_converter(&ranges),
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
