//! The player while a TV plays what it sent: the card over the video, the
//! controls bar showing and driving the TV, and "Use the player", which leaves
//! the TV playing and gives the controls back to mpv.

use super::playback::seek_fraction;
use super::*;
use crate::cast::daemon::{PlaybackStart, State};
use crate::shared::tv_cast::TvMedia;

impl VideoShell {
    /// The TV session the controls drive: this player's, not released.
    pub(super) fn tv_attached(&self) -> Option<State> {
        self.tv.attached()
    }

    pub(super) fn on_tv_changed(&mut self) {
        if let Some(failure) = self.tv.take_failure() {
            self.toast_overlay.add_toast(adw::Toast::new(&failure));
        }
        match self.tv.playing() {
            Some(state) => {
                let released = self.tv.released();
                self.tv_pill.set_visible(released);
                if released {
                    self.tv_card.hide();
                    self.tv_menu.show_released(&state, !self.holds_the_tvs());
                } else {
                    self.tv_card.show(Some(&state));
                    self.show_tv_in_controls(&state);
                }
                self.tv_last = Some(state);
            }
            None if self.tv.switching() => self.tv_card.show(None),
            None => {
                self.tv_pill.set_visible(false);
                let released = std::mem::take(&mut self.tv_was_released);
                let Some(last) = self.tv_last.take() else {
                    self.tv_card.hide();
                    return;
                };
                if released {
                    self.tv_card.hide();
                    return;
                }
                // The file ended on the TV: the next one goes there too.
                let ended = last.duration > 0.0 && last.position >= last.duration - 3.0;
                if ended && self.tv_skip(true, last.receiver_id.clone()) {
                    self.tv_card.show(None);
                    return;
                }
                // Stopped while the controls had it: pick up where it was.
                self.tv_card.hide();
                self.follow_tv_position(&last);
                self.show_mpv_in_controls();
            }
        }
        self.tv_was_released = self.tv.released();
    }

