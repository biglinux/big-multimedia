use super::playback::volume_indicator;
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
            // Radio streams never auto-advance the music queue.
            PlayerEvent::EndFile(EndReason::Eof) if !self.is_stream => {
                if self.queue.advance().is_some() {
                    self.play_current();
                    self.sync_current_to_panels();
                } else {
                    self.handle_mpris_command(MprisCommand::Stop, sender);
                }
            }
            // Skip a track that could not be opened, but stop once every
            // queued track failed in a row or the queue would come back to the
            // same one (repeat-one, a single track).
            // Every file would fail the same way, so nothing is skipped.
            PlayerEvent::EndFile(EndReason::AudioOutputFailed) => {
                self.failed_loads = 0;
                self.notify(&EndReason::audio_output_failed_notice());
                self.handle_mpris_command(MprisCommand::Stop, sender);
            }
            PlayerEvent::EndFile(EndReason::Error) if !self.is_stream => {
                self.failed_loads += 1;
                let name = std::path::Path::new(&self.current_file)
                    .file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
                self.notify(
                    &i18n("Could not play “{file}”: it may be unsupported or corrupt.")
                        .replace("{file}", &name),
                );
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
                let settings = self.settings.clone();
                self.volume_save.run(move || {
                    settings.put_audio_volume_percent(vol);
                });
                let muted = self.mpv.as_ref().is_some_and(|mpv| mpv.get_mute());
                self.playback_bar.emit(volume_indicator(vol, muted));
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeValue(vol));
            }
            PlayerEvent::MuteChanged(muted) => {
                let vol = *self
                    .mpris_state
                    .volume
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                self.playback_bar.emit(volume_indicator(vol, muted));
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
                self.mpris.emit_seeked(position);
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
/// event loop that forwards `PlayerEvent`s into the shell. When the engine
/// fails to initialize, the error is the message that tells the person why.
pub(super) fn build_mpv_backend(
    sender: &ComponentSender<AudioShell>,
    settings: &Settings,
) -> Result<Arc<MpvBackend>, String> {
    let (tx, rx) = async_channel::bounded::<PlayerEvent>(64);
    let mpv = match MpvBackend::new_audio_only_with_output(
        tx,
        &settings.get_string(keys::AUDIO_OUTPUT),
    ) {
        Ok(mpv) => mpv,
        Err(e) => {
            log::error!("failed to initialize mpv audio backend: {e}");
            // mpv accepts any audio output name at start, so only the library itself fails here.
            return Err(i18n(
                "The playback engine could not start: {error}. Update or reinstall the mpv package, then reopen the player.",
            )
            .replace("{error}", &e.to_string()));
        }
    };

    mpv.set_replaygain(&settings.get_string(keys::REPLAYGAIN));
    mpv.set_replaygain_preamp(settings.get_double(keys::REPLAYGAIN_PREAMP));
    // The volume limit goes first: `set_volume` clamps to it.
    big_media_runtime::audio_effects::apply_audio_preferences(settings, &mpv);
    mpv.set_volume(settings.audio_volume_percent());
    mpv.set_gapless(settings.get_bool(keys::GAPLESS_AUDIO));
    mpv.set_save_position(settings.get_bool(keys::RESUME_PLAYBACK));

    let input = sender.input_sender().clone();
    glib::spawn_future_local(async move {
        while let Ok(event) = rx.recv().await {
            if input.send(AudioShellInput::Player(event)).is_err() {
                break;
            }
        }
    });

    Ok(Arc::new(mpv))
}
