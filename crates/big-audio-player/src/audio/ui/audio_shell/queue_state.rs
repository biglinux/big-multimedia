use super::*;

impl AudioShell {
    /// Show the saved queue unless the person already edited this one, and on
    /// the first open run the library reads that waited for the database.
    pub(super) fn restore_queue(
        &mut self,
        restored: queue_persistence::RestoredQueue,
        sender: &ComponentSender<Self>,
    ) {
        if let Some(session) = &restored.session
            && self.queue_persistence.status().revision == 0
        {
            self.queue.set_tracks(session.music.tracks.clone());
            if let Some(index) = session.music.current {
                self.queue.jump_to(index);
            }
            self.saved_podcast_queue =
                Some((session.podcasts.tracks.clone(), session.podcasts.current));
            self.sync_queue_to_panels();
            let paths = self
                .queue
                .tracks()
                .iter()
                .filter(|track| track.id == 0)
                .map(|track| track.file_path.clone())
                .collect();
            Self::load_external_metadata(paths, sender);
        }
        if self.db.is_none() {
            self.db = Some(restored.writer.clone());
            self.db_read = restored.reader;
            self.rebuild_library_tree(sender);
            self.refresh_playlist_dialog_entries(sender);
            self.load_radio_stations(sender);
            self.fetch_lyrics_for_current(sender);
        }
        self.queue_persistence
            .loaded(restored.writer, restored.session.is_some());
    }

    pub(super) fn play_current(&mut self) {
        self.play_current_as(PlaybackStatus::Playing);
    }

