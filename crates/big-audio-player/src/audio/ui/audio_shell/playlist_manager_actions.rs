use super::*;

impl AudioShell {
    pub(super) fn refresh_manager_playlists(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                queries::get_all_playlists(conn)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|playlist| {
                        let (count, dur) =
                            queries::get_playlist_stats(conn, playlist.id).unwrap_or((0, 0.0));
                        (playlist.id, playlist.name, count, dur)
                    })
                    .collect::<Vec<_>>()
            },
            move |entries| {
                let _ = input.send(AudioShellInput::ManagePlaylistsResolved(entries));
            },
        );
    }

    /// Resolve a playlist's tracks → replace the queue and play
    /// (`PlaylistTracksForQueue`). This is the manager's "Load" action.
    pub(super) fn load_playlist_to_queue(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| queries::get_playlist_tracks(conn, id).unwrap_or_default(),
            move |tracks| {
                let _ = input.send(AudioShellInput::PlaylistTracksForQueue(tracks));
            },
        );
    }

    /// Dispatch a playlist-manager action: save current queue, load, rename,
    /// delete, export, or import. Mutations re-resolve the manager list.
    pub(super) fn handle_playlist_manager(
        &mut self,
        out: PlaylistManagerOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            // Mutations refresh the manager via their completion → RefreshPlaylists
            // (not immediately, which would read before the write commits).
            PlaylistManagerOutput::SaveCurrent { name } => {
                self.save_queue_as_playlist(name, sender);
            }
            PlaylistManagerOutput::Load(id) => self.load_playlist_to_queue(id, sender),
            PlaylistManagerOutput::Rename { id, name } => {
                self.rename_playlist_db(id, name, sender);
            }
            PlaylistManagerOutput::Delete(id) => self.delete_playlist(id, sender),
            PlaylistManagerOutput::Export(id) => self.export_playlist(id),
            PlaylistManagerOutput::Import => self.import_playlist(sender),
            PlaylistManagerOutput::ImportFolder => self.import_playlist_folder(sender),
            PlaylistManagerOutput::ExportAll => self.export_all_playlists(),
            PlaylistManagerOutput::GenerateLoad(kind) => {
                self.generate_smart_to_queue(&kind, sender);
            }
        }
    }

    /// Load a smart-generator preset's tracks into the queue and play.
    pub(super) fn generate_smart_to_queue(&self, kind: &str, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let kind = kind.to_string();
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| match kind.as_str() {
                "most-played" => queries::get_most_played(conn, 100).unwrap_or_default(),
                "recently-added" => queries::get_recently_added(conn, 100).unwrap_or_default(),
                "never-played" => queries::get_never_played(conn, 100).unwrap_or_default(),
                _ => Vec::new(),
            },
            move |tracks| {
                let _ = input.send(AudioShellInput::PlaylistTracksForQueue(tracks));
            },
        );
    }

    /// Import every `.m3u`/`.m3u8`/`.pls` file in a chosen folder, each as a new
    /// playlist named after its file. Reports the total tracks matched.
    pub(super) fn import_playlist_folder(&self, sender: &ComponentSender<Self>) {
        let input = sender.input_sender().clone();
        FilePicker::open_folder(i18n("Import Playlists Folder")).select_folder_optional(
            self.window().as_ref(),
            move |folder| {
                let _ = input.send(AudioShellInput::ImportPlaylistFolder(folder));
            },
        );
    }

    /// Parse + save every playlist file in `folder` (off-thread).
    pub(super) fn import_playlist_folder_db(
        &self,
        folder: PathBuf,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                let Ok(entries) = std::fs::read_dir(&folder) else {
                    return 0_usize;
                };
                let mut total = 0_usize;
                for entry in entries.flatten() {
                    let path = entry.path();
                    let is_playlist = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        matches!(e.to_ascii_lowercase().as_str(), "m3u" | "m3u8" | "pls")
                    });
                    if !is_playlist {
                        continue;
                    }
                    let ids: Vec<i64> = playlist_import::parse_playlist_file_for_import(&path)
                        .iter()
                        .filter_map(|p| queries::get_track_by_path(conn, p).ok().flatten())
                        .map(|t| t.id)
                        .collect();
                    if ids.is_empty() {
                        continue;
                    }
                    let name = path.file_stem().map_or_else(
                        || "Imported".to_string(),
                        |s| s.to_string_lossy().into_owned(),
                    );
                    if let Ok(playlist_id) = queries::create_playlist(conn, &name) {
                        for id in &ids {
                            let _ = queries::add_track_to_playlist(conn, playlist_id, *id);
                        }
                        total += ids.len();
                    }
                }
                total
            },
            move |total| {
                let _ = input.send(AudioShellInput::PlaylistImported(total));
            },
        );
    }

    /// Export every saved playlist as `<name>.m3u8` into a chosen folder.
    pub(super) fn export_all_playlists(&self) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let parent = self.window();
        FilePicker::open_folder(i18n("Export All Playlists")).select_folder_optional(
            parent.as_ref(),
            move |folder| {
                spawn_db_query(
                    db,
                    move |conn| {
                        let playlists = queries::get_all_playlists(conn).unwrap_or_default();
                        for playlist in playlists {
                            let tracks =
                                queries::get_playlist_tracks(conn, playlist.id).unwrap_or_default();
                            if tracks.is_empty() {
                                continue;
                            }
                            let file = folder.join(format!(
                                "{}.m3u8",
                                big_app_kit::files::sanitize_filename_stem_preserve_unicode(
                                    &playlist.name,
                                    "playlist",
                                )
                            ));
                            let _ = std::fs::write(
                                &file,
                                crate::audio::ui::library_io::render_m3u8(&tracks),
                            );
                        }
                    },
                    move |()| {},
                );
            },
        );
    }

    /// Persist a playlist rename, then refresh the playlist manager.
    pub(super) fn rename_playlist_db(&self, id: i64, name: String, sender: &ComponentSender<Self>) {
        if let Some(db) = self.db.clone() {
            let input = sender.input_sender().clone();
            spawn_db_query(
                db,
                move |conn| {
                    let _ = queries::rename_playlist(conn, id, &name);
                },
                move |()| {
                    let _ = input.send(AudioShellInput::RefreshPlaylists);
                },
            );
        }
    }

    /// Export a playlist to an `.m3u8` file chosen by the user.
    pub(super) fn export_playlist(&self, id: i64) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let parent = self.window();
        spawn_db_query(
            db,
            move |conn| queries::get_playlist_tracks(conn, id).unwrap_or_default(),
            move |tracks| {
                if tracks.is_empty() {
                    return;
                }
                FilePicker::save_file(i18n("Export Playlist"))
                    .initial_name("playlist.m3u8")
                    .filter(&i18n("M3U8 Playlist"), &["m3u8", "m3u"])
                    .save_optional(parent.as_ref(), move |path| {
                        let _ = std::fs::write(
                            &path,
                            crate::audio::ui::library_io::render_m3u8(&tracks),
                        );
                    });
            },
        );
    }

    /// Import a playlist file → resolve its paths → save as a new playlist.
    pub(super) fn import_playlist(&self, sender: &ComponentSender<Self>) {
        let input = sender.input_sender().clone();
        FilePicker::new(i18n("Import Playlist"))
            .filter(&i18n("Playlists"), &["m3u", "m3u8", "pls"])
            .open_optional(self.window().as_ref(), move |path| {
                let name = path.file_stem().map_or_else(
                    || "Imported".to_string(),
                    |s| s.to_string_lossy().into_owned(),
                );
                let path = path.to_string_lossy().into_owned();
                let _ = input.send(AudioShellInput::ImportPlaylistFile { path, name });
            });
    }

    /// Parse an imported playlist file, match its entries to library tracks, and
    /// save them as a new playlist. Reports how many tracks were added.
    pub(super) fn import_playlist_file(
        &self,
        path: String,
        name: String,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                let paths = playlist_import::parse_playlist_file_for_import(Path::new(&path));
                let ids: Vec<i64> = paths
                    .iter()
                    .filter_map(|p| queries::get_track_by_path(conn, p).ok().flatten())
                    .map(|t| t.id)
                    .collect();
                if !ids.is_empty() {
                    if let Ok(playlist_id) = queries::create_playlist(conn, &name) {
                        for id in &ids {
                            let _ = queries::add_track_to_playlist(conn, playlist_id, *id);
                        }
                    }
                }
                ids.len()
            },
            move |added| {
                let _ = input.send(AudioShellInput::PlaylistImported(added));
            },
        );
    }
}