    /// "Use the player": the TV plays on; the player picks up where it is.
    pub(super) fn release_tv(&mut self) {
        let Some(state) = self.tv.attached() else {
            return;
        };
        self.follow_tv_position(&state);
        self.tv.release();
        self.show_mpv_in_controls();
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            mpv.play();
        }
    }

    /// "On the TV": the controls drive the TV again, and the player pauses
    /// so the two do not play over each other.
    pub(super) fn attach_tv(&mut self) {
        self.tv_pill.popdown();
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            mpv.pause();
        }
        self.tv.attach();
    }

    /// Whether the player holds the file the TV plays.
    fn holds_the_tvs(&self) -> bool {
        let current = self
            .mpv
            .borrow()
            .as_ref()
            .and_then(|mpv| mpv.get_current_file());
        current.is_some() && current == self.tv.source()
    }

    /// "Show this one on the TV": the TV plays what the player holds, from
    /// where the player is, and the controls drive the TV again.
    pub(super) fn replace_tv(&mut self) {
        self.tv_pill.popdown();
        let (Some(mpv), Some(state)) = (self.mpv.borrow().clone(), self.tv.playing()) else {
            return;
        };
        let Some(source) = mpv.get_current_file() else {
            return;
        };
        mpv.pause();
        self.tv.switch_to(
            state.receiver_id.clone(),
            TvMedia {
                source,
                title: mpv.get_media_title().unwrap_or_default(),
                audio: false,
                start: PlaybackStart {
                    seconds: mpv.get_position(),
                    paused: false,
                    volume: state.volume.unwrap_or(1.0),
                    muted: state.muted.unwrap_or(false),
                    height: u32::try_from(self.settings.get_int(keys::TV_RESOLUTION)).unwrap_or(0),
                },
            },
        );
    }

    /// "Play the TV's here": open in the player what the TV plays, at the
    /// TV's point. The TV plays on; the player stays free.
    pub(super) fn follow_tv(&mut self) {
        self.tv_pill.popdown();
        let (Some(mpv), Some(source)) = (self.mpv.borrow().clone(), self.tv.source()) else {
            return;
        };
        self.tv_catch_up = true;
        // Within the playlist, jump to it and keep the list; otherwise open it.
        match self.playlist_paths.iter().position(|path| *path == source) {
            Some(index) => mpv.playlist_play_index(i64::try_from(index).unwrap_or(0)),
            None => super::send_to_shell(VideoShellInput::OpenFiles(vec![source])),
        }
    }

    /// The TV's file loaded after "Play the TV's here": catch up with the TV.
    pub(super) fn tv_catch_up_loaded(&mut self) {
        if !std::mem::take(&mut self.tv_catch_up) || !self.holds_the_tvs() {
            return;
        }
        let (Some(mpv), Some(state)) = (self.mpv.borrow().clone(), self.tv.playing()) else {
            return;
        };
        mpv.seek_absolute(state.position);
        mpv.play();
        self.on_tv_changed();
    }

    /// Move the playlist and send the file that loads to `receiver`. False
    /// when there is nowhere to go.
    pub(super) fn tv_skip(&mut self, forward: bool, receiver: String) -> bool {
        let Some(mpv) = self.mpv.borrow().clone() else {
            return false;
        };
        let (position, count) = (mpv.get_playlist_pos(), mpv.get_playlist_count());
        let target = if forward { position + 1 } else { position - 1 };
        if target < 0 || target >= count {
            return false;
        }
        self.tv_follow = Some(receiver);
        mpv.pause();
        if forward {
            mpv.playlist_next();
        } else {
            mpv.playlist_prev();
        }
        true
    }

    /// A file loaded after Next/Previous: the TV follows, from its start.
    pub(super) fn tv_follow_loaded(&mut self) {
        let Some(receiver) = self.tv_follow.take() else {
            return;
        };
        let Some(mpv) = self.mpv.borrow().clone() else {
            return;
        };
        mpv.pause();
        let Some(source) = mpv.get_current_file() else {
            return;
        };
        let last = self.tv_last.as_ref();
        self.tv.switch_to(
            receiver,
            TvMedia {
                source,
                title: mpv.get_media_title().unwrap_or_default(),
                audio: false,
                start: PlaybackStart {
                    seconds: 0.0,
                    paused: false,
                    volume: last.and_then(|s| s.volume).unwrap_or(1.0),
                    muted: last.and_then(|s| s.muted).unwrap_or(false),
                    height: u32::try_from(self.settings.get_int(keys::TV_RESOLUTION)).unwrap_or(0),
                },
            },
        );
    }

    /// Seek the player to where the TV is, if it plays the same file.
    fn follow_tv_position(&self, state: &State) {
        let Some(mpv) = self.mpv.borrow().clone() else {
            return;
        };
        if mpv.get_current_file().is_some() && mpv.get_current_file() == self.tv.source() {
            mpv.seek_absolute(state.position);
        }
    }

    fn show_tv_in_controls(&self, state: &State) {
        let controls = self.controls.sender();
        let _ = controls.send(VideoControlsBarInput::SetPaused(state.paused));
        let _ = controls.send(VideoControlsBarInput::SetPosition(playback_time(
            state.position,
        )));
        let _ = controls.send(VideoControlsBarInput::SetDuration(playback_time(
            state.duration,
        )));
        if let Some(fraction) = seek_fraction(state.position, state.duration) {
            let _ = controls.send(VideoControlsBarInput::SetSeekFraction(fraction));
        }
        if let Some(volume) = state.volume {
            let _ = controls.send(VideoControlsBarInput::SetVolume(volume * 100.0));
        }
        if let Some(muted) = state.muted {
            let _ = controls.send(VideoControlsBarInput::SetMuted(muted));
        }
    }

    /// Give the controls back what mpv says, after the TV had them.
    fn show_mpv_in_controls(&self) {
        let Some(mpv) = self.mpv.borrow().clone() else {
            return;
        };
        let controls = self.controls.sender();
        let position = mpv.get_position();
        let _ = controls.send(VideoControlsBarInput::SetPaused(mpv.is_paused()));
        let _ = controls.send(VideoControlsBarInput::SetPosition(playback_time(position)));
        let _ = controls.send(VideoControlsBarInput::SetDuration(playback_time(
            self.duration_secs,
        )));
        if let Some(fraction) = seek_fraction(position, self.duration_secs) {
            let _ = controls.send(VideoControlsBarInput::SetSeekFraction(fraction));
        }
        let _ = controls.send(VideoControlsBarInput::SetVolume(mpv.get_volume()));
        let _ = controls.send(VideoControlsBarInput::SetMuted(mpv.get_mute()));
    }
}
