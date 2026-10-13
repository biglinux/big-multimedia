//! Relm4 wrapper around `BigAudioTransport` (the audio playback bar).
//!
//! Push-model Component: the transport buttons and hover popovers emit typed
//! `AudioPlaybackBarOutput` intents; the shell owns mpv/MPRIS/settings and
//! pushes display state back via `AudioPlaybackBarInput`. The component owns
//! its three hover popovers internally (volume/speed/voice), built with the
//! shared `big-media-runtime` factories, so the side effects stay app-side.
//! Manual `impl Component` idiom (no `view!` macro).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use big_media_components::playback::audio_transport::{BigAudioTransport, BigAudioTransportSpec};
use big_media_components::playback::queue::RepeatMode;

use big_media_components::player_chrome::{
    build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group,
};

use crate::cut_timeline::{install_cut_timeline_draw, wire_cut_timeline_interaction};

/// Construction parameters for [`AudioPlaybackBar`].
#[derive(Clone)]
pub struct AudioPlaybackBarInit {
    /// Transport widget spec (translated labels + whether voice is available).
    pub spec: BigAudioTransportSpec,
    /// Maximum volume percent the volume slider allows (e.g. 150).
    pub max_volume: f64,
    /// Initial volume percent for the slider.
    pub initial_volume: f64,
    /// Initial voice-enhancement strength percent (0 when inactive).
    pub initial_voice_pct: f64,
    /// Optional trim controls shown above the transport while trim mode is active.
    pub trim_content: Option<gtk::Widget>,
    /// The noise reduction's settings, shown under the voice slider.
    pub voice_settings: gtk::Widget,
}

/// Display state pushed into the bar by the shell.
#[derive(Debug)]
pub enum AudioPlaybackBarInput {
    /// Enable/disable the transport controls (play/prev/next/shuffle/repeat/seek).
    SetControlsSensitive(bool),
    /// Reflect the paused state on the play button icon.
    SetPaused(bool),
    /// Set the elapsed-position label text.
    SetPosition(String),
    /// Set the total-duration label text.
    SetDuration(String),
    /// Set the seek slider value (`0..=100`); does not emit a seek intent.
    SetSeekValue(f64),
    /// Set the media duration in seconds for trim timeline geometry.
    SetDurationSecs(f64),
    /// Trim mode: each cut's `(id, start, end)` to paint on the cut timeline.
    SetCutSegments(Vec<(u64, f64, f64)>),
    /// Trim mode: pending first mark, if any.
    SetPendingMark(Option<f64>),
    /// Trim mode on/off: swaps the seek scale for the cut timeline and reveals
    /// trim controls above the transport.
    SetEditMode(bool),
    /// Reflect the shuffle state on the toggle without emitting.
    SetShuffleActive(bool),
    /// Reflect the repeat mode (icon + active) without emitting.
    SetRepeat(RepeatMode),
    /// Set the status label text.
    SetStatus(String),
    /// Set the volume percent label and button icon.
    SetVolumeIndicator {
        /// Percent label, e.g. `"100"`.
        label: String,
        /// Symbolic icon name for the current volume/mute state.
        icon: String,
    },
    /// Set the volume slider value (`0..=max_volume`) without emitting.
    SetVolumeValue(f64),
    /// Whose volume the slider moves: the TV's while a TV has the transport.
    SetVolumeOfTv(bool),
    /// Toggle the `voice-active` style on the voice button.
    SetVoiceActive(bool),
    /// Reflect the lyrics-panel visibility on the toggle without emitting.
    SetLyricsActive(bool),
    /// Show/hide the playlist-management button (apps with an always-visible
    /// queue hide it).
    SetPlaylistButtonVisible(bool),
    /// Set (or clear) the blurred album-art background.
    SetBlurArt(Option<gtk::gdk::Texture>),
}

/// User intents emitted by the bar for the shell to act on.
#[derive(Debug)]
pub enum AudioPlaybackBarOutput {
    /// Play/pause toggled.
    TogglePlay,
    /// Skip to the next track.
    Next,
    /// Skip to the previous track.
    Previous,
    /// Seek slider moved by the user; carries the raw `0..=100` value.
    SeekChanged(f64),
    /// A cut's edge/body was dragged on the cut timeline.
    SegmentEdgeAdjusted {
        /// Segment id.
        id: u64,
        /// New start in seconds.
        start: f64,
        /// New end in seconds.
        end: f64,
    },
    /// Shuffle toggled; carries the new active state.
    ToggleShuffle(bool),
    /// Repeat button clicked: advance to the next repeat mode.
    CycleRepeat,
    /// Lyrics toggled; carries the new active state.
    ToggleLyrics(bool),
    /// Playlist-management button clicked.
    ShowPlaylist,
    /// Open the visual trim/export mode for the current track.
    ShowTrimExport,
    /// Volume slider moved; carries the new percent.
    VolumeChanged(f64),
    /// Volume button clicked (mute/unmute).
    VolumeMuteToggled,
    /// Speed slider moved; carries the new multiplier.
    SpeedChanged(f64),
    /// Voice strength slider moved; carries the new percent.
    VoiceChanged(f64),
}

