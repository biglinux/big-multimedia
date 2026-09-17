//! Smartphone web-remote dialog wiring. The dialog UI lives in
//! `big_player_ui::smartphone`; this module only builds the `RemoteHandlers`
//! that bridge the HTTP worker thread back to the shell.
//!
//! Control closures send `AudioShellInput` over a (Send+Sync) Relm4 sender —
//! the shell reuses its normal transport handlers. The status snapshot reads
//! the thread-safe `SharedMprisState`, so it needs no main-loop bounce.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

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
    big_player_ui::smartphone::present(parent, move |revoked| {
        build_handlers(input.clone(), mpris_state.clone(), revoked)
    });
}

/// One slot held from network admission until GUI consumption or channel drop.
#[derive(Debug)]
pub struct RemoteActionPermit {
    revoked: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}
impl RemoteActionPermit {
    pub(crate) fn is_live(&self) -> bool {
        !self.revoked.load(Ordering::Acquire)
    }
}
impl Drop for RemoteActionPermit {
    fn drop(&mut self) {
        self.pending.fetch_sub(1, Ordering::Release);
    }
}
fn build_handlers(
    input: Sender<AudioShellInput>,
    state: Arc<SharedMprisState>,
    revoked: Arc<AtomicBool>,
) -> RemoteHandlers {
    let pending = Arc::new(AtomicUsize::new(0));
    let remote = Arc::new(
        move |input: Sender<AudioShellInput>, out: AudioPlaybackBarOutput| -> bool {
            if revoked.load(Ordering::Acquire) {
                return false;
            }
            if pending
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                    (n < 16).then_some(n + 1)
                })
                .is_err()
            {
                return false;
            }
            let permit = RemoteActionPermit {
                revoked: revoked.clone(),
                pending: pending.clone(),
            };
            input
                .send(AudioShellInput::PlaybackRemote {
                    command: out,
                    permit,
                })
                .is_ok()
        },
    );

    let i = input.clone();
    let dispatch = remote.clone();
    let play_pause: Arc<dyn Fn() -> bool + Send + Sync> =
        Arc::new(move || dispatch(i.clone(), AudioPlaybackBarOutput::TogglePlay));
    let i = input.clone();
    let dispatch = remote.clone();
    let next: Arc<dyn Fn() -> bool + Send + Sync> =
        Arc::new(move || dispatch(i.clone(), AudioPlaybackBarOutput::Next));
    let i = input.clone();
    let dispatch = remote.clone();
    let prev: Arc<dyn Fn() -> bool + Send + Sync> =
        Arc::new(move || dispatch(i.clone(), AudioPlaybackBarOutput::Previous));
    let i = input.clone();
    let dispatch = remote.clone();
    let set_volume: Arc<dyn Fn(f64) -> bool + Send + Sync> = Arc::new(move |percent: f64| {
        dispatch(i.clone(), AudioPlaybackBarOutput::VolumeChanged(percent))
    });

    let st = state.clone();
    let status: Arc<dyn Fn() -> StatusSnapshot + Send + Sync> = Arc::new(move || {
        let snapshot = st
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        StatusSnapshot {
            playing: snapshot.status == crate::mpris::PlaybackStatus::Playing,
            title: snapshot.title.clone(),
            artist: snapshot.artist.clone(),
            position_s: snapshot.position_secs,
            duration_s: snapshot.duration_secs,
            volume: snapshot.volume_pct,
        }
    });

    RemoteHandlers {
        play_pause,
        next,
        prev,
        set_volume,
        status,
    }
}
