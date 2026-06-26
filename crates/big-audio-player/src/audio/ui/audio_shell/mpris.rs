use super::*;

impl AudioShell {
    pub(super) fn handle_mpris_command(&mut self, cmd: &str, arg: Option<i64>) {
        if let Some(mpv) = &self.mpv {
            let toggle_pause = || mpv.toggle_pause();
            let play = || mpv.play();
            let pause = || mpv.pause();
            let stop = || mpv.stop();
            let set_volume = |volume| mpv.set_volume(volume);
            let controls = BigMprisPlaybackControls {
                toggle_pause: &toggle_pause,
                play: &play,
                pause: &pause,
                stop: &stop,
                set_volume: &set_volume,
            };
            if let Some(mpris) = self.mpris.as_ref() {
                let is_paused = || mpv.is_paused();
                let position = || mpv.get_position();
                let duration = || mpv.get_duration();
                let emit_playback_status = |is_paused| mpris.emit_playback_status(is_paused);
                let update_taskbar_progress = |position, duration| {
                    mpris.update_taskbar_progress(position, duration);
                };
                let hide_taskbar_progress = || mpris.hide_taskbar_progress();
                let feedback = BigMprisPlaybackFeedback {
                    is_paused: &is_paused,
                    position: &position,
                    duration: &duration,
                    emit_playback_status: &emit_playback_status,
                    update_taskbar_progress: &update_taskbar_progress,
                    hide_taskbar_progress: &hide_taskbar_progress,
                };
                if route_standard_playback_command(cmd, arg, &controls, Some(&feedback)) {
                    return;
                }
            } else if route_standard_playback_command(cmd, arg, &controls, None) {
                return;
            }
        }
        match cmd {
            "raise" => {
                if let Some(win) = self.window() {
                    win.present();
                }
            }
            "quit" => {
                if let Some(win) = self.window() {
                    win.close();
                }
            }
            "next" if self.queue.advance().is_some() => {
                self.play_current();
                self.sync_queue_to_panels();
            }
            "previous" if self.queue.previous().is_some() => {
                self.play_current();
                self.sync_queue_to_panels();
            }
            "seek" => {
                if let Some(offset_us) = arg {
                    let offset_secs = offset_us as f64 / 1_000_000.0;
                    if let Some(mpv) = &self.mpv {
                        mpv.seek(offset_secs);
                    }
                    if let Some(mpris) = &self.mpris {
                        let pos = *self
                            .mpris_state
                            .position
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        let seeked = pos + offset_secs;
                        mpris.emit_seeked((seeked * 1_000_000.0) as i64);
                        mpris.update_taskbar_progress(seeked, self.duration_secs);
                    }
                }
            }
            "set-position" => {
                if let Some(pos_us) = arg {
                    let pos_secs = pos_us as f64 / 1_000_000.0;
                    if let Some(mpv) = &self.mpv {
                        mpv.seek_absolute(pos_secs);
                    }
                    if let Some(mpris) = &self.mpris {
                        mpris.emit_seeked(pos_us);
                        mpris.update_taskbar_progress(pos_secs, self.duration_secs);
                    }
                }
            }
            "set-loop" => {
                if let Some(mode) = arg {
                    let repeat = match mode {
                        1 => RepeatMode::One,
                        2 => RepeatMode::All,
                        _ => RepeatMode::None,
                    };
                    self.queue.set_repeat(repeat);
                    let loop_str = match repeat {
                        RepeatMode::None => "None",
                        RepeatMode::One => "Track",
                        RepeatMode::All => "Playlist",
                    };
                    *self
                        .mpris_state
                        .loop_status
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = loop_str.to_string();
                    self.playback_bar
                        .emit(AudioPlaybackBarInput::SetRepeat(repeat));
                }
            }
            "set-shuffle" => {
                if let Some(val) = arg {
                    let shuffle = val != 0;
                    self.queue.set_shuffle(shuffle);
                    self.mpris_state.shuffle.store(shuffle, Ordering::Relaxed);
                    self.playback_bar
                        .emit(AudioPlaybackBarInput::SetShuffleActive(shuffle));
                }
            }
            _ => {}
        }
    }
}

/// Build the MPRIS2 D-Bus service: a command callback that forwards onto the
/// main thread via the shell input, and a state callback that reads the shared
/// snapshot. Returns `None` if the session bus is unavailable.
pub(super) fn build_mpris_service(
    sender: &ComponentSender<AudioShell>,
    state: &Arc<SharedMprisState>,
    settings: &Settings,
) -> Option<Mpris> {
    let cmd_sender = sender.input_sender().clone();
    let command_cb: CommandCallback = Box::new(move |cmd, arg| {
        let _ = cmd_sender.send(AudioShellInput::MprisCommand(cmd.to_string(), arg));
    });

    let state_for_cb = Arc::clone(state);
    let state_cb: StateCallback = Box::new(move || {
        let s = &state_for_cb;
        PlayerState {
            is_paused: s.paused.load(Ordering::Relaxed),
            position_secs: *s.position.lock().unwrap_or_else(PoisonError::into_inner),
            duration_secs: *s.duration.lock().unwrap_or_else(PoisonError::into_inner),
            volume_pct: *s.volume.lock().unwrap_or_else(PoisonError::into_inner),
            title: s
                .title
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            artist: s
                .artist
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            album: s
                .album
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            can_next: true,
            can_prev: true,
            loop_status: s
                .loop_status
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            shuffle: s.shuffle.load(Ordering::Relaxed),
            art_url: s
                .art_url
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    });

    let taskbar_enabled = settings.get_bool_or(keys::TASKBAR_PROGRESS, true);
    Mpris::new_with_name(
        "org.mpris.MediaPlayer2.BigAudioPlayer",
        crate::config::audio::APP_ID,
        command_cb,
        state_cb,
        taskbar_enabled,
    )
}
