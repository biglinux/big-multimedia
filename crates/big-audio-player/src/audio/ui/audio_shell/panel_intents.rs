use super::*;

impl AudioShell {
    pub(super) fn handle_library(
        &mut self,
        out: LibraryPanelOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            LibraryPanelOutput::NodeSelected {
                kind,
                key,
                secondary,
                depth: _,
            } => self.resolve_node_and_enqueue(kind, key, secondary, sender),
            LibraryPanelOutput::GroupingChanged(grouping) => {
                self.settings
                    .put_string(keys::LIBRARY_GROUPING, &grouping.serialize());
                self.library_grouping = grouping;
                self.rebuild_library_tree(sender);
            }
            LibraryPanelOutput::AgeFilterChanged(index) => {
                self.library_age = index;
                self.rebuild_library_tree(sender);
            }
            LibraryPanelOutput::SearchTextChanged(text) => {
                self.library_filter = text;
                self.rebuild_library_tree(sender);
            }
            LibraryPanelOutput::PlayTrack(track) => {
                // "Play": append, then jump to it and start immediately.
                self.snapshot_queue();
                self.queue.add_track(track);
                self.queue.jump_to(self.queue.len().saturating_sub(1));
                self.play_current();
                self.commit_queue();
            }
            LibraryPanelOutput::EnqueueTrack(track) => {
                self.snapshot_queue();
                self.queue.add_track(track);
                self.play_if_idle();
                self.commit_queue();
            }
            LibraryPanelOutput::RevealInFileManager(path) => {
                big_app_kit::desktop::reveal_in_file_manager(Path::new(&path));
            }
            LibraryPanelOutput::ShowAddToPlaylist(track) => {
                self.request_add_to_playlist(track, sender);
            }
            LibraryPanelOutput::AddTrackToPlaylist {
                track_id,
                playlist_id,
            } => self.add_track_to_playlist(track_id, playlist_id, sender),
            LibraryPanelOutput::CreatePlaylistWithTrack { name, track_id } => {
                self.create_playlist_with_track(name, track_id, sender);
            }
            LibraryPanelOutput::AddWatchFolder { path } => {
                match path.to_str() {
                    Some(path) => self.scan_folder(path.to_owned(), sender),
                    None => self.notify(&i18n(
                        "This folder name cannot yet be stored in the library. No path was changed or imported.",
                    )),
                }
            }
            LibraryPanelOutput::ReviewUnavailable => {
                if let Some(parent) = self.window()
                    && let Some(db) = self.writer()
                {
                    let input = sender.input_sender().clone();
                    super::super::unavailable_dialog::show(&parent, db, move || {
                        let _ = input.send(AudioShellInput::RebuildLibraryTree);
                        let _ = input.send(AudioShellInput::RefreshPlaylists);
                    });
                }
            }
            LibraryPanelOutput::ScanNow => self.scan_all_watch_folders(sender),
            LibraryPanelOutput::RemoveWatchFolder { id } => self.remove_watch_folder(id, sender),
            LibraryPanelOutput::WriteTags {
                track_id,
                file_path,
                fields,
            } => self.write_tags(track_id, file_path, fields, sender),
            LibraryPanelOutput::ImportPlaylistFile { path } => {
                let input = sender.input_sender().clone();
                spawn_blocking_result(
                    move || crate::media::playlist::parse_playlist_file(&path),
                    move |result| match result {
                        Ok(list) => {
                            if let Some(notice) = left_out_notice(list.truncated, list.skipped) {
                                let _ = input.send(AudioShellInput::Notify(notice));
                            }
                            let _ = input.send(AudioShellInput::ImportPaths(list.entries));
                        }
                        Err(_) => {
                            let _ = input.send(AudioShellInput::Notify(i18n(
                                "The playlist could not be read safely. Check its format and permissions.",
                            )));
                        }
                    },
                );
            }
            LibraryPanelOutput::MusicBrainzLookup(track) => {
                Self::lookup_musicbrainz(track, sender);
            }
        }
    }

    /// Resolve a queue-panel intent. Pure queue mutations are handled here;
    /// playback, undo, DB, and file ops land in follow-up commits.
    pub(super) fn handle_playlist(
        &mut self,
        out: PlaylistPanelOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            PlaylistPanelOutput::JumpTo { index } => {
                self.queue.jump_to(index);
                self.play_current();
            }
            PlaylistPanelOutput::RemoveTracks { indices } => {
                self.snapshot_queue();
                // Indices arrive highest-first so earlier removals stay valid.
                for index in indices {
                    self.queue.remove(index);
                }
                self.commit_queue();
            }
            PlaylistPanelOutput::ReorderTrack { from, to } => {
                self.snapshot_queue();
                self.queue.move_track(from, to);
                self.commit_queue();
            }
            PlaylistPanelOutput::CommitSortedOrder { tracks } => {
                self.snapshot_queue();
                self.queue.set_tracks(tracks);
                self.commit_queue();
            }
            PlaylistPanelOutput::ClearQueue { .. } => {
                self.cancel_pending_opens();
                self.snapshot_queue();
                self.queue.clear();
                self.commit_queue();
            }
            PlaylistPanelOutput::RemoveDuplicates => {
                self.snapshot_queue();
                self.queue.remove_duplicates();
                self.commit_queue();
            }
            PlaylistPanelOutput::RateTrack { id, rating } => self.persist_rating(id, rating),
            PlaylistPanelOutput::SaveQueueAsPlaylist { name } => {
                self.save_queue_as_playlist(name, sender);
            }
            PlaylistPanelOutput::PersistColumnWidths(spec) => {
                self.settings
                    .put_string(keys::PLAYLIST_COLUMN_WIDTHS, &spec);
            }
            PlaylistPanelOutput::ConvertTracks(paths) => {
                if let Some(failure) =
                    converter_handoff::open(converter_handoff::AUDIO_CONVERTER, &paths, &[], None)
                {
                    self.notify(&failure);
                }
            }
            PlaylistPanelOutput::RevealInFileManager(path) => {
                big_app_kit::desktop::reveal_in_file_manager(Path::new(&path));
            }
            PlaylistPanelOutput::DeleteTracksFromStorage { items, permanent } => {
                self.snapshot_queue();
                Self::delete_tracks_from_storage(items, permanent, sender);
            }
            PlaylistPanelOutput::DropPaths { insert_pos, paths } => {
                self.resolve_and_insert(insert_pos, paths, sender);
            }
            PlaylistPanelOutput::OpenFiles(paths) => {
                self.resolve_and_insert(self.queue.len(), paths, sender);
            }
            PlaylistPanelOutput::ShowAddToPlaylistDialog(track) => {
                self.request_add_to_playlist(track, sender);
            }
            PlaylistPanelOutput::ShowTrackInfo(track) => self.show_track_metadata_dialog(&track),
            PlaylistPanelOutput::DownloadEpisodes(episodes) => {
                self.download_episodes(episodes, sender);
            }
        }
    }

    /// Persist a track rating (fire-and-forget).
    pub(super) fn persist_rating(&self, id: i64, rating: i32) {
        let Some(db) = self.writer() else {
            return;
        };
        spawn_db_query(
            db,
            move |conn| {
                if let Err(e) = queries::update_track_rating(conn, id, rating) {
                    log::warn!("rating write failed for track {id}: {e}");
                }
            },
            |()| {},
        );
    }

    /// Write edited tags to the file, then mirror them into the DB; on success,
    /// patch the in-memory queue + rebuild the tree via `TagsWritten`.
    pub(super) fn write_tags(
        &self,
        track_id: i64,
        file_path: String,
        fields: TagEditFields,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.writer() else {
            return;
        };
        let edits = crate::library::metadata::TagEdits {
            title: fields.title.clone(),
            artist: fields.artist.clone(),
            album: fields.album.clone(),
            album_artist: fields.album_artist.clone(),
            track_number: fields.track_number,
            disc_number: fields.disc_number,
            year: fields.year.clone(),
            genre: fields.genre.clone(),
        };
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let write_path = file_path.clone();
            let write = gio::spawn_blocking(move || {
                crate::library::metadata::write_track_tags(&write_path, &edits)
            })
            .await;
            match write {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    log::warn!("tag write failed for {file_path}: {e}");
                    return;
                }
                Err(e) => {
                    log::warn!("tag write task failed: {e:?}");
                    return;
                }
            }

            let db_fields = fields.clone();
            let mirrored = gio::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                update_track_metadata(
                    &conn,
                    track_id,
                    TrackMetadataUpdate {
                        title: db_fields.title.as_deref(),
                        artist: db_fields.artist.as_deref(),
                        album: db_fields.album.as_deref(),
                        album_artist: db_fields.album_artist.as_deref(),
                        track_number: db_fields.track_number_i32(),
                        disc_number: db_fields.disc_number_i32(),
                        year: db_fields.year_number(),
                        genre: db_fields.genre.as_deref(),
                    },
                )
            })
            .await;
            // The file already holds the new tags; a failed mirror leaves the DB
            // stale until the next rescan, so at least say so.
            match mirrored {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("tag DB mirror failed for track {track_id}: {e}"),
                Err(e) => log::warn!("tag DB mirror task failed: {e:?}"),
            }

            let _ = input.send(AudioShellInput::TagsWritten { track_id, fields });
        });
    }

    /// Patch any in-memory queue rows matching `track_id` with the edited fields.
    pub(super) fn apply_written_tags(&mut self, track_id: i64, fields: &TagEditFields) {
        let mut changed = false;
        for track in self.queue.tracks_mut() {
            if track.id != track_id {
                continue;
            }
            track.title.clone_from(&fields.title);
            track.artist.clone_from(&fields.artist);
            track.album.clone_from(&fields.album);
            track.album_artist.clone_from(&fields.album_artist);
            track.genre.clone_from(&fields.genre);
            track.track_number = fields.track_number_i32();
            track.disc_number = fields.disc_number_i32();
            track.year = fields.year_number();
            changed = true;
        }
        if changed {
            self.sync_queue_to_panels();
        }
    }

    /// Create a playlist from the current queue tracks.
    pub(super) fn save_queue_as_playlist(&self, name: String, sender: &ComponentSender<Self>) {
        let Some(db) = self.writer() else {
            return;
        };
        // External files and podcast episodes have no library row (id <= 0) and
        // cannot be playlist members.
        let track_ids: Vec<i64> = self
            .queue
            .tracks()
            .iter()
            .map(|t| t.id)
            .filter(|id| *id > 0)
            .collect();
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| -> big_media_runtime::Result<()> {
                // All or nothing: a half-filled playlist is worse than none.
                let transaction = conn.unchecked_transaction()?;
                let playlist_id = queries::create_playlist(&transaction, &name)?;
                for id in &track_ids {
                    queries::add_track_to_playlist(&transaction, playlist_id, *id)?;
                }
                transaction.commit()?;
                Ok(())
            },
            move |saved| {
                if let Err(e) = saved {
                    log::warn!("saving the queue as a playlist failed: {e}");
                    let _ = input.send(AudioShellInput::Notify(i18n(
                        "The playlist could not be saved.",
                    )));
                }
                // Refresh only after the write commits (avoids a stale list).
                let _ = input.send(AudioShellInput::RefreshPlaylists);
            },
        );
    }

    /// Delete tracks from disk (trash or permanent), the queue, and the DB.
    pub(super) fn delete_tracks_from_storage(
        items: Vec<(usize, Track)>,
        permanent: bool,
        sender: &ComponentSender<Self>,
    ) {
        // Delete (or trash) the files off the main loop: trashing many tracks,
        // or any track on a network mount, would otherwise freeze the UI for the
        // whole batch. Only the (id, path) pairs cross to the worker; the queue +
        // DB reconciliation happens back on the main thread, keyed by track id so
        // it stays correct even if the queue shifts during the deletes.
        let targets: Vec<(i64, String)> = items
            .into_iter()
            .map(|(_, t)| (t.id, t.file_path))
            .collect();
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || {
                let mut deleted = Vec::with_capacity(targets.len());
                for (id, path) in targets {
                    let removed = if permanent {
                        std::fs::remove_file(&path).is_ok()
                    } else {
                        big_media_runtime::media::playlist::trash_file(std::path::Path::new(&path))
                            .is_ok()
                    };
                    if removed {
                        deleted.push((id, path));
                    } else {
                        log::warn!("failed to delete from storage: {path}");
                    }
                }
                deleted
            },
            move |deleted| {
                let _ = input.send(AudioShellInput::TracksDeletedFromStorage(deleted));
            },
        );
    }

    /// Reconcile the queue + database after files were deleted off the main loop.
    pub(super) fn reconcile_deleted_tracks(
        &mut self,
        deleted: Vec<(i64, String)>,
        sender: &ComponentSender<Self>,
    ) {
        if deleted.is_empty() {
            return;
        }
        let ids = remove_deleted_files(&mut self.queue, &deleted);
        self.commit_queue();
        if ids.is_empty() {
            return;
        }
        // Delete every row in one connection lock (not N racing spawns), then
        // rebuild the library tree once the deletes commit so the removed tracks
        // leave the browse view. `spawn_db_query` runs on a pool, so ordering the
        // rebuild after the deletes requires chaining it off their completion.
        if let Some(db) = self.writer() {
            let input = sender.input_sender().clone();
            spawn_db_query(
                db,
                move |conn| {
                    for id in ids {
                        if let Err(e) = queries::delete_track(conn, id) {
                            log::warn!("deleting track {id} from the library failed: {e}");
                        }
                    }
                },
                move |()| {
                    let _ = input.send(AudioShellInput::RebuildLibraryTree);
                },
            );
        }
    }

    /// Download selected podcast episodes: a single episode prompts for a save
    /// path; multiple prompt for a destination folder. Each pick is fed to the
    /// self-driven download panel as an `Enqueue`.
    pub(super) fn download_episodes(
        &self,
        episodes: Vec<(String, String)>,
        sender: &ComponentSender<Self>,
    ) {
        let Some(window) = self.window() else {
            return;
        };
        let downloads = self.downloads.sender().clone();
        match episodes.as_slice() {
            [] => {}
            [(url, title)] => {
                let (url, title) = (url.clone(), title.clone());
                FilePicker::save_file(i18n("Save Episode"))
                    .accept_label(i18n("Save"))
                    .initial_name(filename_from_url(&url, &title))
                    .save(&window, move |dest| {
                        let _ = downloads.send(DownloadQueueInput::Enqueue { url, title, dest });
                    });
            }
            _ => {
                let input = sender.input_sender().clone();
                FilePicker::open_folder(i18n("Save Episodes")).select_folder(&window, move |dir| {
                    spawn_blocking_result(
                        move || {
                            let destinations = episode_destinations(&dir, &episodes);
                            (episodes, destinations)
                        },
                        move |(episodes, destinations)| match destinations {
                            Ok(destinations) => {
                                for ((url, title), dest) in episodes.into_iter().zip(destinations) {
                                    let _ = downloads.send(DownloadQueueInput::EnqueueNoReplace {
                                        url,
                                        title,
                                        dest,
                                    });
                                }
                            }
                            Err(e) => {
                                log::warn!("choosing episode file names failed: {e}");
                                let _ = input.send(AudioShellInput::Notify(i18n(
                                    "The episodes could not be saved in this folder. Check its permissions and try again.",
                                )));
                            }
                        },
                    );
                });
            }
        }
    }

    /// Resolve MusicBrainz metadata off-thread and push the result back into the
    /// currently open tag editor via `LibraryPanelInput`.
    pub(super) fn lookup_musicbrainz(track: Track, sender: &ComponentSender<Self>) {
        let artist = track.display_artist().to_string();
        let title = track.display_title().to_string();
        let track_id = track.id;
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || crate::library::musicbrainz::lookup_metadata(&artist, &title),
            move |metadata| {
                let _ =
                    input.send(AudioShellInput::MusicBrainzLookupResolved { track_id, metadata });
            },
        );
    }
}

