use super::*;

impl AudioShell {
    pub(super) fn refresh_playlist_dialog_entries(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| {
                let playlists = queries::get_all_playlists(conn)?;
                playlists
                    .into_iter()
                    .map(|playlist| {
                        let (count, dur) = queries::get_playlist_stats(conn, playlist.id)?;
                        Ok((playlist.id, playlist.name, count, dur))
                    })
                    .collect::<big_media_runtime::Result<Vec<_>>>()
            },
            move |result| {
                let message = match result {
                    Ok(entries) => AudioShellInput::ManagePlaylistsResolved(entries),
                    Err(_) => AudioShellInput::PlaylistReadFailed,
                };
                let _ = input.send(message);
            },
        );
    }

    /// Resolve a playlist's tracks → replace the queue and play
    /// (`PlaylistTracksForQueue`). This is the dialog's "Load" action.
    pub(super) fn load_playlist_to_queue(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| queries::get_playlist_tracks(conn, id),
            move |result| {
                let message = match result {
                    Ok(tracks) => AudioShellInput::PlaylistTracksForQueue(tracks),
                    Err(_) => AudioShellInput::PlaylistReadFailed,
                };
                let _ = input.send(message);
            },
        );
    }

    /// Dispatch a playlist-dialog action: save current queue, load, rename,
    /// delete, export, or import. Mutations re-resolve the dialog list.
    pub(super) fn handle_playlist_library_dialog(
        &mut self,
        dialog_output: PlaylistLibraryDialogOutput,
        sender: &ComponentSender<Self>,
    ) {
        match dialog_output {
            // Mutations refresh the dialog via their completion → RefreshPlaylists
            // (not immediately, which would read before the write commits).
            PlaylistLibraryDialogOutput::SaveCurrent { name } => {
                self.save_queue_as_playlist(name, sender);
            }
            PlaylistLibraryDialogOutput::Load(id) => self.load_playlist_to_queue(id, sender),
            PlaylistLibraryDialogOutput::Rename { id, name } => {
                self.rename_playlist_db(id, name, sender);
            }
            PlaylistLibraryDialogOutput::Delete(id) => self.delete_playlist(id, sender),
            PlaylistLibraryDialogOutput::Export(id) => self.export_playlist(id),
            PlaylistLibraryDialogOutput::Import => self.import_playlist(sender),
            PlaylistLibraryDialogOutput::ImportFolder => self.import_playlist_folder(sender),
            PlaylistLibraryDialogOutput::ExportAll => self.export_all_playlists(),
            PlaylistLibraryDialogOutput::GenerateLoad(kind) => {
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
                "most-played" => queries::get_most_played(conn, 100),
                "recently-added" => queries::get_recently_added(conn, 100),
                "never-played" => queries::get_never_played(conn, 100),
                _ => Err(big_media_runtime::Error::invalid_argument(
                    "playlist",
                    "Unknown smart playlist",
                )),
            },
            move |result| {
                let message = match result {
                    Ok(tracks) => AudioShellInput::PlaylistTracksForQueue(tracks),
                    Err(_) => AudioShellInput::PlaylistReadFailed,
                };
                let _ = input.send(message);
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

    /// Import one bounded file at a time; parse outside the DB lock. Each
    /// playlist is committed atomically, and later failure preserves prior ones.
    pub(super) fn import_playlist_folder_db(
        &self,
        folder: PathBuf,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        crate::settings::acknowledged_task(
            false,
            move || {
                let mut paths = Vec::new();
                for (index, entry) in std::fs::read_dir(folder)?.enumerate() {
                    if index >= 4096 {
                        return Err(std::io::Error::other(i18n(
                            "Too many entries in the chosen folder",
                        )));
                    }
                    let path = entry?.path();
                    if path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        matches!(e.to_ascii_lowercase().as_str(), "m3u" | "m3u8" | "pls")
                    }) {
                        paths.push(path);
                        if paths.len() > 100 {
                            return Err(std::io::Error::other(i18n(
                                "Choose at most 100 playlist files per import",
                            )));
                        }
                    }
                }
                paths.sort();
                let mut total = 0_usize;
                for path in paths {
                    let fallback = i18n("Imported");
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(&fallback);
                    match import_one_playlist(&db, &path, name, 100_000 - total) {
                        Ok(added) => total += added,
                        Err(_) => {
                            return Err(std::io::Error::other(format!(
                                "{}: {total}",
                                i18n("Import stopped. Confirmed tracks were kept")
                            )));
                        }
                    }
                }
                Ok(total)
            },
            move |result| {
                let _ = input.send(AudioShellInput::PlaylistImported(
                    result.map_err(|e| e.to_string()),
                ));
            },
        );
    }

    /// Export private snapshots on the persistence worker. Publication never
    /// replaces an existing document; a partial set is reported and retained.
    pub(super) fn export_all_playlists(&self) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let parent = self.playlist_library_parent();
        let weak_parent = parent.as_ref().map(glib::object::ObjectExt::downgrade);
        FilePicker::open_folder(i18n("Export All Playlists")).select_folder_optional(
            parent.as_ref(),
            move |folder| {
                crate::settings::acknowledged_task(
                    false,
                    move || export_playlists(&db, &folder),
                    move |result| {
                        let body = match result {
                            Ok(count) => format!("{}: {count}", i18n("Playlists exported")),
                            Err(error) => format!(
                                "{} {error}",
                                i18n(
                                    "Export was not fully completed. Published files were kept; choose an empty destination and try again.",
                                )
                            ),
                        };
                        show_export_result(weak_parent.as_ref(), &body);
                    },
                );
            },
        );
    }

    /// Persist a playlist rename, then refresh the playlist dialog.
    pub(super) fn rename_playlist_db(&self, id: i64, name: String, sender: &ComponentSender<Self>) {
        if let Some(db) = self.db.clone() {
            let input = sender.input_sender().clone();
            spawn_db_query(
                db,
                move |conn| queries::rename_playlist(conn, id, &name).is_ok(),
                move |success| {
                    let _ = input.send(AudioShellInput::PlaylistMutationFinished(success));
                },
            );
        }
    }

    /// Export with an acknowledged disk task; read failures never become an
    /// empty successful playlist, and a chosen existing document stays intact.
    pub(super) fn export_playlist(&self, id: i64) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let parent = self.playlist_library_parent();
        let weak_parent = parent.as_ref().map(glib::object::ObjectExt::downgrade);
        FilePicker::save_file(i18n("Export Playlist"))
            .initial_name("playlist.m3u8")
            .filter(&i18n("M3U8 Playlist"), &["m3u8", "m3u"])
            .save_optional(parent.as_ref(), move |path| {
                crate::settings::acknowledged_task(
                    false,
                    move || {
                        let items = playlist_paths(&db, id)?;
                        crate::media::playlist_scan::write_m3u(&path, &items)
                            .map_err(std::io::Error::other)
                    },
                    move |result| {
                        let text = i18n(if result.is_ok() {
                            "Playlist exported."
                        } else {
                            "The playlist was not exported. Check storage or choose a new filename; existing files were not replaced."
                        });
                        show_export_result(weak_parent.as_ref(), &text);
                    },
                );
            });
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
                let _ = input.send(AudioShellInput::ImportPlaylistFile { path, name });
            });
    }

    /// Parse outside the database lock; report only an acknowledged transaction.
    pub(super) fn import_playlist_file(
        &self,
        path: PathBuf,
        name: String,
        sender: &ComponentSender<Self>,
    ) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        crate::settings::acknowledged_task(
            false,
            move || import_one_playlist(&db, &path, &name, 100_000),
            move |result| {
                let _ = input.send(AudioShellInput::PlaylistImported(
                    result.map_err(|e| e.to_string()),
                ));
            },
        );
    }
}

