//! Relm4 wrapper around [`AudioEditPage`] (inline audio editor).
//!
//! Push-model Component: the live `apply_trim` signal (fired on every waveform
//! edit) emits [`AudioEditorOutput::SegmentsApplied`] directly; the convert
//! button routes to [`AudioEditorInput::ConvertRequested`] so `update()` can
//! gather `trim_ranges()` and emit [`AudioEditorOutput::ApplyAndConvert`]. The
//! shared volume/speed adjustments arrive via [`AudioEditorInit`] (owned by the
//! shell, also bound into the sidebar) so both stay in sync without messages.
//! mpv internals untouched. Manual `impl Component` idiom (no `view!`).

use std::path::PathBuf;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::converter::ffmpeg::TrimRange;
use crate::player::mpv_backend::AudioEffectsSnapshot;

use super::AudioEditPage;

/// Boot configuration for [`AudioEditor`].
pub struct AudioEditorInit {
    /// Render in the compact inline layout (true in the converter).
    pub compact: bool,
}

/// Messages accepted by [`AudioEditor`].
#[derive(Debug)]
pub enum AudioEditorInput {
    /// Bind the shared volume/speed adjustments (owned by the sidebar) so the
    /// editor's transport controls and the sidebar rows stay in sync.
    BindAdjustments {
        /// Shared volume adjustment.
        volume: gtk::Adjustment,
        /// Shared speed adjustment.
        speed: gtk::Adjustment,
    },
    /// Load `path` into the editor + waveform (allocates mpv lazily).
    LoadFile(PathBuf),
    /// Replace the editor's trim segments.
    SetSegments(Vec<TrimRange>),
    /// Toggle whether trim/cut mode is active.
    SetCutModeActive(bool),
    /// Start playback.
    Play,
    /// Stop playback.
    Stop,
    /// Toggle play/pause.
    TogglePlayPause,
    /// Reset the waveform zoom.
    ResetZoom,
    /// Push live audio-filter effects + volume/speed to the preview.
    ApplyPreview {
        /// Effect set snapshot.
        effects: AudioEffectsSnapshot,
        /// Linear volume factor.
        volume: f64,
        /// Playback speed factor.
        speed: f64,
    },
    /// Tear down the loaded file + mpv backend.
    Unload,
    /// Internal: convert clicked — gather ranges and emit
    /// [`AudioEditorOutput::ApplyAndConvert`].
    ConvertRequested,
}

/// User actions emitted by [`AudioEditor`].
#[derive(Debug)]
pub enum AudioEditorOutput {
    /// Segments edited (live); carries the current trim ranges.
    SegmentsApplied(Vec<TrimRange>),
    /// Convert clicked; carries the final trim ranges.
    ApplyAndConvert(Vec<TrimRange>),
    /// Open the audio-settings (noise) dialog.
    ShowAudioSettings,
    /// Open the equalizer dialog.
    ShowEqualizer,
    /// Playback state changed (`true` = playing).
    PlaybackStateChanged(bool),
    /// Previous-track button pressed.
    PreviousTrack,
    /// Next-track button pressed.
    NextTrack,
}

/// Relm4 wrapper around the inline [`AudioEditPage`].
pub struct AudioEditor {
    page: AudioEditPage,
}

impl Component for AudioEditor {
    type Init = AudioEditorInit;
    type Input = AudioEditorInput;
    type Output = AudioEditorOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let page = AudioEditPage::new();
        root.append(page.widget());

        if init.compact {
            page.set_compact_mode(true);
        }

        let s = sender.clone();
        page.connect_apply_trim(move |ranges| {
            let _ = s.output(AudioEditorOutput::SegmentsApplied(ranges));
        });
        let s = sender.clone();
        page.connect_show_audio_settings(move |()| {
            let _ = s.output(AudioEditorOutput::ShowAudioSettings);
        });
        let s = sender.clone();
        page.connect_show_equalizer(move |()| {
            let _ = s.output(AudioEditorOutput::ShowEqualizer);
        });
        let s = sender.clone();
        page.connect_playback_state_changed(move |playing| {
            let _ = s.output(AudioEditorOutput::PlaybackStateChanged(playing));
        });
        let s = sender.clone();
        page.connect_previous_track(move |()| {
            let _ = s.output(AudioEditorOutput::PreviousTrack);
        });
        let s = sender.clone();
        page.connect_next_track(move |()| {
            let _ = s.output(AudioEditorOutput::NextTrack);
        });
        let s = sender.clone();
        page.connect_convert(move |()| {
            s.input(AudioEditorInput::ConvertRequested);
        });

        ComponentParts {
            model: Self { page },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            AudioEditorInput::BindAdjustments { volume, speed } => {
                self.page.bind_volume_adjustment(&volume);
                self.page.bind_speed_adjustment(&speed);
            }
            AudioEditorInput::LoadFile(path) => self.page.load_file(&path),
            AudioEditorInput::SetSegments(segments) => self.page.set_segments(&segments),
            AudioEditorInput::SetCutModeActive(active) => self.page.set_cut_mode_active(active),
            AudioEditorInput::Play => self.page.play(),
            AudioEditorInput::Stop => self.page.stop(),
            AudioEditorInput::TogglePlayPause => self.page.toggle_play_pause(),
            AudioEditorInput::ResetZoom => self.page.reset_zoom(),
            AudioEditorInput::ApplyPreview {
                effects,
                volume,
                speed,
            } => {
                self.page
                    .apply_audio_preview(&effects.as_effects(), volume, speed);
            }
            AudioEditorInput::Unload => self.page.unload(),
            AudioEditorInput::ConvertRequested => {
                let ranges = self.page.trim_ranges();
                let _ = sender.output(AudioEditorOutput::ApplyAndConvert(ranges));
            }
        }
    }
}