/// Drops every queue entry whose file was deleted and returns the library ids to
/// forget. Entries match by path, because files opened from outside the library
/// all share id 0.
fn remove_deleted_files(queue: &mut PlayQueue, deleted: &[(i64, String)]) -> Vec<i64> {
    let is_deleted = |track: &Track| deleted.iter().any(|(_, path)| *path == track.file_path);
    while let Some(index) = queue.tracks().iter().position(is_deleted) {
        queue.remove(index);
    }
    deleted
        .iter()
        .filter(|(id, _)| *id != 0)
        .map(|(id, _)| *id)
        .collect()
}

/// One path in `dir` per episode that replaces neither an existing file nor
/// another episode of the batch, numbered ` (2)`, ` (3)` before the extension.
/// Leading dots are dropped, so a feed cannot name a hidden file.
fn episode_destinations(
    dir: &Path,
    episodes: &[(String, String)],
) -> std::io::Result<Vec<PathBuf>> {
    let mut destinations: Vec<PathBuf> = Vec::with_capacity(episodes.len());
    for (url, title) in episodes {
        let name = filename_from_url(url, title);
        let name = name.trim_start_matches('.');
        let mut dest = dir.join(name);
        let mut copy = 1;
        while destinations.contains(&dest) || std::fs::exists(&dest)? {
            copy += 1;
            let numbered = match name.rsplit_once('.') {
                Some((stem, extension)) => format!("{stem} ({copy}).{extension}"),
                None => format!("{name} ({copy})"),
            };
            dest = dir.join(numbered);
        }
        destinations.push(dest);
    }
    Ok(destinations)
}

