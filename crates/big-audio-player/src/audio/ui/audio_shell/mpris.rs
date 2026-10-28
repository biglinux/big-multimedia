use super::*;

impl AudioShell {
    pub(super) fn handle_mpris_command(
        &mut self,
        command: MprisCommand,
        sender: &ComponentSender<Self>,
    ) {
        // Revalidate track-bound commands here, after the D-Bus -> GTK hop.
        let state = self
            .mpris_state
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let current_id = self
            .mpris_state
            .track_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        match command {
            MprisCommand::Raise => {
                if let Some(window) = self.window() {
                    window.present();
                }
            }
            MprisCommand::Quit => {
                // A quit asked for ends the player; a plain close only hides
                // it to the tray.
                super::tray::mark_quitting();
                if let Some(window) = self.window() {
                    window.close();
                }
            }
            MprisCommand::Next | MprisCommand::Previous => {
                if (matches!(command, MprisCommand::Next) && !state.can_next)
                    || (matches!(command, MprisCommand::Previous) && !state.can_prev)
                {
                    return;
                }
                let status = state.status;
                let moved = if matches!(command, MprisCommand::Previous) {
                    self.queue.previous().is_some()
                } else {
                    // Repeat-one applies to EOF, not an explicit Next request.
                    let repeat = self.queue.repeat();
                    if repeat == RepeatMode::One {
                        self.queue.set_repeat(RepeatMode::None);
                    }
                    let moved = self.queue.advance().is_some();
                    self.queue.set_repeat(repeat);
                    moved
                };
                if moved {
                    self.play_current_as(status);
                    self.sync_current_to_panels();
                }
            }
            MprisCommand::Play | MprisCommand::PlayPause => {
                if state.status == PlaybackStatus::Stopped {
                    if self.is_stream && !self.current_file.is_empty() {
                        self.mpris_state.invalidate();
                        if let Some(mpv) = &self.mpv {
                            mpv.load_file_with_audio_profile(
                                &self.current_file,
                                AudioCacheProfile::Radio,
                            );
                        }
                    } else {
                        self.play_current();
                    }
                } else if let Some(mpv) = &self.mpv {
                    if matches!(command, MprisCommand::PlayPause) {
                        mpv.toggle_pause();
                    } else {
                        mpv.play();
                    }
                }
            }
            MprisCommand::Pause => {
                if let Some(mpv) = &self.mpv {
                    mpv.pause();
                }
            }
            MprisCommand::Stop => {
                self.is_playing = false;
                self.sync_now_playing();
                self.duration_secs = 0.0;
                self.mpris_state.invalidate();
                *self
                    .mpris_state
                    .status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = PlaybackStatus::Stopped;
                *self
                    .mpris_state
                    .position
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = 0.0;
                *self
                    .mpris_state
                    .duration
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = 0.0;
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetPaused(true));
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetPosition("0:00".into()));
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetDuration("0:00".into()));
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetSeekValue(0.0));
                if let Some(mpv) = &self.mpv {
                    mpv.stop();
                }
            }
            MprisCommand::Seek {
                track_id,
                offset_us,
            } => {
                if current_id.as_deref() != Some(track_id.as_str()) || !state.can_seek {
                    return;
                }
                let target = state.position_secs + offset_us as f64 / 1_000_000.0;
                if state.duration_secs > 0.0 && target > state.duration_secs {
                    if state.can_next {
                        self.handle_mpris_command(MprisCommand::Next, sender);
                    }
                } else if let Some(mpv) = &self.mpv {
                    mpv.seek_absolute_async(target.max(0.0));
                }
            }
            MprisCommand::SetPosition {
                track_id,
                position_us,
            } => {
                if current_id.as_deref() != Some(track_id.as_str())
                    || !state.can_seek
                    || position_us < 0
                {
                    return;
                }
                let target = position_us as f64 / 1_000_000.0;
                if state.duration_secs > 0.0 && target > state.duration_secs {
                    return;
                }
                if let Some(mpv) = &self.mpv {
                    mpv.seek_absolute_async(target);
                }
            }
            MprisCommand::OpenUri(uri) => {
                self.cancel_pending_opens();
                let file = gio::File::for_uri(&uri);
                let path = file
                    .path()
                    .and_then(|p| p.to_str().map(str::to_owned))
                    .unwrap_or(uri);
                self.snapshot_queue();
                self.podcast_mode = false;
                self.queue
                    .set_tracks(vec![Track::new_external_path(path.clone())]);
                self.play_current();
                self.commit_queue();
                Self::load_external_metadata(vec![path], sender);
            }
            MprisCommand::SetVolume(volume) => {
                if volume.is_finite()
                    && let Some(mpv) = &self.mpv
                {
                    mpv.set_volume((volume * 100.0).clamp(0.0, 250.0));
                }
            }
            MprisCommand::SetRate(rate) => {
                if rate.is_finite()
                    && (0.25..=4.0).contains(&rate)
                    && let Some(mpv) = &self.mpv
                {
                    mpv.set_speed(rate);
                }
            }
            MprisCommand::SetLoopStatus(mode) => {
                self.queue.set_repeat(match mode.as_str() {
                    "Track" => RepeatMode::One,
                    "Playlist" => RepeatMode::All,
                    _ => RepeatMode::None,
                });
                *self
                    .mpris_state
                    .loop_status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = mode;
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetRepeat(self.queue.repeat()));
            }
            MprisCommand::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetShuffleActive(shuffle));
            }
        }
    }

    /// Publish atomically after a whole GTK message, never partially from a bus
    /// callback. Data acquisition is memory-only; no native call or file I/O.
    pub(super) fn publish_mpris_snapshot(&self) {
        let s = &self.mpris_state;
        let status = *s.status.lock().unwrap_or_else(PoisonError::into_inner);
        let track_id = s
            .track_id
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let present = track_id.is_some();
        let operational = self.mpv.is_some();
        let can_next = !self.is_stream
            && self.queue.len() > 1
            && (self.queue.shuffle()
                || self.queue.repeat() == RepeatMode::All
                || self
                    .queue
                    .current_index()
                    .is_some_and(|i| i + 1 < self.queue.len()));
        let (title, artist, album) = if self.is_stream {
            (
                self.radio_station_name.clone(),
                i18n("Radio"),
                String::new(),
            )
        } else if let Some(track) = self.queue.current() {
            (
                track.display_title().to_owned(),
                track.display_artist().to_owned(),
                track.album.clone().unwrap_or_default(),
            )
        } else {
            (String::new(), String::new(), String::new())
        };
        let snapshot = PlayerState {
            status,
            track_id,
            position_secs: finite(*s.position.lock().unwrap_or_else(PoisonError::into_inner)),
            duration_secs: finite(*s.duration.lock().unwrap_or_else(PoisonError::into_inner)),
            volume_pct: finite(*s.volume.lock().unwrap_or_else(PoisonError::into_inner)),
            rate: *s.rate.lock().unwrap_or_else(PoisonError::into_inner),
            title,
            artist,
            album,
            art_url: s
                .art_url
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            loop_status: s
                .loop_status
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            shuffle: self.queue.shuffle(),
            can_next: operational && can_next,
            can_prev: operational && !self.is_stream && self.queue.len() > 1,
            can_play: operational
                && (present
                    || !self.queue.is_empty()
                    || (self.is_stream && !self.current_file.is_empty())),
            can_pause: operational && present,
            can_seek: operational && present && s.seekable.load(Ordering::Relaxed),
        };
        *s.snapshot.lock().unwrap_or_else(PoisonError::into_inner) = snapshot;
        if let Some(mpris) = &self.mpris {
            mpris.sync();
        }
    }
}
fn finite(x: f64) -> f64 {
    if x.is_finite() { x.max(0.0) } else { 0.0 }
}

pub(super) fn build_mpris_service(
    sender: &ComponentSender<AudioShell>,
    state: &Arc<SharedMprisState>,
    settings: &Settings,
) -> Option<Mpris> {
    let input = sender.input_sender().clone();
    let command_cb: CommandCallback = Box::new(move |command| {
        let _ = input.send(AudioShellInput::MprisCommand(command));
    });
    let state = state.clone();
    let state_cb: StateCallback = Box::new(move || {
        state
            .snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    });
    Mpris::new_with_name(
        "org.mpris.MediaPlayer2.BigAudioPlayer",
        crate::config::audio::APP_ID,
        command_cb,
        state_cb,
        settings.get_bool_or(keys::TASKBAR_PROGRESS, true),
    )
}
