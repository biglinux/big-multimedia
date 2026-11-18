//! Trim & Export and Convert for the audio player: mark cuts on the playing
//! track, then hand the track (and the cuts) to the audio converter GUI. The
//! player converts nothing itself.

use std::path::PathBuf;

use big_player_ui::cut_edit_dialog::present_cut_edit_dialog;
use big_player_ui::cut_list::{Cut, play_only_cuts_seek};

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
                self.trim_ranges = segments
                    .iter()
                    .map(|(_, start, end)| (*start, *end))
                    .collect();
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
        let mpv = self.mpv.clone();
        let trim = self.trim.sender().clone();
        present_cut_edit_dialog(
            self.playback_bar.widget(),
            Cut { id, start, end },
            self.duration_secs.max(1.0),
            move || {
                mpv.as_ref()
                    .map_or(0.0, |mpv| mpv.get_property_f64("time-pos"))
            },
            move |id, start, end| {
                let _ = trim.send(AudioTrimBarInput::SetSegmentTimes(id, start, end));
            },
        );
    }

    fn apply_trim_play_selection(&self, position: f64) {
        if let (Some(target), Some(mpv)) = (
            play_only_cuts_seek(&self.trim_ranges, position),
            self.mpv.as_ref(),
        ) {
            mpv.seek_absolute(target);
        }
    }

    fn current_local_input(&self) -> Option<PathBuf> {
        let input = self
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.get_current_file())
            .unwrap_or_else(|| self.current_file.clone());
        big_os_kit::url::local_path(&input)
    }
}
