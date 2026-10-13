use super::playback::volume_icon;
use super::*;

impl AudioShell {
    pub(super) fn handle_player_event(
        &mut self,
        event: PlayerEvent,
        sender: &ComponentSender<Self>,
    ) {
        // While the TV has the transport it shows the TV, not the paused mpv.
        let tv = self.tv.attached().is_some();
        match event {
            // Reason 0 = natural end-of-file → advance to the next queue track
            // (radio streams never auto-advance the music queue).
            PlayerEvent::EndFile(0) if !self.is_stream => {
                if self.queue.advance().is_some() {
                    self.play_current();
                    self.sync_current_to_panels();
                } else {
                    self.handle_mpris_command(MprisCommand::Stop, sender);
                }
            }
            // Reason 4 = the track could not be opened. Skip it, but stop once
            // every queued track failed in a row or the queue would come back
            // to the same one (repeat-one, a single track).
            PlayerEvent::EndFile(4) if !self.is_stream => {
                self.failed_loads += 1;
                let name = std::path::Path::new(&self.current_file)
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                self.notify(&format!(
                    "{name}: {}",
                    i18n("Could not play this file — it may be unsupported or corrupt.")
                ));
                let failed = self.queue.current_index();
                if self.failed_loads < self.queue.len()
                    && self.queue.advance().is_some()
                    && self.queue.current_index() != failed
                {
                    self.play_current();
                    self.sync_current_to_panels();
                } else {
                    self.failed_loads = 0;
                    self.handle_mpris_command(MprisCommand::Stop, sender);
                }
            }
            PlayerEvent::PauseChanged(paused) => {
                let mut status = self
                    .mpris_state
                    .status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                // A pause-property echo after Stop does not resume playback.
                if *status != PlaybackStatus::Stopped {
                    *status = if paused {
                        PlaybackStatus::Paused
                    } else {
                        PlaybackStatus::Playing
                    };
                }
                self.is_playing = *status == PlaybackStatus::Playing;
                drop(status);
                if !tv {
                    self.playback_bar
                        .emit(AudioPlaybackBarInput::SetPaused(!self.is_playing));
                }
                self.sync_now_playing();
            }
            PlayerEvent::PositionChanged(secs) => {
                *self
                    .mpris_state
                    .position
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = secs;

                if !tv {
                    self.playback_bar.emit(AudioPlaybackBarInput::SetPosition(
                        time::playback_time(secs),
                    ));
                }
                if !tv && self.duration_secs > 0.0 {
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
                if !tv {
                    self.playback_bar.emit(AudioPlaybackBarInput::SetDuration(
                        time::playback_time(secs),
                    ));
                }
                self.feed_trim_duration(secs);
            }
            PlayerEvent::VolumeChanged(_) | PlayerEvent::MuteChanged(_) if tv => {}
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
            PlayerEvent::MuteChanged(muted) => {
                let vol = *self
                    .mpris_state
                    .volume
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeIndicator {
                        label: format!("{vol:.0}"),
                        icon: if muted {
                            "audio-volume-muted-symbolic".to_string()
                        } else {
                            volume_icon(vol).to_string()
                        },
                    });
            }
            PlayerEvent::FileLoaded(_) => {
                self.failed_loads = 0;
                self.mpris_state.selected();
                let paused = self.mpv.as_ref().is_none_or(|mpv| mpv.is_paused());
                self.is_playing = !paused;
                if !tv {
                    self.playback_bar
                        .emit(AudioPlaybackBarInput::SetPaused(paused));
                }
                *self
                    .mpris_state
                    .status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = if paused {
                    PlaybackStatus::Paused
                } else {
                    PlaybackStatus::Playing
                };
                self.mpris_state.seekable.store(
                    self.mpv.as_ref().is_some_and(|mpv| mpv.is_seekable()),
                    Ordering::Relaxed,
                );
                if self.trim_active {
                    self.exit_trim_mode();
                }
                self.trim.emit(AudioTrimBarInput::ClearSegments);
                self.trim_ranges.clear();
                self.now_playing_gen = self.now_playing_gen.wrapping_add(1);
                self.sync_now_playing();
                if !self.is_stream {
                    self.save_queue_to_db();
                }
                self.sync_now_playing_card();
                self.fetch_lyrics_for_current(sender);
                self.push_mpris_metadata(sender);
                self.tv_follow_loaded();
                self.tv_catch_up_loaded();
            }
            PlayerEvent::Seeked(position) if position.is_finite() && position >= 0.0 => {
                *self
                    .mpris_state
                    .position
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = position;
                if let Some(mpris) = &self.mpris {
                    mpris.emit_seeked((position * 1_000_000.0).min(i64::MAX as f64).round() as i64);
                }
            }
            PlayerEvent::RateChanged(rate) if rate.is_finite() && rate > 0.0 => {
                *self
                    .mpris_state
                    .rate
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = rate;
            }
            PlayerEvent::Shutdown => {
                self.mpris_state.invalidate();
                *self
                    .mpris_state
                    .status
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner) = PlaybackStatus::Stopped;
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
    let (tx, rx) = async_channel::bounded::<PlayerEvent>(64);
    let mpv = match MpvBackend::new_audio_only_with_output(
        tx,
        &settings.get_string(keys::AUDIO_OUTPUT),
    ) {
        Ok(mpv) => mpv,
        Err(e) => {
            log::error!("failed to initialize mpv audio backend: {e}");
            return None;
        }
    };

    let replaygain = settings.get_string(keys::REPLAYGAIN);
    mpv.set_replaygain(if replaygain.is_empty() {
        "no"
    } else {
        &replaygain
    });
    mpv.set_replaygain_preamp(settings.get_double(keys::REPLAYGAIN_PREAMP));
    mpv.set_volume_max(i64::from(settings.get_int_or(keys::VOLUME_MAX, 150)));
    // Zero is a deliberate user choice, not a missing setting.
    mpv.set_volume(f64::from(restored_volume(
        settings.get_int_or(keys::AUDIO_VOLUME, 100),
    )));
    mpv.set_gapless(settings.get_bool(keys::GAPLESS_AUDIO));
    mpv.set_audio_pitch_correction(settings.get_bool(keys::AUDIO_PITCH_CORRECTION));
    mpv.set_audio_channels(&settings.get_string(keys::AUDIO_CHANNELS));
    mpv.set_preferred_audio_lang(&settings.get_string(keys::PREFERRED_AUDIO_LANG));
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

fn restored_volume(saved: i32) -> i32 {
    saved.clamp(0, 250)
}

#[cfg(test)]
mod review_tests {
    use super::restored_volume;

    #[test]
    fn restored_volume_keeps_zero_and_clamps() {
        assert_eq!(restored_volume(0), 0);
        assert_eq!(restored_volume(-1), 0);
        assert_eq!(restored_volume(65), 65);
        assert_eq!(restored_volume(i32::MAX), 250);
    }
}
