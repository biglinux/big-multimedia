//! The player while a TV plays what it sent: the strip above the transport,
//! the transport showing and driving the TV, the queue following it there,
//! and "Use the player", which leaves the TV playing and gives the transport
//! back to mpv.

use super::playback::volume_icon;
use super::*;
use crate::cast::daemon::{PlaybackStart, State};
use crate::shared::tv_cast::TvMedia;

impl AudioShell {
    pub(super) fn on_tv_changed(&mut self) {
        if let Some(failure) = self.tv.take_failure() {
            self.notify(&failure);
        }
        match self.tv.playing() {
            Some(state) => {
                if self.tv.released() {
                    let other = self.tv.source().as_deref() != Some(self.current_file.as_str());
                    self.tv_card.show_released(&state, other);
                } else {
                    self.tv_card.show(Some(&state));
                    self.show_tv_in_transport(&state);
                }
                self.tv_last = Some(state);
            }
            None if self.tv.switching() => self.tv_card.show(None),
            None => {
                let released = std::mem::take(&mut self.tv_was_released);
                let last = self.tv_last.take();
                match last {
                    Some(last) if !released => {
                        // The track ended on the TV: the next one goes there.
                        let ended = last.duration > 0.0 && last.position >= last.duration - 3.0;
                        if ended && self.tv_skip(true, last.receiver_id.clone()) {
                            self.tv_card.show(None);
                            return;
                        }
                        self.tv_card.hide();
                        self.follow_tv_position(&last);
                        self.show_mpv_in_transport();
                    }
                    _ => self.tv_card.hide(),
                }
            }
        }
        self.tv_was_released = self.tv.released();
    }

    /// "Use the player" (the TV plays on, the player picks up where it is) or
    /// "Control the TV" (the transport drives the TV again, the player pauses).
    pub(super) fn toggle_tv_release(&mut self) {
        if self.tv.released() {
            if let Some(mpv) = &self.mpv {
                mpv.pause();
            }
            self.tv.attach();
            return;
        }
        let Some(state) = self.tv.attached() else {
            return;
        };
        self.follow_tv_position(&state);
        self.tv.release();
        self.show_mpv_in_transport();
        if let Some(mpv) = &self.mpv {
            mpv.play();
        }
    }

    /// "Show this one on the TV": the TV plays the player's track from where
    /// the player is, and the transport drives the TV again.
    pub(super) fn replace_tv(&mut self) {
        let Some(state) = self.tv.playing() else {
            return;
        };
        let (seconds, title) = self.mpv.as_ref().map_or((0.0, String::new()), |mpv| {
            mpv.pause();
            (
                mpv.get_position(),
                mpv.get_media_title().unwrap_or_default(),
            )
        });
        self.tv.switch_to(
            state.receiver_id.clone(),
            TvMedia {
                source: self.current_file.clone(),
                title,
                audio: true,
                start: PlaybackStart {
                    seconds,
                    paused: false,
                    volume: state.volume.unwrap_or(1.0),
                    muted: state.muted.unwrap_or(false),
                    height: 0,
                },
            },
        );
    }

    /// "Play the TV's here": play in the player the track the TV plays, at
    /// the TV's point. The TV plays on; the player stays free.
    pub(super) fn follow_tv(&mut self) {
        let Some(source) = self.tv.source() else {
            return;
        };
        let Some(index) = self
            .queue
            .tracks()
            .iter()
            .position(|track| track.file_path == source)
        else {
            self.notify(&i18n("The TV's track is no longer in the queue."));
            return;
        };
        self.tv_catch_up = true;
        self.queue.jump_to(index);
        self.play_current();
        self.sync_queue_to_panels();
    }

    /// The TV's track loaded after "Play the TV's here": catch up with it.
    pub(super) fn tv_catch_up_loaded(&mut self) {
        if !std::mem::take(&mut self.tv_catch_up) {
            return;
        }
        if let (Some(mpv), Some(state)) = (&self.mpv, self.tv.playing())
            && self.tv.source().as_deref() == Some(self.current_file.as_str())
        {
            mpv.seek_absolute(state.position);
        }
        self.on_tv_changed();
    }

