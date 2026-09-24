//! MPRIS2 for the video player: mirror mpv state into the snapshot the D-Bus
//! service polls, and turn bus commands into mpv calls.

use super::*;
use big_app_kit::mpris::{
    CommandCallback, Mpris, MprisCommand, PlaybackStatus, PlayerState, StateCallback,
};
use std::sync::{Mutex, PoisonError};

/// Start the service. Commands cross a `Send` channel because the shell's own
/// input type carries GTK objects and cannot leave the main thread.
pub(super) fn build_mpris_service(
    sender: &ComponentSender<VideoShell>,
    state: &Arc<Mutex<PlayerState>>,
    settings: &Settings,
) -> Option<Mpris> {
    let (command_tx, command_rx) = relm4::channel::<MprisCommand>();
    let input = sender.input_sender().clone();
    glib::spawn_future_local(async move {
        // Ends when the service (and with it `command_tx`) is dropped.
        while let Some(command) = command_rx.recv().await {
            if input.send(VideoShellInput::Mpris(command)).is_err() {
                break;
            }
        }
    });
    let command_cb: CommandCallback = Box::new(move |command| command_tx.emit(command));
    let state = state.clone();
    let settings = settings.clone();
    let state_cb: StateCallback = Box::new(move || {
        let mut snapshot = state.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // The settings are the source of truth: every path that changes repeat
        // or shuffle (boot, Preferences, playlist menu, MPRIS) applies them to mpv.
        let status = match settings.get_string(keys::LOOP_MODE).as_str() {
            "file" => "Track",
            "playlist" => "Playlist",
            _ => "None",
        };
        status.clone_into(&mut snapshot.loop_status);
        snapshot.shuffle = settings.get_bool(keys::PLAYLIST_SHUFFLE);
        snapshot
    });
    // `new` is the video identity (BigVideoPlayer bus name, no taskbar progress).
    Mpris::new(command_cb, state_cb)
}

