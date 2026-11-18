use super::*;

impl AudioShell {
    /// Keyboard seek, on the TV while it has the transport.
    pub(super) fn seek_by(&mut self, seconds: f64) {
        if self.tv.attached().is_some() {
            self.tv.seek_by(seconds);
        } else if let Some(mpv) = &self.mpv {
            mpv.seek(seconds);
        }
    }

    pub(super) fn handle_playback(
        &mut self,
        out: AudioPlaybackBarOutput,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(state) = self.tv.attached()
            && self.tv_playback(&out, state)
        {
            return;
        }
        match out {
            AudioPlaybackBarOutput::TogglePlay => {
                self.handle_mpris_command(MprisCommand::PlayPause, sender);
            }
            AudioPlaybackBarOutput::Next => {
                if self.queue.advance().is_some() {
                    self.play_current();
                    self.sync_current_to_panels();
                }
            }
            AudioPlaybackBarOutput::Previous => {
                if self.queue.previous().is_some() {
                    self.play_current();
                    self.sync_current_to_panels();
                }
            }
            AudioPlaybackBarOutput::SeekChanged(pct) => {
                if self.duration_secs > 0.0
                    && let Some(mpv) = &self.mpv
                {
                    mpv.seek_absolute(pct / 100.0 * self.duration_secs);
                }
            }
            AudioPlaybackBarOutput::SegmentEdgeAdjusted { id, start, end } => {
                self.adjust_trim_segment(id, start, end);
            }
            AudioPlaybackBarOutput::VolumeChanged(vol) => {
                if let Some(mpv) = &self.mpv {
                    self.playback_bar.emit(set_volume_unmuted(mpv, vol));
                }
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
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetRepeat(next));
            }
            AudioPlaybackBarOutput::ToggleShuffle(active) => {
                self.queue.set_shuffle(active);
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetShuffleActive(active));
            }
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
                    self.lyrics_paned
                        .set_position(super::update::lyrics_paned_position(
                            self.lyrics_paned.height(),
                            false,
                        ));
                    self.fetch_lyrics_for_current(sender);
                }
            }
            // Voice-enhancement panel is wired with the equalizer subsystem.
            AudioPlaybackBarOutput::VoiceChanged(pct) => {
                let active = crate::shared::popovers::handle_voice_strength_changed(
                    &self.settings,
                    self.mpv.as_ref(),
                    pct,
                    &self.voice_active,
                );
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVoiceActive(active));
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
        let Some(db) = self.deferred_reader() else {
            self.lyrics
                .emit(LyricsPanelInput::SetState(if self.queue_ready {
                    LyricsState::Empty
                } else {
                    LyricsState::Loading
                }));
            return;
        };
        self.lyrics
            .emit(LyricsPanelInput::SetState(LyricsState::Loading));
        let r#gen = self.now_playing_gen;
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let text = match gio::spawn_blocking(move || {
                lyrics_fetch_engine::fetch_lyrics_blocking(&db, &file_path)
            })
            .await
            {
                Ok(Ok(text)) => text,
                Ok(Err(error)) => {
                    log::warn!("lyrics lookup could not read the library: {error}");
                    let _ = input.send(AudioShellInput::Notify(i18n(
                        "The library could not be read, so lyrics were not searched online.",
                    )));
                    None
                }
                Err(_) => None,
            };
            let _ = input.send(AudioShellInput::LyricsResolved { r#gen, text });
        });
    }
}

/// Build the transport bar's translated control labels.
pub(super) fn transport_labels() -> BigAudioTransportLabels {
    BigAudioTransportLabels {
        seek: i18n("Seek position"),
        cut_timeline: i18n("Cut timeline"),
        play: i18n("Play"),
        pause: i18n("Pause"),
        previous: i18n("Previous"),
        next: i18n("Next"),
        shuffle: i18n("Shuffle"),
        repeat: i18n("Repeat"),
        volume: i18n("Volume"),
        speed: i18n("Speed"),
        voice: i18n("Voice Enhancement"),
        trim_export: i18n("Trim & Export"),
        lyrics: i18n("Lyrics"),
        status: i18n("Status"),
        album_art: i18n("Album art"),
    }
}

/// Moving the volume, by drag, keyboard or phone, asks for sound: it unmutes,
/// as the video player's slider does.
fn set_volume_unmuted(mpv: &MpvBackend, volume: f64) -> AudioPlaybackBarInput {
    mpv.set_volume(volume);
    mpv.set_mute(false);
    volume_indicator(volume, false)
}

/// The transport's volume readout and icon for `volume` percent.
pub(super) fn volume_indicator(volume: f64, muted: bool) -> AudioPlaybackBarInput {
    AudioPlaybackBarInput::SetVolumeIndicator {
        label: format!("{volume:.0}"),
        icon: volume_icon_name(volume, muted).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_change_unmutes() {
        let (tx, _rx) = async_channel::unbounded();
        let mpv = MpvBackend::new_audio_only_with_output(tx, "null").expect("mpv on null output");
        mpv.set_mute(true);
        let AudioPlaybackBarInput::SetVolumeIndicator { label, icon } =
            set_volume_unmuted(&mpv, 40.0)
        else {
            panic!("expected the volume indicator");
        };
        assert!(!mpv.get_mute());
        assert_eq!(mpv.get_volume(), 40.0);
        assert_eq!(
            (label.as_str(), icon.as_str()),
            ("40", "audio-volume-medium-symbolic")
        );
    }
}
