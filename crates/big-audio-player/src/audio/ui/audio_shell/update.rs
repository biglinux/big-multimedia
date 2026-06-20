use super::*;

impl AudioShell {
    pub(super) fn handle_shell_input(
        &mut self,
        msg: AudioShellInput,
        sender: ComponentSender<Self>,
    ) {
        match msg {
            AudioShellInput::TracksDeletedFromStorage(ids) => self.reconcile_deleted_tracks(ids),
            AudioShellInput::Library(out) => self.handle_library(out, &sender),
            AudioShellInput::Playlist(out) => self.handle_playlist(out, &sender),
            AudioShellInput::Playback(out) | AudioShellInput::PlaybackRemote(out) => {
                self.handle_playback(out, &sender);
            }
            AudioShellInput::Files(out) => self.handle_files(out, &sender),
            AudioShellInput::Radio(out) => self.handle_radio(out, &sender),
            AudioShellInput::Lyrics(LyricsPanelOutput::ExpandToggled(only_lyrics)) => {
                let pos = lyrics_paned_position(self.lyrics_paned.height(), only_lyrics);
                self.lyrics_paned.set_position(pos);
            }
            AudioShellInput::LyricsResolved { gen, text } => {
                if is_current_generation(gen, self.now_playing_gen) {
                    if lyrics_text_is_meaningful(text.as_deref()) {
                        self.lyrics
                            .emit(LyricsPanelInput::SetText(text.unwrap_or_default()));
                    } else {
                        self.lyrics
                            .emit(LyricsPanelInput::SetState(LyricsState::Empty));
                    }
                }
            }
            AudioShellInput::Podcast(out) => self.handle_podcast(out, &sender),
            AudioShellInput::PodcastFeedResolved { title, episodes } => {
                self.enter_podcast_mode(title, episodes);
            }
            AudioShellInput::ViewTabChanged(name) => self.handle_tab_changed(&name),
            AudioShellInput::RadioStationsResolved(stations) => {
                self.radio.emit(RadioPanelInput::RefreshStations(stations));
            }
            AudioShellInput::ScanFinished => {
                self.playback_bar
                    .emit(AudioPlaybackBarInput::SetStatus(i18n("Library updated")));
                self.rebuild_library_tree(&sender);
                self.refresh_manager_playlists(&sender);
            }
            AudioShellInput::Player(event) => self.handle_player_event(event, &sender),
            AudioShellInput::DroppedResolved { insert_pos, tracks } => {
                self.snapshot_queue();
                let base = drop_insert_base(insert_pos, self.queue.len());
                for (offset, track) in tracks.into_iter().enumerate() {
                    self.queue.insert_at(drop_insert_index(base, offset), track);
                }
                // When idle, start at the first inserted track (not the queue
                // tail, which `play_if_idle` would pick).
                if should_start_playback_on_drop(self.is_playing, self.current_file.is_empty()) {
                    self.queue.jump_to(base);
                    self.play_current();
                }
                self.commit_queue();
            }
            AudioShellInput::DetailResolved(tracks) => {
                self.library.emit(LibraryPanelInput::PopulateDetail(tracks));
            }
            AudioShellInput::NodeTracksResolved(tracks) => self.enqueue_tracks(tracks),
            AudioShellInput::RebuildLibraryTree => self.rebuild_library_tree(&sender),
            AudioShellInput::LibraryTreeResolved(data) => {
                self.library.emit(LibraryPanelInput::SetLibraryTree(data));
            }
            AudioShellInput::RefreshPlaylists => {
                self.refresh_manager_playlists(&sender);
            }
            AudioShellInput::AddToPlaylistResolved { track, playlists } => {
                self.library
                    .emit(LibraryPanelInput::ShowAddToPlaylistPicker { track, playlists });
            }
            AudioShellInput::TagsWritten { track_id, fields } => {
                self.apply_written_tags(track_id, &fields);
                self.rebuild_library_tree(&sender);
                self.notify(&i18n("Tags saved"));
            }
            AudioShellInput::MusicBrainzLookupResolved { track_id, metadata } => {
                self.library
                    .emit(LibraryPanelInput::MusicBrainzLookupResolved { track_id, metadata });
            }
            AudioShellInput::ImportPaths(paths) => {
                self.resolve_and_insert(self.queue.len(), paths, &sender);
            }
            AudioShellInput::OpenPaths(paths) => self.open_paths(paths),
            AudioShellInput::ShowEqualizer => {
                if let Some(win) = self.window() {
                    let dialog = crate::shared::audio_effects::create_equalizer_dialog(
                        &self.settings,
                        self.mpv.clone(),
                        None,
                    );
                    dialog.set_transient_for(Some(&win));
                    dialog.present();
                }
            }
            #[cfg(feature = "chromecast")]
            AudioShellInput::ShowCast => {
                if let Some(win) = self.window() {
                    crate::audio::ui::cast_dialog::present_cast(
                        &win,
                        self.cast_manager.clone(),
                        sender.input_sender().clone(),
                    );
                }
            }
            #[cfg(feature = "chromecast")]
            AudioShellInput::CastToDeviceIndex(idx) => self.cast_to_device(idx),
            #[cfg(feature = "chromecast")]
            AudioShellInput::CastStop => {
                self.cast_manager.stop_casting();
                self.notify(&i18n("Casting stopped"));
            }
            AudioShellInput::ShowSmartphone => {
                if let Some(win) = self.window() {
                    crate::audio::ui::smartphone_dialog::present_smartphone(
                        &win,
                        sender.input_sender().clone(),
                        self.mpris_state.clone(),
                    );
                }
            }
            AudioShellInput::RefreshBlurArt => self.refresh_blur_art(),
            AudioShellInput::ShowManagePlaylists => {
                let dialog = self.playlist_manager.widget();
                dialog.set_transient_for(self.window().as_ref());
                dialog.set_visible(true);
                dialog.present();
                self.refresh_manager_playlists(&sender);
            }
            AudioShellInput::ManagePlaylistsResolved(entries) => {
                self.playlist_manager
                    .emit(PlaylistManagerInput::SetPlaylists(entries));
            }
            AudioShellInput::PlaylistManager(out) => self.handle_playlist_manager(out, &sender),
            AudioShellInput::PlaylistTracksForQueue(tracks) => {
                if should_replace_queue_with(tracks.len()) {
                    self.snapshot_queue();
                    self.queue.set_tracks(tracks);
                    self.queue.jump_to(0);
                    self.play_current();
                    self.commit_queue();
                }
            }
            AudioShellInput::ImportPlaylistFile { path, name } => {
                self.import_playlist_file(path, name, &sender);
            }
            AudioShellInput::ImportPlaylistFolder(folder) => {
                self.import_playlist_folder_db(folder, &sender);
            }
            AudioShellInput::PlaylistImported(added) => {
                self.notify(&format!("{added} {}", i18n("tracks imported")));
                sender.input(AudioShellInput::RefreshPlaylists);
            }
            AudioShellInput::ShowSleepTimer => self.show_sleep_timer(&sender),
            AudioShellInput::SleepTimerStart { minutes, action } => {
                self.start_sleep_timer(sleep_timer_seconds(minutes), action, &sender);
            }
            AudioShellInput::SleepTimerCancel => {
                self.cancel_sleep_timer();
                self.notify(&i18n("Sleep timer cancelled"));
            }
            AudioShellInput::SleepTick => self.sleep_tick(),
            AudioShellInput::Undo => self.undo_queue(),
            AudioShellInput::Redo => self.redo_queue(),
            AudioShellInput::ToggleShuffleShortcut => {
                self.handle_playback(
                    AudioPlaybackBarOutput::ToggleShuffle(next_shuffle_state(self.queue.shuffle())),
                    &sender,
                );
            }
            AudioShellInput::MprisCommand(cmd, arg) => self.handle_mpris_command(&cmd, arg),
            AudioShellInput::ArtResolved { gen, data } => {
                if is_current_generation(gen, self.now_playing_gen) {
                    self.apply_resolved_art(data, &sender);
                }
            }
            AudioShellInput::MprisArtSaved { gen, url } => {
                if is_current_generation(gen, self.now_playing_gen) {
                    self.apply_mpris_art_url(url);
                }
            }
        }
    }
}