#[cfg(test)]
mod tests {
    use super::{episode_destinations, remove_deleted_files};
    use crate::db::models::Track;
    use crate::player::queue::PlayQueue;

    fn library_track(id: i64, path: &str) -> Track {
        let mut track = Track::new_external_path(path);
        track.id = id;
        track
    }

    fn paths(queue: &PlayQueue) -> Vec<&str> {
        queue
            .tracks()
            .iter()
            .map(|t| t.file_path.as_str())
            .collect()
    }

    #[test]
    fn deleting_external_file_keeps_others() {
        let mut queue = PlayQueue::new();
        queue.add_track(Track::new_external_path("/downloads/first.mp3"));
        queue.add_track(library_track(7, "/music/library.mp3"));
        queue.add_track(Track::new_external_path("/downloads/second.mp3"));

        let ids = remove_deleted_files(&mut queue, &[(0, "/downloads/first.mp3".to_string())]);

        assert_eq!(
            paths(&queue),
            ["/music/library.mp3", "/downloads/second.mp3"]
        );
        assert_eq!(ids, Vec::<i64>::new());
    }

    #[test]
    fn deleting_library_track_drops_every_copy() {
        let mut queue = PlayQueue::new();
        queue.add_track(library_track(7, "/music/library.mp3"));
        queue.add_track(Track::new_external_path("/downloads/first.mp3"));
        queue.add_track(library_track(7, "/music/library.mp3"));

        let ids = remove_deleted_files(&mut queue, &[(7, "/music/library.mp3".to_string())]);

        assert_eq!(paths(&queue), ["/downloads/first.mp3"]);
        assert_eq!(ids, [7]);
    }