/// Relm4 wrapper around the audio [`BigAudioTransport`].
pub struct AudioPlaybackBar {
    transport: BigAudioTransport,
    trim_controls_revealer: gtk::Revealer,
    cut_segments: Rc<RefCell<Vec<(u64, f64, f64)>>>,
    pending_mark: Rc<RefCell<Option<f64>>>,
    seek_fraction: Rc<Cell<f64>>,
    duration_secs: Rc<Cell<f64>>,
    volume_scale: gtk::Scale,
    /// Guards programmatic toggle/slider updates from re-emitting intents.
    updating: Rc<Cell<bool>>,
    /// Hover popovers kept alive for the lifetime of the bar.
    _popovers: Vec<gtk::Popover>,
}

/// Map a [`RepeatMode`] to its toggle (active, icon-name) indicator pair.
#[must_use]
fn repeat_indicator(mode: RepeatMode) -> (bool, &'static str) {
    match mode {
        RepeatMode::None => (false, "media-playlist-repeat-symbolic"),
        RepeatMode::All => (true, "media-playlist-repeat-symbolic"),
        RepeatMode::One => (true, "media-playlist-repeat-song-symbolic"),
    }
}

impl Component for AudioPlaybackBar {
    type Init = AudioPlaybackBarInit;
    type Input = AudioPlaybackBarInput;
    type Output = AudioPlaybackBarOutput;
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
        let AudioPlaybackBarInit {
            spec,
            max_volume,
            initial_volume,
            initial_voice_pct,
            trim_content,
            voice_settings,
        } = init;

        let transport = BigAudioTransport::new(spec);
        let trim_controls_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(false)
            .build();
        if let Some(trim_content) = trim_content {
            trim_controls_revealer.set_child(Some(&trim_content));
        }
        root.append(&trim_controls_revealer);
        root.append(transport.root());

        let updating = Rc::new(Cell::new(false));
        let cut_segments = Rc::new(RefCell::new(Vec::new()));
        let pending_mark = Rc::new(RefCell::new(None));
        let seek_fraction = Rc::new(Cell::new(0.0));
        let duration_secs = Rc::new(Cell::new(0.0));
        install_cut_timeline_draw(
            transport.cut_timeline(),
            &cut_segments,
            &pending_mark,
            &seek_fraction,
            &duration_secs,
        );
        let seek_sender = sender.clone();
        let segment_sender = sender.clone();
        wire_cut_timeline_interaction(
            transport.cut_timeline(),
            &cut_segments,
            &duration_secs,
            move |fraction| {
                let _ = seek_sender.output(AudioPlaybackBarOutput::SeekChanged(fraction * 100.0));
            },
            move |adjustment| {
                let _ = segment_sender.output(AudioPlaybackBarOutput::SegmentEdgeAdjusted {
                    id: adjustment.id,
                    start: adjustment.start,
                    end: adjustment.end,
                });
            },
        );

        wire_transport_signals(&transport, &sender, &updating);
        let (volume_scale, popovers) = build_hover_popovers(
            &transport,
            max_volume,
            initial_volume,
            initial_voice_pct,
            &voice_settings,
            &sender,
            &updating,
        );