/// Lyrics paned split position: full height when only-lyrics is requested, else
/// ~40% of the height with a 150px floor.
fn lyrics_paned_position(height: i32, only_lyrics: bool) -> i32 {
    if only_lyrics {
        height
    } else {
        (f64::from(height) * 0.4).max(150.0) as i32
    }
}

/// Whether resolved lyrics text is worth showing (present and not blank).
fn lyrics_text_is_meaningful(text: Option<&str>) -> bool {
    text.is_some_and(|t| !t.trim().is_empty())
}

/// Clamp a drop insert position to the current queue length.
fn drop_insert_base(insert_pos: usize, queue_len: usize) -> usize {
    insert_pos.min(queue_len)
}

/// Start playback for a drop only when nothing is currently playing.
fn should_start_playback_on_drop(is_playing: bool, current_file_empty: bool) -> bool {
    !is_playing || current_file_empty
}

/// Sleep-timer countdown length converted to seconds.
fn sleep_timer_seconds(minutes: u32) -> u32 {
    minutes * 60
}

/// Guard a stale async result against the current now-playing generation.
fn is_current_generation(result_gen: u64, current_gen: u64) -> bool {
    result_gen == current_gen
}

/// Insert index for the `offset`-th track of a multi-track drop, starting at
/// the clamped `base` so the dropped run lands contiguously.
fn drop_insert_index(base: usize, offset: usize) -> usize {
    base + offset
}