    /// File names chosen in `dir` for episodes at `urls`.
    fn destination_names(dir: &std::path::Path, urls: &[&str]) -> Vec<String> {
        let episodes: Vec<(String, String)> = urls
            .iter()
            .map(|url| ((*url).to_string(), "Title".to_string()))
            .collect();
        episode_destinations(dir, &episodes)
            .unwrap()
            .iter()
            .map(|dest| {
                dest.strip_prefix(dir)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn batch_download_never_replaces_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("show.mp3"), b"kept").unwrap();
        std::fs::write(dir.path().join("show (2).mp3"), b"kept").unwrap();

        let names = destination_names(
            dir.path(),
            &[
                "https://feed.test/a/show.mp3",
                "https://feed.test/b/show.mp3",
                "https://feed.test/other.ogg",
            ],
        );

        assert_eq!(names, ["show (3).mp3", "show (4).mp3", "other.ogg"]);
    }

    #[test]
    fn batch_download_refuses_hidden_names() {
        let dir = tempfile::tempdir().unwrap();

        let names = destination_names(
            dir.path(),
            &[
                "https://feed.test/.profile",
                "https://feed.test/..hidden.mp3",
            ],
        );

        assert_eq!(names, ["Title.mp3", "Title (2).mp3"]);
    }
}