        ComponentParts {
            model: Self {
                transport,
                trim_controls_revealer,
                cut_segments,
                pending_mark,
                seek_fraction,
                duration_secs,
                volume_scale,
                updating,
                _popovers: popovers,
            },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>, _root: &Self::Root) {
        let t = &self.transport;
        match message {
            AudioPlaybackBarInput::SetControlsSensitive(on) => {
                t.play_button().set_sensitive(on);
                t.previous_button().set_sensitive(on);
                t.next_button().set_sensitive(on);
                t.shuffle_button().set_sensitive(on);
                t.repeat_button().set_sensitive(on);
                t.trim_export_button().set_sensitive(on);
                t.seek_bar().set_sensitive(on);
                t.cut_timeline().set_sensitive(on);
            }
            AudioPlaybackBarInput::SetPaused(paused) => t.set_paused(paused),
            AudioPlaybackBarInput::SetPosition(text) => t.position_label().set_text(Some(&text)),
            AudioPlaybackBarInput::SetDuration(text) => {
                let chars =
                    big_media_components::playback::audio_transport::time_label_chars(&text);
                t.position_label().set_min_chars(chars);
                t.position_label().set_nat_chars(chars);
                t.duration_label().set_label(&text);
            }
            AudioPlaybackBarInput::SetSeekValue(value) => {
                let value = value.clamp(0.0, 100.0);
                self.seek_fraction.set(value / 100.0);
                // Each move repaints the window. Position arrives ten times a
                // second, while a long track moves the knob a pixel every few
                // seconds, and GTK places the knob on whole pixels: a smaller
                // step repaints an identical frame (a quarter of all frames
                // playing music, measured on an Intel HD 3000, 2026-10-02).
                let bar = t.seek_bar();
                let width = f64::from(bar.width().max(t.cut_timeline().width()));
                // Unallocated (hidden) bars draw nothing: keep them current.
                if width > 0.0 && ((value - bar.value()) / 100.0 * width).abs() < 1.0 {
                    return;
                }
                self.with_guard(|| bar.set_value(value));
                t.cut_timeline().queue_draw();
            }
            AudioPlaybackBarInput::SetDurationSecs(secs) => {
                self.duration_secs.set(secs.max(0.0));
                t.cut_timeline().queue_draw();
            }
            AudioPlaybackBarInput::SetCutSegments(segments) => {
                self.cut_segments.replace(segments);
                t.cut_timeline().queue_draw();
            }
            AudioPlaybackBarInput::SetPendingMark(mark) => {
                self.pending_mark.replace(mark);
                t.cut_timeline().queue_draw();
            }
            AudioPlaybackBarInput::SetEditMode(on) => {
                t.set_cut_timeline_visible(on);
                self.trim_controls_revealer.set_reveal_child(on);
            }
            AudioPlaybackBarInput::SetShuffleActive(active) => {
                self.with_guard(|| t.shuffle_button().set_active(active));
            }
            AudioPlaybackBarInput::SetRepeat(mode) => {
                let (active, icon) = repeat_indicator(mode);
                self.with_guard(|| {
                    t.repeat_button().set_active(active);
                    t.repeat_button().set_icon_name(icon);
                });
            }
            AudioPlaybackBarInput::SetStatus(text) => t.status_label().set_label(&text),
            AudioPlaybackBarInput::SetVolumeIndicator { label, icon } => {
                t.volume_label().set_label(&label);
                t.volume_button().set_icon_name(&icon);
            }
            AudioPlaybackBarInput::SetVolumeValue(value) => {
                let scale = self.volume_scale.clone();
                self.with_guard(|| scale.set_value(value));
            }
            AudioPlaybackBarInput::SetVolumeOfTv(tv) => {
                use big_media_components::text::text;
                let label = if tv {
                    text("TV volume")
                } else {
                    text("Volume")
                };
                // The volume popover's heading is its content's first child.
                if let Some(title) = self
                    ._popovers
                    .first()
                    .and_then(|popover| popover.child())
                    .and_then(|content| content.first_child())
                    .and_downcast::<gtk::Label>()
                {
                    title.set_label(&label);
                }
                self.volume_scale
                    .update_property(&[gtk::accessible::Property::Label(&label)]);
            }
            AudioPlaybackBarInput::SetVoiceActive(active) => {
                if active {
                    t.voice_button().add_css_class("voice-active");
                } else {
                    t.voice_button().remove_css_class("voice-active");
                }
            }
            AudioPlaybackBarInput::SetLyricsActive(active) => {
                self.with_guard(|| t.lyrics_button().set_active(active));
            }
            AudioPlaybackBarInput::SetPlaylistButtonVisible(visible) => {
                t.playlist_button().set_visible(visible);
            }
            AudioPlaybackBarInput::SetBlurArt(texture) => {
                t.blur_bg().set_paintable(texture.as_ref());
                t.blur_bg().set_visible(texture.is_some());
            }
        }
    }
}

impl AudioPlaybackBar {
    /// Run `f` with the re-entrancy guard set so toggle/slider handlers do not
    /// echo a programmatic update back out as a user intent.
    fn with_guard(&self, f: impl FnOnce()) {
        self.updating.set(true);
        f();
        self.updating.set(false);
    }
}