/// Whether a `PlaylistTracksForQueue` message carries tracks worth replacing the
/// queue with (an empty resolution must leave the current queue untouched).
fn should_replace_queue_with(track_count: usize) -> bool {
    track_count > 0
}

/// Shuffle state after the toggle shortcut fires — the negation of the current
/// one.
fn next_shuffle_state(current_shuffle: bool) -> bool {
    !current_shuffle
}

#[cfg(test)]
mod tests {
    use super::{
        drop_insert_base, drop_insert_index, is_current_generation, lyrics_paned_position,
        lyrics_text_is_meaningful, next_shuffle_state, should_replace_queue_with,
        should_start_playback_on_drop, sleep_timer_seconds,
    };

    #[test]
    fn lyrics_position_full_height_when_only_lyrics() {
        assert_eq!(lyrics_paned_position(800, true), 800);
        assert_eq!(lyrics_paned_position(0, true), 0);
    }

    #[test]
    fn lyrics_position_is_40_percent_with_150_floor() {
        assert_eq!(lyrics_paned_position(800, false), 320); // 800 * 0.4
        assert_eq!(lyrics_paned_position(1000, false), 400);
        // Below the floor the 150px minimum wins (300 * 0.4 = 120 < 150).
        assert_eq!(lyrics_paned_position(300, false), 150);
        assert_eq!(lyrics_paned_position(0, false), 150);
    }

    #[test]
    fn lyrics_text_meaningful_only_when_present_and_non_blank() {
        assert!(lyrics_text_is_meaningful(Some("la la")));
        assert!(!lyrics_text_is_meaningful(Some("")));
        assert!(!lyrics_text_is_meaningful(Some("   \n\t ")));
        assert!(!lyrics_text_is_meaningful(None));
    }

    #[test]
    fn drop_insert_base_clamps_to_queue_len() {
        assert_eq!(drop_insert_base(0, 5), 0);
        assert_eq!(drop_insert_base(3, 5), 3);
        assert_eq!(drop_insert_base(5, 5), 5);
        assert_eq!(drop_insert_base(99, 5), 5);
        assert_eq!(drop_insert_base(1, 0), 0);
    }

    #[test]
    fn start_playback_on_drop_only_when_idle() {
        assert!(should_start_playback_on_drop(false, true));
        assert!(should_start_playback_on_drop(false, false)); // not playing
        assert!(should_start_playback_on_drop(true, true)); // no current file
        assert!(!should_start_playback_on_drop(true, false)); // actively playing
    }

    #[test]
    fn sleep_timer_seconds_is_minutes_times_60() {
        assert_eq!(sleep_timer_seconds(0), 0);
        assert_eq!(sleep_timer_seconds(1), 60);
        assert_eq!(sleep_timer_seconds(30), 1800);
    }

    #[test]
    fn current_generation_matches_only_equal_gen() {
        assert!(is_current_generation(7, 7));
        assert!(!is_current_generation(6, 7));
        assert!(!is_current_generation(8, 7));
    }

    #[test]
    fn drop_insert_index_offsets_from_base() {
        assert_eq!(drop_insert_index(3, 0), 3);
        assert_eq!(drop_insert_index(3, 1), 4);
        assert_eq!(drop_insert_index(3, 2), 5);
        assert_eq!(drop_insert_index(0, 0), 0);
    }

    #[test]
    fn replace_queue_only_with_a_non_empty_resolution() {
        assert!(!should_replace_queue_with(0));
        assert!(should_replace_queue_with(1));
        assert!(should_replace_queue_with(42));
    }

    #[test]
    fn shuffle_shortcut_negates_current_state() {
        assert!(next_shuffle_state(false));
        assert!(!next_shuffle_state(true));
    }
}
