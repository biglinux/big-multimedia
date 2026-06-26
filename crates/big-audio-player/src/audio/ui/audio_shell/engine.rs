use super::playback::volume_icon;
use super::*;

impl AudioShell {
    pub(super) fn handle_player_event(
        &mut self,
        event: PlayerEvent,
        sender: &ComponentSender<Self>,
    ) {
        match event {
            // Reason 0 = natural end-of-file → advance to the next queue track
            // (radio streams never auto-advance the music queue).
            PlayerEvent::EndFile(0) if !self.is_stream => {
                if self.queue.advance().is_some() {
                    self.play_current();
                    self.sync_queue_to_panels();
                } else {
                    self.is_playing = false;
                }
            }
            PlayerEvent::PauseChanged(paused) => {
                self.is_playing = !paused;
                self.mpris_state.paused.store(paused, Ordering::Relaxed);
                if let Some(mpris) = &self.mpris {
                    mpris.emit_playback_status(paused);
                }
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetPaused(paused));
            }
            PlayerEvent::PositionChanged(secs) => {
                *self
                    .mpris_state
                    .position
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = secs;
                if let Some(mpris) = &self.mpris {
                    mpris.update_taskbar_progress(secs, self.duration_secs);
                }
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetPosition(time::playback_time(
                        secs,
                    )));
                if self.duration_secs > 0.0 {
                    self.playback_bar.emit(AudioPlaybackBarInput::SetSeekValue(
                        secs / self.duration_secs * 100.0,
                    ));
                }
                self.feed_trim_playhead(secs);
            }
            PlayerEvent::DurationChanged(secs) => {
                self.duration_secs = secs;
                *self
                    .mpris_state
                    .duration
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = secs;
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetDuration(time::playback_time(
                        secs,
                    )));
                self.feed_trim_duration(secs);
            }
            PlayerEvent::VolumeChanged(vol) => {
                *self
                    .mpris_state
                    .volume
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = vol;
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeIndicator {
                        label: format!("{vol:.0}"),
                        icon: volume_icon(vol).to_string(),
                    });
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeValue(vol));
            }
            PlayerEvent::FileLoaded(_) => {
                if self.trim_active {
                    self.exit_trim_mode();
                }
                self.trim.emit(AudioTrimBarInput::ClearSegments);
                self.trim_ranges.clear();
                self.now_playing_gen = self.now_playing_gen.wrapping_add(1);
                self.sync_now_playing();
                self.sync_now_playing_card();
                self.fetch_lyrics_for_current(sender);
                self.push_mpris_metadata(sender);
            }
            _ => {}
        }
    }
}

/// Build the mpv audio backend, apply persisted audio settings, and spawn the
/// event loop that forwards `PlayerEvent`s into the shell. Returns `None` if the
/// engine fails to initialize (the UI still loads; playback is unavailable).
pub(super) fn build_mpv_backend(
    sender: &ComponentSender<AudioShell>,
    settings: &Settings,
) -> Option<Arc<MpvBackend>> {
    let (tx, rx) = async_channel::unbounded::<PlayerEvent>();
    let mpv = match MpvBackend::new_audio_only(tx) {
        Ok(mpv) => mpv,
        Err(e) => {
            log::error!("failed to initialize mpv audio backend: {e}");
            return None;
        }
    };

    let replaygain = settings.get_string(keys::REPLAYGAIN);
    if !replaygain.is_empty() && replaygain != "no" {
        mpv.set_replaygain(&replaygain);
    }
    let preamp = settings.get_double(keys::REPLAYGAIN_PREAMP);
    if preamp.abs() > 0.01 {
        mpv.set_replaygain_preamp(preamp);
    }
    let volume = f64::from(settings.get_int(keys::AUDIO_VOLUME));
    mpv.set_volume(if volume > 0.0 { volume } else { 100.0 });
    if settings.get_bool(keys::GAPLESS_AUDIO) {
        mpv.set_gapless(true);
    }
    if settings.get_bool(keys::AUDIO_PITCH_CORRECTION) {
        mpv.set_audio_pitch_correction(true);
    }
    mpv.set_save_position(settings.get_bool(keys::RESUME_PLAYBACK));

    let input = sender.input_sender().clone();
    glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            if input.send(AudioShellInput::Player(event)).is_err() {
                break;
            }
        }
    });

    Some(Arc::new(mpv))
}