/// Wire the transport buttons and seek slider to output intents.
fn wire_transport_signals(
    transport: &BigAudioTransport,
    sender: &ComponentSender<AudioPlaybackBar>,
    updating: &Rc<Cell<bool>>,
) {
    let s = sender.clone();
    transport.play_button().connect_clicked(move |button| {
        big_media_components::player_chrome::pulse_play_button(button);
        let _ = s.output(AudioPlaybackBarOutput::TogglePlay);
    });

    let s = sender.clone();
    transport.previous_button().connect_clicked(move |_| {
        let _ = s.output(AudioPlaybackBarOutput::Previous);
    });

    let s = sender.clone();
    transport.next_button().connect_clicked(move |_| {
        let _ = s.output(AudioPlaybackBarOutput::Next);
    });

    let s = sender.clone();
    let guard = updating.clone();
    transport
        .seek_bar()
        .connect_change_value(move |_scale, _scroll, value| {
            if guard.get() {
                return gtk::glib::Propagation::Proceed;
            }
            let _ = s.output(AudioPlaybackBarOutput::SeekChanged(value));
            gtk::glib::Propagation::Proceed
        });

    let s = sender.clone();
    let guard = updating.clone();
    transport.shuffle_button().connect_toggled(move |button| {
        if guard.get() {
            return;
        }
        let _ = s.output(AudioPlaybackBarOutput::ToggleShuffle(button.is_active()));
    });

    let s = sender.clone();
    let guard = updating.clone();
    transport.repeat_button().connect_toggled(move |_| {
        if guard.get() {
            return;
        }
        let _ = s.output(AudioPlaybackBarOutput::CycleRepeat);
    });

    let s = sender.clone();
    let guard = updating.clone();
    transport.lyrics_button().connect_toggled(move |button| {
        if guard.get() {
            return;
        }
        let _ = s.output(AudioPlaybackBarOutput::ToggleLyrics(button.is_active()));
    });

    let s = sender.clone();
    transport.playlist_button().connect_clicked(move |_| {
        let _ = s.output(AudioPlaybackBarOutput::ShowPlaylist);
    });

    let s = sender.clone();
    transport.trim_export_button().connect_clicked(move |_| {
        let _ = s.output(AudioPlaybackBarOutput::ShowTrimExport);
    });
}

/// Build the volume/speed/voice hover popovers and the exclusive hover group.
/// Returns the volume scale (for programmatic updates) and the popovers to
/// keep alive.
fn build_hover_popovers(
    transport: &BigAudioTransport,
    max_volume: f64,
    initial_volume: f64,
    initial_voice_pct: f64,
    voice_settings: &gtk::Widget,
    sender: &ComponentSender<AudioPlaybackBar>,
    updating: &Rc<Cell<bool>>,
) -> (gtk::Scale, Vec<gtk::Popover>) {
    let (vol_popover, volume_scale) =
        build_volume_popover(transport.volume_hover_box(), max_volume, initial_volume);
    let s = sender.clone();
    let guard = updating.clone();
    volume_scale.connect_value_changed(move |scale| {
        // Skip when the value was set programmatically to mirror the system mixer
        // (`SetVolumeValue` under `with_guard`); otherwise it echoes back out as a
        // user intent and fights the external change.
        if guard.get() {
            return;
        }
        let _ = s.output(AudioPlaybackBarOutput::VolumeChanged(scale.value()));
    });
    let s = sender.clone();
    transport.volume_button().connect_clicked(move |_| {
        let _ = s.output(AudioPlaybackBarOutput::VolumeMuteToggled);
    });

    let s = sender.clone();
    let spd_popover = build_speed_popover(
        transport.speed_hover_box(),
        transport.speed_button(),
        transport.speed_label(),
        move |speed| {
            let _ = s.output(AudioPlaybackBarOutput::SpeedChanged(speed));
        },
    );

    let s_value = sender.clone();
    let voice_popover = build_voice_popover(
        transport.voice_hover_box(),
        transport.voice_button(),
        transport.voice_label(),
        initial_voice_pct,
        move |pct| {
            let _ = s_value.output(AudioPlaybackBarOutput::VoiceChanged(pct));
        },
        voice_settings,
    );

    let popovers = setup_exclusive_hover_popover_group([
        (transport.volume_hover_box(), &vol_popover),
        (transport.speed_hover_box(), &spd_popover),
        (transport.voice_hover_box(), &voice_popover),
    ]);

    (volume_scale, popovers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_mode_maps_to_indicator() {
        assert_eq!(
            repeat_indicator(RepeatMode::None),
            (false, "media-playlist-repeat-symbolic")
        );
        assert_eq!(
            repeat_indicator(RepeatMode::All),
            (true, "media-playlist-repeat-symbolic")
        );
        assert_eq!(
            repeat_indicator(RepeatMode::One),
            (true, "media-playlist-repeat-song-symbolic")
        );
    }
}
