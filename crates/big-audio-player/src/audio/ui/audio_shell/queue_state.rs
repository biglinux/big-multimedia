use super::*;

impl AudioShell {
    pub(super) fn play_current(&mut self) { self.play_current_as(PlaybackStatus::Playing); }

    pub(super) fn play_current_as(&mut self, status: PlaybackStatus) {
        let Some(path) = self.queue.current().map(|track| track.file_path.clone()) else { return; };
        self.mpris_state.invalidate();
        self.current_file = path.clone(); self.is_stream = false;
        self.is_playing = status == PlaybackStatus::Playing;
        if status == PlaybackStatus::Stopped {
            // Select the next item without starting decoder/audio output.
            self.mpris_state.selected();
            *self.mpris_state.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
            *self.mpris_state.position.lock().unwrap_or_else(PoisonError::into_inner) = 0.0;
        } else if let Some(mpv) = &self.mpv {
            let profile = if self.podcast_mode { AudioCacheProfile::Podcast } else { AudioCacheProfile::Music };
            mpv.load_audio_paused(&path, profile, status == PlaybackStatus::Paused);
        }
        self.sync_now_playing();
    }

    pub(super) fn play_if_idle(&mut self) {
        if !self.is_playing || self.current_file.is_empty() {
            let last = self.queue.len().saturating_sub(1);
            self.queue.jump_to(last);
            self.play_current();
        }
    }

    /// Append a node's tracks to the queue, starting playback from the first new
    /// one when idle (the Music-tab activate-to-enqueue behavior).
    pub(super) fn enqueue_tracks(&mut self, tracks: Vec<Track>) {
        if tracks.is_empty() {
            return;
        }
        self.snapshot_queue();
        let start = self.queue.len();
        for track in tracks {
            self.queue.add_track(track);
        }
        if !self.is_playing || self.current_file.is_empty() {
            self.queue.jump_to(start);
            self.play_current();
        }
        self.commit_queue();
    }

    /// Record the pre-edit queue snapshot. Call before a user-initiated queue
    /// mutation (add / remove / reorder / sort / clear / dedupe).
    pub(super) fn snapshot_queue(&mut self) {
        self.history.snapshot(self.queue.tracks().to_vec());
    }

    /// Append files opened from the desktop / CLI as external queue tracks
    /// (playlists are expanded) and play from the first new entry.
    pub(super) fn open_paths(&mut self, paths: Vec<String>) {
        let mut resolved: Vec<String> = Vec::new();
        for path in &paths {
            if crate::media::playlist::is_playlist_file(path) {
                resolved.extend(crate::media::playlist::parse_playlist_file(path));
            } else {
                resolved.push(path.clone());
            }
        }
        if resolved.is_empty() {
            return;
        }
        self.snapshot_queue();
        let first_new = self.queue.len();
        for path in resolved {
            self.queue.add_track(Track::new_external_path(path));
        }
        self.queue.jump_to(first_new);
        self.play_current();
        self.commit_queue();
    }

    /// Restore the previous queue snapshot (Ctrl+Z).
    pub(super) fn undo_queue(&mut self) {
        let current = self.queue.tracks().to_vec();
        if let Some(previous) = self.history.undo(current) {
            self.queue.set_tracks(previous);
            self.commit_queue();
            self.notify(&i18n("Undo"));
        } else {
            self.notify(&i18n("Nothing to undo"));
        }
    }

    /// Re-apply the last undone queue edit (Ctrl+Shift+Z / Ctrl+Y).
    pub(super) fn redo_queue(&mut self) {
        let current = self.queue.tracks().to_vec();
        if let Some(next) = self.history.redo(current) {
            self.queue.set_tracks(next);
            self.commit_queue();
            self.notify(&i18n("Redo"));
        } else {
            self.notify(&i18n("Nothing to redo"));
        }
    }
}
