//! Smartphone web-remote dialog wiring. The dialog UI lives in
//! `big_player_ui::smartphone`; this module only builds the `RemoteHandlers`
//! that bridge the HTTP worker thread back to the shell.
//!
//! Control closures send `AudioShellInput` over a (Send+Sync) Relm4 sender —
//! the shell reuses its normal transport handlers. The status snapshot reads
//! the thread-safe `SharedMprisState`, so it needs no main-loop bounce.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use adw::prelude::*;
use big_player_ui::playback_bar::AudioPlaybackBarOutput;
use big_player_ui::smartphone::{RemoteHandlers, StatusSnapshot};
use relm4::Sender;

use crate::audio::mpris_state::SharedMprisState;
use crate::audio::ui::audio_shell::AudioShellInput;

/// Present the smartphone dialog (web remote + QR + KDE Connect note).
pub(crate) fn present_smartphone(
    parent: &impl IsA<gtk::Widget>,
    input: Sender<AudioShellInput>,
    mpris_state: Arc<SharedMprisState>,
) {
    big_player_ui::smartphone::present(parent, move || {
        build_handlers(input.clone(), mpris_state.clone())
    });
}

fn build_handlers(input: Sender<AudioShellInput>, state: Arc<SharedMprisState>) -> RemoteHandlers {
    let remote = |input: Sender<AudioShellInput>, out: AudioPlaybackBarOutput| {
        let _ = input.send(AudioShellInput::PlaybackRemote(out));
    };

    let i = input.clone();
    let play_pause: Arc<dyn Fn() + Send + Sync> =
        Arc::new(move || remote(i.clone(), AudioPlaybackBarOutput::TogglePlay));
    let i = input.clone();
    let next: Arc<dyn Fn() + Send + Sync> =
        Arc::new(move || remote(i.clone(), AudioPlaybackBarOutput::Next));
    let i = input.clone();
    let prev: Arc<dyn Fn() + Send + Sync> =
        Arc::new(move || remote(i.clone(), AudioPlaybackBarOutput::Previous));
    let i = input.clone();
    let set_volume: Arc<dyn Fn(f64) + Send + Sync> = Arc::new(move |percent: f64| {
        remote(i.clone(), AudioPlaybackBarOutput::VolumeChanged(percent));
    });

    let st = state.clone();
    let status: Arc<dyn Fn() -> StatusSnapshot + Send + Sync> = Arc::new(move || StatusSnapshot {
        playing: !st.paused.load(Ordering::Relaxed),
        title: st.title.lock().map(|g| g.clone()).unwrap_or_default(),
        artist: st.artist.lock().map(|g| g.clone()).unwrap_or_default(),
        position_s: st.position.lock().map_or(0.0, |g| *g),
        duration_s: st.duration.lock().map_or(0.0, |g| *g),
        volume: st.volume.lock().map_or(0.0, |g| *g),
    });

    RemoteHandlers {
        play_pause,
        next,
        prev,
        set_volume,
        status,
    }
}
