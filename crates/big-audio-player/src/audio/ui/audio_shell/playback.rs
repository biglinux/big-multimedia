use super::*;

impl AudioShell {
    pub(super) fn handle_playback(
        &mut self,
        out: AudioPlaybackBarOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            AudioPlaybackBarOutput::TogglePlay => {
                if let Some(mpv) = &self.mpv {
                    mpv.toggle_pause();
                }
            }
            AudioPlaybackBarOutput::Next => {
                if self.queue.advance().is_some() {
                    self.play_current();
                    self.sync_queue_to_panels();
                }
            }
            AudioPlaybackBarOutput::Previous => {
                if self.queue.previous().is_some() {
                    self.play_current();
                    self.sync_queue_to_panels();
                }
            }
            AudioPlaybackBarOutput::SeekChanged(pct) => {
                if self.duration_secs > 0.0 {
                    if let Some(mpv) = &self.mpv {
                        mpv.seek_absolute(pct / 100.0 * self.duration_secs);
                    }
                }
            }
            AudioPlaybackBarOutput::SegmentEdgeAdjusted { id, start, end } => {
                self.adjust_trim_segment(id, start, end);
            }
            AudioPlaybackBarOutput::VolumeChanged(vol) => {
                if let Some(mpv) = &self.mpv {
                    mpv.set_volume(vol);
                }
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeIndicator {
                        label: format!("{vol:.0}"),
                        icon: volume_icon(vol).to_string(),
                    });
            }
            AudioPlaybackBarOutput::VolumeMuteToggled => {
                if let Some(mpv) = &self.mpv {
                    mpv.toggle_mute();
                }
            }
            AudioPlaybackBarOutput::CycleRepeat => {
                let next = match self.queue.repeat() {
                    RepeatMode::None => RepeatMode::All,
                    RepeatMode::All => RepeatMode::One,
                    RepeatMode::One => RepeatMode::None,
                };
                self.queue.set_repeat(next);
                let loop_str = match next {
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
                    .emit(AudioPlaybackBarInput::SetRepeat(next));
            }
            AudioPlaybackBarOutput::ToggleShuffle(active) => {
                self.queue.set_shuffle(active);
                self.mpris_state.shuffle.store(active, Ordering::Relaxed);
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetShuffleActive(active));
            }
            // The queue lives in the always-visible right content pane now, so
            // the playlist button is a no-op (kept for transport-bar API parity).
            AudioPlaybackBarOutput::ShowPlaylist => {}
            AudioPlaybackBarOutput::ShowTrimExport => {
                sender.input(AudioShellInput::ShowTrimExport);
            }
            AudioPlaybackBarOutput::SpeedChanged(speed) => {
                if let Some(mpv) = &self.mpv {
                    mpv.set_speed(speed);
                }
            }
            AudioPlaybackBarOutput::ToggleLyrics(active) => {
                self.lyrics_visible = active;
                self.lyrics.widget().set_visible(active);
                if active {
                    let height = self.lyrics_paned.height();
                    let pos = (f64::from(height) * 0.4).max(150.0) as i32;
                    self.lyrics_paned.set_position(pos);
                    self.fetch_lyrics_for_current(sender);
                }
            }
            // Voice-enhancement panel is wired with the equalizer subsystem.
            AudioPlaybackBarOutput::VoiceChanged(pct) => {
                let active = crate::shared::popovers::handle_voice_strength_changed(
                    &self.settings,
                    self.mpv.as_deref(),
                    pct,
                    &self.voice_active,
                );
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVoiceActive(active));
            }
            AudioPlaybackBarOutput::VoiceSettingsRequested => {
                if let Some(win) = self.window() {
                    crate::shared::popovers::show_nr_settings_dialog(&self.settings, &win);
                }
            }
        }
    }

    /// Fetch lyrics for the current track off-thread (no-op while hidden), then
    /// push the plain-text result into the panel via `LyricsResolved`.
    pub(super) fn fetch_lyrics_for_current(&self, sender: &ComponentSender<Self>) {
        if !self.lyrics_visible {
            return;
        }
        let file_path = self.current_file.clone();
        if file_path.is_empty() {
            self.lyrics
                .emit(LyricsPanelInput::SetState(LyricsState::Empty));
            return;
        }
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            self.lyrics
                .emit(LyricsPanelInput::SetState(LyricsState::Empty));
            return;
        };
        self.lyrics
            .emit(LyricsPanelInput::SetState(LyricsState::Loading));
        let gen = self.now_playing_gen;
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let text = gio::spawn_blocking(move || {
                lyrics_fetch_engine::fetch_lyrics_blocking(&db, &file_path)
            })
            .await
            .ok()
            .flatten();
            let _ = input.send(AudioShellInput::LyricsResolved { gen, text });
        });
    }
}

/// Build the transport bar's translated control labels.
pub(super) fn transport_labels() -> BigAudioTransportLabels {
    BigAudioTransportLabels {
        seek: i18n("Seek position"),
        play_pause: i18n("Play / Pause"),
        previous: i18n("Previous"),
        next: i18n("Next"),
        shuffle: i18n("Shuffle"),
        repeat: i18n("Repeat"),
        volume: i18n("Volume"),
        speed: i18n("Speed"),
        voice: i18n("Voice Enhancement"),
        trim_export: i18n("Trim & Export"),
        lyrics: i18n("Lyrics"),
        playlist: i18n("Playlist"),
        status: i18n("Status"),
        album_art: i18n("Album art"),
    }
}

/// Symbolic volume icon for a 0..=max percent.
pub(super) fn volume_icon(volume: f64) -> &'static str {
    if volume <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume < 34.0 {
        "audio-volume-low-symbolic"
    } else if volume < 67.0 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}
