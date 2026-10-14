use super::*;

impl AudioShell {
    pub(super) fn play_current(&mut self) {
        self.play_current_as(PlaybackStatus::Playing);
    }

    pub(super) fn play_current_as(&mut self, status: PlaybackStatus) {
        let Some(path) = self.queue.current().map(|track| track.file_path.clone()) else {
            return;
        };
        self.mpris_state.invalidate();
        self.current_file.clone_from(&path);
        self.is_stream = false;
        self.is_playing = status == PlaybackStatus::Playing;
        if status == PlaybackStatus::Stopped {
            // Select the next item without starting decoder/audio output.
            self.mpris_state.selected();
            *self
                .mpris_state
                .status
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = status;
            *self
                .mpris_state
                .position
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = 0.0;
        } else if let Some(mpv) = &self.mpv {
            let profile = if self.podcast_mode {
                AudioCacheProfile::Podcast
            } else {
                AudioCacheProfile::Music
            };
            mpv.load_audio_paused(&path, profile, status == PlaybackStatus::Paused);
        }
        self.sync_now_playing();
    }

    pub(super) fn save_queue_to_db(&self) {
        use crate::db::queries_advanced::{QueueSession, SavedQueue};
        let active = SavedQueue {
            tracks: self.queue.tracks().to_vec(),
            current: self.queue.current_index(),
        };
        let stashed = |queue: &Option<(Vec<Track>, Option<usize>)>| {
            queue
                .as_ref()
                .map(|(tracks, current)| SavedQueue {
                    tracks: tracks.clone(),
                    current: *current,
                })
                .unwrap_or_default()
        };
        let session = if self.podcast_mode {
            QueueSession {
                music: stashed(&self.saved_music_queue),
                podcasts: active,
            }
        } else {
            QueueSession {
                music: active,
                podcasts: stashed(&self.saved_podcast_queue),
            }
        };
        self.queue_persistence.replace(session);
    }

    /// Push the queue to the panels and persist it. Call after any track-set
    /// mutation (add / remove / reorder / clear / dedupe / set).
    pub(super) fn commit_queue(&self) {
        self.sync_queue_to_panels();
        self.save_queue_to_db();
    }

    /// Start playback only when nothing is currently playing (used by enqueue).
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
    pub(super) fn cancel_pending_opens(&mut self) {
        self.pending_opens.clear();
        self.open_generation = self.open_generation.wrapping_add(1);
    }

    pub(super) fn open_paths(&mut self, paths: Vec<String>, sender: &ComponentSender<Self>) {
        self.pending_opens.push_back(paths);
        if self.queue_ready && self.pending_opens.len() == 1 {
            self.resolve_next_open(sender);
        }
    }

    pub(super) fn resolve_next_open(&mut self, sender: &ComponentSender<Self>) {
        let Some(paths) = self.pending_opens.front().cloned() else {
            return;
        };
        self.open_generation = self.open_generation.wrapping_add(1);
        let generation = self.open_generation;
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || {
                let mut resolved = Vec::new();
                let mut failed = false;
                for path in paths {
                    if crate::media::playlist::is_playlist_file(&path) {
                        match crate::audio::playlist_import::parse_playlist_file_for_import(
                            Path::new(&path),
                        ) {
                            Ok(paths) => resolved.extend(paths),
                            Err(_) => failed = true,
                        }
                    } else {
                        resolved.push(path);
                    }
                }
                (resolved, failed)
            },
            move |(paths, failed)| {
                let _ = input.send(AudioShellInput::OpenPathsResolved {
                    generation,
                    paths,
                    failed,
                });
            },
        );
    }

    pub(super) fn append_open_paths(
        &mut self,
        resolved: Vec<String>,
        sender: &ComponentSender<Self>,
    ) {
        let queued: Vec<&str> = self
            .queue
            .tracks()
            .iter()
            .map(|track| track.file_path.as_str())
            .collect();
        let (added, target) = plan_open(&queued, &resolved);
        let Some(target) = target else {
            return;
        };
        if !added.is_empty() {
            self.snapshot_queue();
            for path in &added {
                self.queue.add_track(Track::new_external_path(path.clone()));
            }
        }
        // Opening the file that is already playing leaves it playing.
        let already_playing = self.queue.current_index() == Some(target)
            && self.current_file == resolved[0]
            && self.is_playing;
        if !already_playing {
            self.queue.jump_to(target);
            self.play_current();
        }
        if !added.is_empty() {
            self.commit_queue();
            Self::load_external_metadata(added, sender);
        }
    }

    /// Restore the previous queue snapshot (Ctrl+Z).
    pub(super) fn undo_queue(&mut self, sender: &ComponentSender<Self>) {
        let current = self.queue.tracks().to_vec();
        if let Some(previous) = self.history.undo(current) {
            self.restore_queue_snapshot(previous, sender);
        }
    }

    /// Re-apply the last undone queue edit (Ctrl+Shift+Z / Ctrl+Y).
    pub(super) fn redo_queue(&mut self, sender: &ComponentSender<Self>) {
        let current = self.queue.tracks().to_vec();
        if let Some(next) = self.history.redo(current) {
            self.restore_queue_snapshot(next, sender);
        }
    }

    fn restore_queue_snapshot(&mut self, tracks: Vec<Track>, sender: &ComponentSender<Self>) {
        self.cancel_pending_opens();
        let selected = tracks
            .iter()
            .position(|track| track.file_path == self.current_file);
        let status = *self
            .mpris_state
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.queue.set_tracks(tracks);
        if let Some(index) = selected {
            self.queue.jump_to(index);
        } else if !self.is_stream && !self.current_file.is_empty() {
            if self.queue.is_empty() {
                self.handle_mpris_command(MprisCommand::Stop, sender);
                self.current_file.clear();
                self.now_playing_gen = self.now_playing_gen.wrapping_add(1);
                self.sync_now_playing_card();
            } else {
                self.play_current_as(status);
            }
        }
        self.commit_queue();
    }
}

/// What opening `resolved` does to a queue holding `queued`: the paths to
/// append, each once and only if the queue lacks it, and the index to play,
/// the first opened path wherever it now sits. Opening a file twice used to
/// queue it twice.
fn plan_open(queued: &[&str], resolved: &[String]) -> (Vec<String>, Option<usize>) {
    let mut added: Vec<String> = Vec::new();
    for path in resolved {
        if !queued.contains(&path.as_str()) && !added.contains(path) {
            added.push(path.clone());
        }
    }
    let target = resolved.first().map(|first| {
        queued
            .iter()
            .position(|queued| *queued == first)
            .unwrap_or_else(|| {
                queued.len() + added.iter().position(|path| path == first).unwrap_or(0)
            })
    });
    (added, target)
}

#[cfg(test)]
mod open_tests {
    use super::plan_open;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn a_file_already_queued_plays_where_it_is() {
        let (added, target) = plan_open(&["/a.mp3", "/b.mp3"], &paths(&["/b.mp3"]));
        assert_eq!(added, Vec::<String>::new());
        assert_eq!(target, Some(1));
    }

    #[test]
    fn new_files_append_once_and_play_from_the_first() {
        let (added, target) = plan_open(
            &["/a.mp3"],
            &paths(&["/c.mp3", "/a.mp3", "/c.mp3", "/d.mp3"]),
        );
        assert_eq!(added, paths(&["/c.mp3", "/d.mp3"]));
        assert_eq!(target, Some(1));
    }

    #[test]
    fn opening_nothing_does_nothing() {
        assert_eq!(plan_open(&["/a.mp3"], &[]), (Vec::new(), None));
    }
}