    /// Transport intents while the TV has the transport. False for the ones
    /// that stay the player's (shuffle, repeat, lyrics, …).
    pub(super) fn tv_playback(&mut self, out: &AudioPlaybackBarOutput, state: State) -> bool {
        match out {
            AudioPlaybackBarOutput::TogglePlay => {
                self.tv
                    .control(if state.paused { "play" } else { "pause" }, 0.0);
            }
            AudioPlaybackBarOutput::SeekChanged(pct) => {
                if state.can_seek && state.duration > 0.0 {
                    self.tv.control("seek-to", pct / 100.0 * state.duration);
                }
            }
            AudioPlaybackBarOutput::Next => {
                self.tv_skip(true, state.receiver_id);
            }
            AudioPlaybackBarOutput::Previous => {
                self.tv_skip(false, state.receiver_id);
            }
            AudioPlaybackBarOutput::VolumeChanged(vol) => {
                self.tv.control("volume", (vol / 100.0).clamp(0.0, 1.0));
                if state.muted == Some(true) {
                    self.tv.control("unmute", 0.0);
                }
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetVolumeIndicator {
                        label: format!("{vol:.0}"),
                        icon: volume_icon(*vol).to_string(),
                    });
            }
            AudioPlaybackBarOutput::VolumeMuteToggled => {
                let muted = state.muted != Some(true);
                self.tv.control(if muted { "mute" } else { "unmute" }, 0.0);
            }
            _ => return false,
        }
        true
    }

    /// Move the queue and send the track that loads to `receiver`. False
    /// when there is nowhere to go.
    fn tv_skip(&mut self, forward: bool, receiver: String) -> bool {
        let moved = if forward {
            self.queue.advance()
        } else {
            self.queue.previous()
        };
        if moved.is_none() {
            return false;
        }
        self.tv_follow = Some(receiver);
        self.play_current_as(PlaybackStatus::Paused);
        self.sync_queue_to_panels();
        true
    }

    /// A track loaded after Next/Previous or the end of one: the TV follows.
    pub(super) fn tv_follow_loaded(&mut self) {
        let Some(receiver) = self.tv_follow.take() else {
            return;
        };
        if let Some(mpv) = &self.mpv {
            mpv.pause();
        }
        let last = self.tv_last.as_ref();
        let title = self
            .mpv
            .as_ref()
            .and_then(|mpv| mpv.get_media_title())
            .unwrap_or_default();
        self.tv.switch_to(
            receiver,
            TvMedia {
                source: self.current_file.clone(),
                title,
                audio: true,
                start: PlaybackStart {
                    seconds: 0.0,
                    paused: false,
                    volume: last.and_then(|s| s.volume).unwrap_or(1.0),
                    muted: last.and_then(|s| s.muted).unwrap_or(false),
                    height: 0,
                },
            },
        );
    }

    /// Seek the player to where the TV is, if it plays the same track.
    fn follow_tv_position(&self, state: &State) {
        if let Some(mpv) = &self.mpv
            && self.tv.source().as_deref() == Some(self.current_file.as_str())
        {
            mpv.seek_absolute(state.position);
        }
    }

    fn show_tv_in_transport(&self, state: &State) {
        let bar = self.playback_bar.sender();
        let _ = bar.send(AudioPlaybackBarInput::SetVolumeOfTv(true));
        let _ = bar.send(AudioPlaybackBarInput::SetPaused(state.paused));
        let _ = bar.send(AudioPlaybackBarInput::SetPosition(time::playback_time(
            state.position,
        )));
        let _ = bar.send(AudioPlaybackBarInput::SetDuration(time::playback_time(
            state.duration,
        )));
        if state.duration > 0.0 {
            let _ = bar.send(AudioPlaybackBarInput::SetSeekValue(
                state.position / state.duration * 100.0,
            ));
        }
        if let Some(volume) = state.volume {
            let percent = volume * 100.0;
            let _ = bar.send(AudioPlaybackBarInput::SetVolumeValue(percent));
            let _ = bar.send(AudioPlaybackBarInput::SetVolumeIndicator {
                label: format!("{percent:.0}"),
                icon: if state.muted == Some(true) {
                    "audio-volume-muted-symbolic".to_string()
                } else {
                    volume_icon(percent).to_string()
                },
            });
        }
    }

    /// Give the transport back what mpv says, after the TV had it.
    fn show_mpv_in_transport(&self) {
        let Some(mpv) = &self.mpv else {
            return;
        };
        let bar = self.playback_bar.sender();
        let _ = bar.send(AudioPlaybackBarInput::SetVolumeOfTv(false));
        let position = mpv.get_position();
        let volume = mpv.get_volume();
        let _ = bar.send(AudioPlaybackBarInput::SetPaused(mpv.is_paused()));
        let _ = bar.send(AudioPlaybackBarInput::SetPosition(time::playback_time(
            position,
        )));
        let _ = bar.send(AudioPlaybackBarInput::SetDuration(time::playback_time(
            self.duration_secs,
        )));
        if self.duration_secs > 0.0 {
            let _ = bar.send(AudioPlaybackBarInput::SetSeekValue(
                position / self.duration_secs * 100.0,
            ));
        }
        let _ = bar.send(AudioPlaybackBarInput::SetVolumeValue(volume));
        let _ = bar.send(AudioPlaybackBarInput::SetVolumeIndicator {
            label: format!("{volume:.0}"),
            icon: if mpv.get_mute() {
                "audio-volume-muted-symbolic".to_string()
            } else {
                volume_icon(volume).to_string()
            },
        });
    }
}