fn finite(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

impl VideoShell {
    /// Mirror one mpv event into the MPRIS snapshot.
    pub(super) fn mirror_mpris(&mut self, event: &PlayerEvent) {
        let Some(mpris) = &self.mpris else {
            return;
        };
        let mpv = self.mpv.borrow().clone();
        let mut state = self
            .mpris_state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match event {
            PlayerEvent::FileLoaded(_) => {
                self.mpris_track_seq += 1;
                state.track_id = Some(format!(
                    "/br/com/biglinux/VideoPlayer/Track/{}",
                    self.mpris_track_seq
                ));
                state.position_secs = 0.0;
                state.can_play = true;
                state.can_pause = true;
                if let Some(mpv) = mpv {
                    state.title = mpv.get_media_title().unwrap_or_default();
                    state.duration_secs = finite(mpv.get_duration());
                    state.status = if mpv.is_paused() {
                        PlaybackStatus::Paused
                    } else {
                        PlaybackStatus::Playing
                    };
                    state.can_seek = mpv.is_seekable();
                    let (index, count) = (mpv.get_playlist_pos(), mpv.get_playlist_count());
                    state.can_prev = index > 0;
                    // Repeat-playlist wraps past the last entry.
                    let wraps =
                        count > 1 && self.settings.get_string(keys::LOOP_MODE) == "playlist";
                    state.can_next = wraps || (index >= 0 && index + 1 < count);
                    state.volume_pct = finite(mpv.get_volume());
                    state.rate = mpv.get_speed();
                }
            }
            PlayerEvent::MetadataChanged(_) => {
                if let Some(title) = mpv.and_then(|mpv| mpv.get_media_title()) {
                    state.title = title;
                }
            }
            PlayerEvent::PositionChanged(position) => state.position_secs = finite(*position),
            PlayerEvent::DurationChanged(duration) => state.duration_secs = finite(*duration),
            PlayerEvent::PauseChanged(paused) if state.track_id.is_some() => {
                state.status = if *paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
            }
            PlayerEvent::VolumeChanged(volume) => state.volume_pct = finite(*volume),
            PlayerEvent::RateChanged(rate) => state.rate = *rate,
            PlayerEvent::Seeked(position) => {
                state.position_secs = finite(*position);
                mpris.emit_seeked((state.position_secs * 1_000_000.0).round() as i64);
            }
            // Between playlist entries the next `FileLoaded` restores the track.
            // Past the last entry (or after Stop) mpv sits idle with nothing to
            // resume, so nothing is offered until a file loads again.
            PlayerEvent::EndFile(_) => {
                *state = PlayerState {
                    volume_pct: state.volume_pct,
                    rate: state.rate,
                    ..PlayerState::default()
                };
            }
            _ => {}
        }
    }

    /// Act on a command from the bus, revalidated against the current snapshot.
    pub(super) fn handle_mpris_command(
        &self,
        command: MprisCommand,
        sender: &ComponentSender<Self>,
        window: Option<&gtk::Window>,
    ) {
        let state = self
            .mpris_state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let current_track =
            |track_id: &str| state.can_seek && state.track_id.as_deref() == Some(track_id);
        match command {
            MprisCommand::Raise => {
                if let Some(window) = window {
                    window.present();
                }
            }
            MprisCommand::Quit => {
                if let Some(window) = window {
                    window.close();
                }
            }
            MprisCommand::OpenUri(uri) => {
                let path = gio::File::for_uri(&uri)
                    .path()
                    .and_then(|path| path.to_str().map(str::to_owned))
                    .unwrap_or(uri);
                sender.input(VideoShellInput::OpenFiles(vec![path]));
            }
            command => {
                let Some(mpv) = self.mpv.borrow().clone() else {
                    return;
                };
                match command {
                    MprisCommand::Play => mpv.play(),
                    MprisCommand::Pause => mpv.pause(),
                    MprisCommand::PlayPause => mpv.toggle_pause(),
                    MprisCommand::Stop => mpv.stop(),
                    MprisCommand::Next if state.can_next => mpv.playlist_next(),
                    MprisCommand::Previous if state.can_prev => mpv.playlist_prev(),
                    MprisCommand::Seek {
                        track_id,
                        offset_us,
                    } if current_track(&track_id) => {
                        let target = mpv.get_position() + offset_us as f64 / 1_000_000.0;
                        if state.duration_secs > 0.0 && target > state.duration_secs {
                            if state.can_next {
                                mpv.playlist_next();
                            }
                        } else {
                            mpv.seek_absolute(target.max(0.0));
                        }
                    }
                    MprisCommand::SetPosition {
                        track_id,
                        position_us,
                    } if current_track(&track_id) && position_us >= 0 => {
                        let target = position_us as f64 / 1_000_000.0;
                        if state.duration_secs <= 0.0 || target <= state.duration_secs {
                            mpv.seek_absolute(target);
                        }
                    }
                    MprisCommand::SetVolume(volume) if volume.is_finite() => {
                        let volume_max =
                            f64::from(self.settings.get_int(keys::VOLUME_MAX)).max(100.0);
                        mpv.set_volume((volume * 100.0).clamp(0.0, volume_max));
                    }
                    MprisCommand::SetRate(rate)
                        if rate.is_finite() && (0.25..=4.0).contains(&rate) =>
                    {
                        mpv.set_speed(rate);
                    }
                    MprisCommand::SetLoopStatus(status) => {
                        let mode = match status.as_str() {
                            "Track" => "file",
                            "Playlist" => "playlist",
                            _ => "no",
                        };
                        let _ = self.settings.put_string(keys::LOOP_MODE, mode);
                        mpv.set_loop_mode(mode);
                    }
                    MprisCommand::SetShuffle(shuffle) => {
                        let _ = self.settings.put_bool(keys::PLAYLIST_SHUFFLE, shuffle);
                        mpv.playlist_shuffle(shuffle);
                    }
                    // The rest failed validation.
                    _ => {}
                }
            }
        }
    }
}