    pub(super) fn play_current_as(&mut self, status: PlaybackStatus) {
        if status != PlaybackStatus::Stopped && self.report_missing_engine() {
            return;
        }
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
            // The music queue keeps entries of imported playlists, saved
            // queues included, so every queue load passes the list check.
            let (profile, origin) = if self.podcast_mode {
                (AudioCacheProfile::Podcast, ListOrigin::Network)
            } else {
                (AudioCacheProfile::Music, ListOrigin::LocalFile)
            };
            mpv.load_audio_paused(
                &path,
                TargetSource::List(origin),
                profile,
                status == PlaybackStatus::Paused,
            );
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
                let mut truncated = false;
                let mut skipped = 0;
                for path in paths {
                    if crate::media::playlist::is_playlist_file(&path) {
                        match crate::media::playlist::parse_playlist_file(Path::new(&path)) {
                            Ok(list) => {
                                truncated |= list.truncated;
                                skipped += list.skipped;
                                resolved.extend(list.entries);
                            }
                            Err(_) => failed = true,
                        }
                    } else {
                        resolved.push(path);
                    }
                }
                (resolved, failed, truncated, skipped)
            },
            move |(paths, failed, truncated, skipped)| {
                let _ = input.send(AudioShellInput::OpenPathsResolved {
                    generation,
                    paths,
                    failed,
                    truncated,
                    skipped,
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

/// A queue set aside while the other one plays, with its playing index.
type StashedQueue = (Vec<Track>, Option<usize>);

fn stash(queue: &PlayQueue) -> StashedQueue {
    (queue.tracks().to_vec(), queue.current_index())
}

/// The feed's episodes as tracks numbered from 1, with ids counting down from
/// -1 so none matches a library track, whose database ids are positive.
pub(super) fn podcast_episode_tracks(
    feed_title: &str,
    episodes: Vec<(String, String)>,
) -> Vec<Track> {
    episodes
        .into_iter()
        .zip(1..)
        .map(|((audio_url, episode_title), number)| {
            Track::new_podcast_episode(
                -i64::from(number),
                audio_url,
                episode_title,
                feed_title,
                number,
            )
        })
        .collect()
}

/// Plays `episodes` from the first one, stashing the music queue unless
/// podcasts already play.
pub(super) fn start_podcast_queue(
    podcast_mode: &mut bool,
    queue: &mut PlayQueue,
    saved_music: &mut Option<StashedQueue>,
    episodes: Vec<Track>,
) {
    if !*podcast_mode {
        *saved_music = Some(stash(queue));
    }
    *podcast_mode = true;
    queue.set_tracks(episodes);
    queue.jump_to(0);
}

/// Swaps the playing queue with the stashed one when the `tab` switch crosses
/// between music and podcasts; true when `queue` changed.
pub(super) fn swap_queue_for_tab(
    tab: &str,
    podcast_mode: &mut bool,
    queue: &mut PlayQueue,
    saved_music: &mut Option<StashedQueue>,
    saved_podcast: &mut Option<StashedQueue>,
) -> bool {
    let (tracks, current) = if tab == "podcast" {
        if *podcast_mode {
            return false;
        }
        let Some(podcasts) = saved_podcast.take() else {
            return false;
        };
        *saved_music = Some(stash(queue));
        *podcast_mode = true;
        podcasts
    } else {
        if !*podcast_mode {
            return false;
        }
        *saved_podcast = Some(stash(queue));
        *podcast_mode = false;
        let Some(music) = saved_music.take() else {
            return false;
        };
        music
    };
    queue.set_tracks(tracks);
    if let Some(index) = current {
        queue.jump_to(index);
    }
    true
}

/// What the transport holds once a tab switch put the other queue on screen.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum LoadedAfterSwap {
    /// The visible queue's current entry is the loaded file.
    Keep,
    /// Unload, then select the visible queue's current entry without playing it.
    Select,
    /// Unload; the visible queue is empty.
    Clear,
}

/// The transport follows the visible queue: the player keeps `loaded` (empty
/// when nothing is) only while `queue` shows it as current.
pub(super) fn loaded_after_swap(queue: &PlayQueue, loaded: &str) -> LoadedAfterSwap {
    match queue.current() {
        Some(track) if track.file_path == loaded => LoadedAfterSwap::Keep,
        Some(_) => LoadedAfterSwap::Select,
        None => LoadedAfterSwap::Clear,
    }
}

#[cfg(test)]
mod podcast_queue_tests {
    use super::*;

    fn titles(queue: &PlayQueue) -> Vec<&str> {
        queue
            .tracks()
            .iter()
            .map(|track| track.title.as_deref().unwrap_or_default())
            .collect()
    }

    /// Songs A, B and C with C playing.
    fn music_queue() -> PlayQueue {
        let mut queue = PlayQueue::new();
        for title in ["Song A", "Song B", "Song C"] {
            let mut track = Track::new_external_path(format!("/music/{title}.mp3"));
            track.title = Some(title.to_string());
            queue.add_track(track);
        }
        queue.jump_to(2);
        queue
    }

    fn episodes(count: usize) -> Vec<(String, String)> {
        (0..count)
            .map(|index| {
                (
                    format!("https://feed/{index}.mp3"),
                    format!("Episode {index}"),
                )
            })
            .collect()
    }

    #[test]
    fn episode_ids_negative_and_unique() {
        let tracks = podcast_episode_tracks("Feed", episodes(50));
        let mut ids: Vec<i64> = tracks.iter().map(|track| track.id).collect();
        assert!(ids.iter().all(|id| *id < 0), "{ids:?}");
        let numbers: Vec<Option<i32>> = tracks.iter().map(|track| track.track_number).collect();
        assert_eq!(numbers, (1..=50).map(Some).collect::<Vec<_>>());
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 50);
    }

    #[test]
    fn tab_switches_restore_each_queue() {
        let mut queue = music_queue();
        let (mut podcast_mode, mut saved_music, mut saved_podcast) = (false, None, None);

        start_podcast_queue(
            &mut podcast_mode,
            &mut queue,
            &mut saved_music,
            podcast_episode_tracks("Feed", episodes(3)),
        );
        queue.jump_to(1);

        let mut switch = |tab: &str, queue: &mut PlayQueue| {
            swap_queue_for_tab(
                tab,
                &mut podcast_mode,
                queue,
                &mut saved_music,
                &mut saved_podcast,
            )
        };
        assert!(switch("music", &mut queue));
        assert_eq!(titles(&queue), ["Song A", "Song B", "Song C"]);
        assert_eq!(queue.current_index(), Some(2));
        assert!(!switch("radio", &mut queue));

        assert!(switch("podcast", &mut queue));
        assert_eq!(titles(&queue), ["Episode 0", "Episode 1", "Episode 2"]);
        assert_eq!(queue.current_index(), Some(1));
        assert!(!switch("podcast", &mut queue));

        assert!(switch("music", &mut queue));
        assert_eq!(titles(&queue), ["Song A", "Song B", "Song C"]);
        assert_eq!(queue.current_index(), Some(2));
    }

    /// A restored session stashes the podcast queue even when it is empty.
    #[test]
    fn transport_follows_the_queue_each_tab_shows() {
        let mut queue = music_queue();
        let song = "/music/Song C.mp3";
        let (mut podcast_mode, mut saved_music) = (false, None);
        let mut saved_podcast = Some((Vec::new(), None));
        let mut switch = |tab: &str, queue: &mut PlayQueue| {
            swap_queue_for_tab(
                tab,
                &mut podcast_mode,
                queue,
                &mut saved_music,
                &mut saved_podcast,
            )
        };

        assert!(switch("podcast", &mut queue));
        assert!(queue.is_empty());
        assert_eq!(loaded_after_swap(&queue, song), LoadedAfterSwap::Clear);

        assert!(switch("music", &mut queue));
        assert_eq!(titles(&queue), ["Song A", "Song B", "Song C"]);
        assert_eq!(loaded_after_swap(&queue, ""), LoadedAfterSwap::Select);
        assert_eq!(loaded_after_swap(&queue, song), LoadedAfterSwap::Keep);

        queue.set_tracks(podcast_episode_tracks("Feed", episodes(2)));
        assert_eq!(loaded_after_swap(&queue, song), LoadedAfterSwap::Select);
    }
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
