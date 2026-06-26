use super::*;

impl AudioShell {
    pub(super) fn handle_files(&mut self, out: FileBrowserOutput, sender: &ComponentSender<Self>) {
        match out {
            FileBrowserOutput::PlayFile { path, .. } => {
                let input = sender.input_sender().clone();
                let insert_pos = self.queue.len();
                spawn_blocking_result(
                    move || crate::library::metadata::read_track_metadata(&path).ok(),
                    move |track| {
                        if let Some(track) = track {
                            let _ = input.send(AudioShellInput::DroppedResolved {
                                insert_pos,
                                tracks: vec![track],
                            });
                        }
                    },
                );
            }
            FileBrowserOutput::ScanFolder(path) => self.scan_folder(path, sender),
        }
    }

    /// Scan a folder into the library off-thread (ported scan engine), then
    /// refresh the tree on completion via `ScanFinished`.
    pub(super) fn scan_folder(&self, folder: String, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetStatus(i18n("Scanning folder…")));
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let _ = gio::spawn_blocking(move || {
                library_scan_engine::scan_folder_blocking(&db, &folder)
            })
            .await;
            let _ = input.send(AudioShellInput::ScanFinished);
        });
    }

    /// Remove a watch folder (+ its tracks) off-thread, then refresh.
    pub(super) fn remove_watch_folder(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let _ = gio::spawn_blocking(move || {
                library_scan_engine::remove_watch_folder_blocking(&db, id)
            })
            .await;
            let _ = input.send(AudioShellInput::ScanFinished);
        });
    }

    /// Rescan every watch folder off-thread, then refresh via `ScanFinished`.
    pub(super) fn scan_all_watch_folders(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetStatus(i18n("Scanning library…")));
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let _ = gio::spawn_blocking(move || {
                for folder in library_scan_engine::watch_folder_paths_blocking(&db) {
                    let _ = library_scan_engine::scan_folder_blocking(&db, &folder);
                }
            })
            .await;
            let _ = input.send(AudioShellInput::ScanFinished);
        });
    }

    /// Resolve watched folders off-thread, then open the folder manager dialog.
    pub(super) fn resolve_watch_folders(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            |conn| queries::get_watch_folders(conn).unwrap_or_default(),
            move |folders| {
                let _ = input.send(AudioShellInput::ManageFoldersResolved(folders));
            },
        );
    }

    /// Resolve a radio-panel intent: station CRUD (DB) + stream playback (mpv).
    /// Explore search, favicons, and the explore cache are self-contained in the
    /// panel. Recording is deferred to the recording subsystem.
    pub(super) fn handle_radio(&mut self, out: RadioPanelOutput, sender: &ComponentSender<Self>) {
        match out {
            RadioPanelOutput::RequestStations => self.load_radio_stations(sender),
            RadioPanelOutput::SaveStation {
                name,
                url,
                favicon,
                group_name,
            } => {
                if let Some(db) = self.db.clone() {
                    let input = sender.input_sender().clone();
                    spawn_db_query(
                        db,
                        move |conn| {
                            let _ = queries::insert_radio_station(
                                conn,
                                &name,
                                &url,
                                &favicon,
                                &group_name,
                            );
                            queries::get_all_radio_stations(conn).unwrap_or_default()
                        },
                        move |stations| {
                            let _ = input.send(AudioShellInput::RadioStationsResolved(stations));
                        },
                    );
                }
            }
            RadioPanelOutput::DeleteStation(id) => {
                if let Some(db) = self.db.clone() {
                    let input = sender.input_sender().clone();
                    spawn_db_query(
                        db,
                        move |conn| {
                            let _ = queries::delete_radio_station(conn, id);
                            queries::get_all_radio_stations(conn).unwrap_or_default()
                        },
                        move |stations| {
                            let _ = input.send(AudioShellInput::RadioStationsResolved(stations));
                        },
                    );
                }
            }
            RadioPanelOutput::PlayStation { url, name, .. } => {
                self.is_playing = true;
                self.is_stream = true;
                self.radio_station_name = name;
                if let Some(mpv) = &self.mpv {
                    mpv.load_file(&url);
                    mpv.play();
                }
                self.current_file.clone_from(&url);
                // A stream is not a queue/library track — clear those indicators.
                self.library.emit(LibraryPanelInput::SetPlayingPath(None));
                self.playlist
                    .emit(PlaylistPanelInput::SetPlaying { idx: None });
                self.radio.emit(RadioPanelInput::NowPlayingChanged {
                    url,
                    is_stream: true,
                });
                self.sync_now_playing_card();
            }
            RadioPanelOutput::RecordStream => self.record_radio_stream(),
            RadioPanelOutput::StopRecording => self.stop_radio_recording(),
            RadioPanelOutput::Notify(message) => self.notify(&message),
        }
    }

    /// Show a transient toast in the window overlay.
    pub(super) fn notify(&self, message: &str) {
        self.toast_overlay.add_toast(adw::Toast::new(message));
    }

    /// Start recording the current radio stream into `~/Music/Radio Recordings`.
    pub(super) fn record_radio_stream(&mut self) {
        let Some(mpv) = &self.mpv else {
            return;
        };
        let music_dir = glib::user_special_dir(glib::UserDirectory::Music)
            .unwrap_or_else(|| glib::home_dir().join("Music"));
        let rec_dir = music_dir.join("Radio Recordings");
        let _ = std::fs::create_dir_all(&rec_dir);

        let safe_name = big_app_kit::files::sanitize_filename_stem_preserve_unicode(
            &self.radio_station_name,
            "radio",
        );
        let ts = glib::DateTime::now_local()
            .ok()
            .and_then(|now| now.format("%Y-%m-%d_%H-%M-%S").ok())
            .map_or_else(|| "unknown".to_string(), |s| s.to_string());
        let filename = format!("{safe_name}_{ts}.mp3");
        let path = rec_dir.join(&filename);

        mpv.set_stream_record(path.to_str().unwrap_or(""));
        self.recording_path = Some(path);
        self.notify(&format!("{}: {filename}", i18n("Recording to")));
    }

    /// Stop the in-progress recording and show a saved dialog with open actions.
    pub(super) fn stop_radio_recording(&mut self) {
        if let Some(mpv) = &self.mpv {
            mpv.set_stream_record("");
        }
        let Some(path) = self.recording_path.take() else {
            return;
        };
        let filename = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let folder = path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        // bigagents: app-local-dialog. Recording completion has domain-specific
        // open-folder/open-file actions.
        let dialog = adw::AlertDialog::builder()
            .heading(i18n("Recording saved"))
            .body(format!("{filename}\n\n📁 {folder}"))
            .build();
        dialog.add_response("close", &i18n("Close"));
        dialog.add_response("open_folder", &i18n("Open Folder"));
        dialog.add_response("open_file", &i18n("Open File"));
        dialog.set_response_appearance("open_file", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("open_file"));
        dialog.set_close_response("close");
        dialog.connect_response(None, move |_, response| match response {
            "open_folder" => big_app_kit::desktop::open_containing_folder(&path),
            "open_file" => big_app_kit::desktop::open_file(&path),
            _ => {}
        });
        if let Some(win) = self.window() {
            dialog.present(Some(&win));
        }
    }

    /// Resolve a podcast-panel intent: fetch the feed off-thread → enter podcast
    /// mode (`PodcastFeedResolved`). Subscribe/search/feed-list are self-contained.
    pub(super) fn handle_podcast(
        &mut self,
        out: PodcastPanelOutput,
        sender: &ComponentSender<Self>,
    ) {
        match out {
            PodcastPanelOutput::LoadFeed(url) => {
                let input = sender.input_sender().clone();
                self.notify(&i18n("Loading feed…"));
                spawn_blocking_result(
                    move || crate::library::podcast::fetch_feed(&url),
                    move |feed| {
                        let Some(feed) = feed else {
                            let _ = input.send(AudioShellInput::PodcastFeedResolved {
                                title: String::new(),
                                episodes: Vec::new(),
                            });
                            return;
                        };
                        let episodes = feed
                            .episodes
                            .into_iter()
                            .map(|ep| (ep.audio_url, ep.title))
                            .collect();
                        let _ = input.send(AudioShellInput::PodcastFeedResolved {
                            title: feed.title,
                            episodes,
                        });
                    },
                );
            }
            PodcastPanelOutput::Notify(message) => self.notify(&message),
        }
    }

    /// Enter podcast mode: stash the music queue (first entry), rebuild the queue
    /// from the feed's episodes, and start the first one.
    pub(super) fn enter_podcast_mode(&mut self, title: String, episodes: Vec<(String, String)>) {
        if episodes.is_empty() {
            self.notify(&i18n("No episodes found"));
            return;
        }
        if !self.podcast_mode {
            self.saved_music_queue =
                Some((self.queue.tracks().to_vec(), self.queue.current_index()));
        }
        self.podcast_mode = true;
        self.current_playlist_name.clone_from(&title);

        let tracks = episodes
            .into_iter()
            .enumerate()
            .map(|(i, (audio_url, ep_title))| {
                Track::new_podcast_episode(
                    -(i as i64 + 1),
                    audio_url,
                    ep_title,
                    title.clone(),
                    i as i32 + 1,
                )
            })
            .collect::<Vec<_>>();
        self.queue.set_tracks(tracks);
        self.queue.jump_to(0);
        self.play_current();
        self.sync_queue_to_panels();
    }

    /// Stash/restore the music vs podcast queues on view-stack tab switches,
    /// mirroring the god-object's podcast mode.
    pub(super) fn handle_tab_changed(&mut self, name: &str) {
        if name == "podcast" {
            // Returning to the podcast tab restores a stashed episode queue.
            if !self.podcast_mode {
                if let Some((tracks, index)) = self.saved_podcast_queue.take() {
                    self.saved_music_queue =
                        Some((self.queue.tracks().to_vec(), self.queue.current_index()));
                    self.podcast_mode = true;
                    self.queue.set_tracks(tracks);
                    if let Some(idx) = index {
                        self.queue.jump_to(idx);
                    }
                    self.commit_queue();
                }
            }
        } else if self.podcast_mode {
            // Leaving the podcast tab stashes the episode queue and restores music.
            self.saved_podcast_queue =
                Some((self.queue.tracks().to_vec(), self.queue.current_index()));
            self.podcast_mode = false;
            if let Some((tracks, index)) = self.saved_music_queue.take() {
                self.queue.set_tracks(tracks);
                if let Some(idx) = index {
                    self.queue.jump_to(idx);
                }
                self.commit_queue();
            }
        }
    }

    /// Load the saved radio stations off-thread → `RadioStationsResolved`.
    pub(super) fn load_radio_stations(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            move |conn| queries::get_all_radio_stations(conn).unwrap_or_default(),
            move |stations| {
                let _ = input.send(AudioShellInput::RadioStationsResolved(stations));
            },
        );
    }
}
