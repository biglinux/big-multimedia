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
            LibraryPanelOutput::EnqueueNext(track) => {
                self.snapshot_queue();
                self.queue.add_next(track);
                self.play_if_idle();
                self.commit_queue();
            }
            LibraryPanelOutput::PlayAllFrom { tracks, index } => {
                self.snapshot_queue();
                self.queue.set_tracks(tracks);
                self.queue.jump_to(index);
                self.play_current();
                self.commit_queue();
            }
            LibraryPanelOutput::RateTrack { id, rating } => self.persist_rating(id, rating),
            LibraryPanelOutput::RevealInFileManager(path) => {
                big_app_kit::desktop::reveal_in_file_manager(Path::new(&path));
            }
            LibraryPanelOutput::ShowAddToPlaylist(track) => {
                self.request_add_to_playlist(track, sender);
            }
            LibraryPanelOutput::AddTrackToPlaylist {
                track_id,
                playlist_id,
            } => self.add_track_to_playlist(track_id, playlist_id),
            LibraryPanelOutput::CreatePlaylistWithTrack { name, track_id } => {
                self.create_playlist_with_track(name, track_id, sender);
            }
            LibraryPanelOutput::SaveTracksAsPlaylist { name, track_ids } => {
                self.save_tracks_as_playlist_db(name, track_ids, sender);
            }
            LibraryPanelOutput::AddWatchFolder { path } => {
                self.scan_folder(path.to_string_lossy().into_owned(), sender);
            }
            LibraryPanelOutput::ScanNow => self.scan_all_watch_folders(sender),
            LibraryPanelOutput::RemoveWatchFolder { id } => self.remove_watch_folder(id, sender),
            LibraryPanelOutput::ShowTrackInfo(track) => self.show_track_info(&track),
            LibraryPanelOutput::Notify(message) => self.notify(&message),
            LibraryPanelOutput::WriteTags {
                track_id,
                file_path,
                fields,
            } => self.write_tags(track_id, file_path, fields, sender),
            LibraryPanelOutput::PersistColumnWidths(spec) => {
                self.settings.put_string(keys::LIBRARY_COLUMN_WIDTHS, &spec);
            }
            LibraryPanelOutput::ImportPlaylistFile { path } => {
                let input = sender.input_sender().clone();
                spawn_blocking_result(
                    move || playlist_import::parse_playlist_file_for_import(&path),
                    move |paths| {
                        if !paths.is_empty() {
                            let _ = input.send(AudioShellInput::ImportPaths(paths));
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
            PlaylistPanelOutput::ConvertTracks(paths) => converter_launcher::launch(&paths),
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
            PlaylistPanelOutput::ShowTrackInfo(track) => self.show_track_info(&track),
            PlaylistPanelOutput::DownloadEpisodes(episodes) => {
                self.download_episodes(episodes);
            }
        }
    }

    /// Persist a track rating (fire-and-forget).
    pub(super) fn persist_rating(&self, id: i64, rating: i32) {
        let Some(db) = self.db.clone() else {
            return;
        };
        spawn_db_query(
            db,
            move |conn| {
                let _ = queries::update_track_rating(conn, id, rating);
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
        let Some(db) = self.db.clone() else {
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

            let year = fields.year.as_deref().and_then(|s| s.parse::<i32>().ok());
            let db_fields = fields.clone();
            let _ = gio::spawn_blocking(move || {
                let conn = db.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                update_track_metadata(
                    &conn,
                    track_id,
                    TrackMetadataUpdate {
                        title: db_fields.title.as_deref(),
                        artist: db_fields.artist.as_deref(),
                        album: db_fields.album.as_deref(),
                        album_artist: db_fields.album_artist.as_deref(),
                        track_number: db_fields.track_number.map(|n| n as i32),
                        disc_number: db_fields.disc_number.map(|n| n as i32),
                        year,
                        genre: db_fields.genre.as_deref(),
                    },
                )
            })
            .await;

            let _ = input.send(AudioShellInput::TagsWritten { track_id, fields });
        });
    }

    /// Patch any in-memory queue rows matching `track_id` with the edited fields.
    pub(super) fn apply_written_tags(&mut self, track_id: i64, fields: &TagEditFields) {
        let year = fields.year.as_deref().and_then(|s| s.parse::<i32>().ok());
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
            track.track_number = fields.track_number.map(|n| n as i32);
            track.disc_number = fields.disc_number.map(|n| n as i32);
            track.year = year;
            changed = true;
        }
        if changed {
            self.sync_queue_to_panels();
        }
    }

    /// Create a playlist from the current queue tracks.
    pub(super) fn save_queue_as_playlist(&self, name: String, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let track_ids: Vec<i64> = self.queue.tracks().iter().map(|t| t.id).collect();
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                if let Ok(playlist_id) = queries::create_playlist(conn, &name) {
                    for id in &track_ids {
                        let _ = queries::add_track_to_playlist(conn, playlist_id, *id);
                    }
                }
            },
            move |()| {
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
        // Delete (or trash) the files off the main loop — trashing many tracks,
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
                let mut deleted_ids = Vec::with_capacity(targets.len());
                for (id, path) in targets {
                    let removed = if permanent {
                        std::fs::remove_file(&path).is_ok()
                    } else {
                        gio::File::for_path(&path)
                            .trash(gio::Cancellable::NONE)
                            .is_ok()
                    };
                    if removed {
                        deleted_ids.push(id);
                    } else {
                        log::warn!("failed to delete from storage: {path}");
                    }
                }
                deleted_ids
            },
            move |deleted_ids| {
                let _ = input.send(AudioShellInput::TracksDeletedFromStorage(deleted_ids));
            },
        );
    }

    /// Reconcile the queue + database after files were deleted off the main loop.
    /// Each track is located in the queue by **id** (not a stale index) so a
    /// queue change during the deletes can't drop the wrong entry.
    pub(super) fn reconcile_deleted_tracks(&mut self, ids: Vec<i64>) {
        if ids.is_empty() {
            return;
        }
        for id in ids {
            if let Some(index) = self.queue.tracks().iter().position(|t| t.id == id) {
                self.queue.remove(index);
            }
            if let Some(db) = self.db.clone() {
                spawn_db_query(
                    db,
                    move |conn| {
                        let _ = queries::delete_track(conn, id);
                    },
                    |()| {},
                );
            }
        }
        self.commit_queue();
        // TODO R6d: rebuild the library tree (deleted tracks left the library).
    }

    /// Download selected podcast episodes: a single episode prompts for a save
    /// path; multiple prompt for a destination folder. Each pick is fed to the
    /// self-driven download panel as an `Enqueue`.
    pub(super) fn download_episodes(&self, episodes: Vec<(String, String)>) {
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
                        let _ = downloads.send(DownloadManagerInput::Enqueue { url, title, dest });
                    });
            }
            _ => {
                FilePicker::open_folder(i18n("Save Episodes")).select_folder(&window, move |dir| {
                    for (url, title) in &episodes {
                        let dest = dir.join(filename_from_url(url, title));
                        let _ = downloads.send(DownloadManagerInput::Enqueue {
                            url: url.clone(),
                            title: title.clone(),
                            dest,
                        });
                    }
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