fn import_one_playlist(
    db: &DbHandle,
    path: &Path,
    name: &str,
    remaining: usize,
) -> std::io::Result<usize> {
    if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        return Err(std::io::Error::other("Invalid playlist name"));
    }
    let paths = playlist_import::parse_playlist_file_for_import(path)?;
    if paths.len() > remaining {
        return Err(std::io::Error::other(i18n(
            "The batch import limit was reached. Split the playlists into smaller imports.",
        )));
    }
    // Short lock wait, never a GUI lock. Busy databases produce a retryable
    // error instead of holding up all later saves indefinitely.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let conn = loop {
        match db.try_lock() {
            Ok(conn) => break conn,
            Err(std::sync::TryLockError::Poisoned(_)) => {
                return Err(std::io::Error::other(i18n("Library unavailable")));
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                if std::time::Instant::now() >= until {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WouldBlock,
                        "Library busy; retry import",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    };
    let mut ids = Vec::new();
    for path in paths {
        if let Some(track) =
            queries::get_track_by_path(&conn, &path).map_err(std::io::Error::other)?
        {
            ids.push(track.id);
        }
    }
    if ids.is_empty() {
        return Ok(0);
    }
    queries::create_playlist_with_tracks(&conn, name, &ids).map_err(std::io::Error::other)?;
    Ok(ids.len())
}

/// Most playlists one "Export All" run writes.
const MAX_EXPORTED_PLAYLISTS: usize = 1000;

/// Write every playlist into `folder` as `<name>-<id>.m3u8` and return how many
/// were published. Stops at the first failure; files already written stay.
fn export_playlists(db: &DbHandle, folder: &Path) -> std::io::Result<usize> {
    let playlists = {
        let conn = db
            .lock()
            .map_err(|_| std::io::Error::other(i18n("Library unavailable")))?;
        let mut statement = conn
            .prepare("SELECT id,name FROM playlists ORDER BY id LIMIT ?1")
            .map_err(std::io::Error::other)?;
        let rows = statement
            .query_map([MAX_EXPORTED_PLAYLISTS as i64 + 1], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(std::io::Error::other)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(std::io::Error::other)?
    };
    if playlists.len() > MAX_EXPORTED_PLAYLISTS {
        return Err(std::io::Error::other(i18n(
            "Too many playlists for one export",
        )));
    }
    let mut published = 0;
    for (id, name) in playlists {
        let name = big_app_kit::files::sanitize_filename_stem_preserve_unicode(&name, "playlist");
        // The id keeps equal or equally sanitized names from colliding.
        let target = folder.join(format!("{name}-{id}.m3u8"));
        let written = playlist_paths(db, id).and_then(|items| {
            crate::media::playlist_scan::write_m3u(&target, &items).map_err(std::io::Error::other)
        });
        if written.is_err() {
            return Err(std::io::Error::other(format!(
                "{}: {published}",
                i18n("Export stopped. Confirmed outputs were kept")
            )));
        }
        published += 1;
    }
    Ok(published)
}

fn playlist_paths(db: &DbHandle, id: i64) -> std::io::Result<Vec<String>> {
    let conn = db
        .lock()
        .map_err(|_| std::io::Error::other(i18n("Library unavailable")))?;
    let tracks = queries::get_playlist_tracks(&conn, id).map_err(std::io::Error::other)?;
    Ok(tracks.into_iter().map(|track| track.file_path).collect())
}

fn show_export_result(parent: Option<&glib::WeakRef<gtk::Window>>, body: &str) {
    let Some(parent) = parent.and_then(glib::WeakRef::upgrade) else {
        return;
    };
    let dialog = adw::AlertDialog::builder()
        .heading(i18n("Playlist export"))
        .body(body)
        .build();
    dialog.add_response("close", &i18n("Close"));
    dialog.present(Some(&parent));
}
