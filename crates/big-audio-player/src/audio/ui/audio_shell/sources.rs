use super::*;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
};

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

    pub(super) fn scan_folder(&self, folder: String, sender: &ComponentSender<Self>) {
        self.start_library_scan(Some(vec![folder]), sender);
    }

    fn start_library_scan(&self, folders: Option<Vec<String>>, sender: &ComponentSender<Self>) {
        if self.scan_slot.0.borrow().as_ref().is_some_and(|control| !control.finished.load(Ordering::Acquire)) {
            self.notify(&i18n("A library scan is already running. Cancel it or wait for its result."));
            return;
        }
        let Some(db) = self.db.clone() else { return; };
        let Some(admission) = library_scan_engine::ScanAdmission::reserve() else {
            self.notify(&i18n("Library scanning is busy. Try again after another scan finishes."));
            return;
        };
        let control = Arc::new(library_scan_engine::ScanControl::default());
        *self.scan_slot.0.borrow_mut() = Some(control.clone());
        if let Some(parent) = self.window() {
            super::super::scan_progress::show(&parent, control.clone());
        }
        self.playback_bar.emit(AudioPlaybackBarInput::SetStatus(i18n("Scanning library…")));
        let worker_control = control.clone(); let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let _admission = admission;
                let folders = match folders {
                    Some(folders) => folders,
                    None => library_scan_engine::watch_folder_paths_cancellable(&db, &worker_control.cancelled)?,
                };
                for folder in folders {
                    library_scan_engine::scan_folder_cancellable(&db, &folder, &worker_control)?;
                }
                Ok::<(), String>(())
            }).await;
            let failed = !matches!(result, Ok(Ok(()))) && !control.cancelled.load(Ordering::Acquire);
            control.failed.store(failed, Ordering::Release);
            control.finished.store(true, Ordering::Release);
            let report = control.report(failed);
            let _ = input.send(AudioShellInput::LibraryScanFinished { control, report });
        });
    }

    /// Stop monitoring a watch folder, preserving its tracks, then refresh.
    pub(super) fn remove_watch_folder(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.db.clone() else {
            return;
        };
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let complete = matches!(gio::spawn_blocking(move || {
                library_scan_engine::remove_watch_folder_blocking(&db, id)
            }).await, Ok(true));
            let _ = input.send(AudioShellInput::ScanFinished(complete));
        });
    }

    /// Rescan one folder at a time under the same bounded cancellation token.
    pub(super) fn scan_all_watch_folders(&self, sender: &ComponentSender<Self>) {
        self.start_library_scan(None, sender);
    }

    /// Resolve watched folders off-thread, then open the folder manager dialog.
    pub(super) fn resolve_watch_folders(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.db_read.clone().or_else(|| self.db.clone()) else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(
            db,
            queries::get_watch_folders,
            move |folders| {
                let message = match folders {
                    Ok(folders) => AudioShellInput::ManageFoldersResolved(folders),
                    Err(_) => AudioShellInput::Notify(i18n("Watch folders could not be read. Previous settings were kept.")),
                };
                let _ = input.send(message);
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
                self.mpris_state.invalidate();
                self.is_playing = true;
                self.is_stream = true;
                self.radio_station_name = name;
                if let Some(mpv) = &self.mpv {
                    mpv.load_file_with_audio_profile(&url, crate::player::mpv_backend::AudioCacheProfile::Radio, false);
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
        let parent = self.window().map(|window| window.downgrade());
        let toast_overlay = self.toast_overlay.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(750), move || {
            if !recording_file_has_content(&path) {
                toast_overlay.add_toast(adw::Toast::new(&i18n("Recording was not saved")));
                return;
            }
            if let Some(parent) = parent.as_ref().and_then(glib::WeakRef::upgrade) {
                present_recording_saved_dialog(&parent, path);
            }
        });
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
            if !self.podcast_mode
                && let Some((tracks, index)) = self.saved_podcast_queue.take()
            {
                self.saved_music_queue =
                    Some((self.queue.tracks().to_vec(), self.queue.current_index()));
                self.podcast_mode = true;
                self.queue.set_tracks(tracks);
                if let Some(idx) = index {
                    self.queue.jump_to(idx);
                }
                self.commit_queue();
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordingSavedPaths {
    file_name: String,
    folder_name: String,
}

fn recording_saved_paths(path: &Path) -> RecordingSavedPaths {
    RecordingSavedPaths {
        file_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        folder_name: path
            .parent()
            .map(|folder| folder.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

fn recording_file_has_content(path: &Path) -> bool {
    std::fs::metadata(path)
        .is_ok_and(|metadata| recording_metadata_has_content(metadata.is_file(), metadata.len()))
}

fn recording_metadata_has_content(is_file: bool, byte_len: u64) -> bool {
    is_file && byte_len > 0
}

fn present_recording_saved_dialog(parent: &impl IsA<gtk::Widget>, path: PathBuf) {
    let saved_paths = recording_saved_paths(&path);
    let dialog = adw::Dialog::builder()
        .title(i18n("Recording saved"))
        .content_width(460)
        .content_height(340)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&i18n("The recording was saved and is ready to open."));

    let rows = gtk::Box::new(gtk::Orientation::Vertical, 8);
    rows.append(&recording_saved_field(
        &i18n("File"),
        &saved_paths.file_name,
    ));
    rows.append(&recording_saved_field(
        &i18n("Folder"),
        &saved_paths.folder_name,
    ));
    page.content().append(&rows);

    let open_folder_button = gtk::Button::builder()
        .label(i18n("Open Folder"))
        .halign(gtk::Align::Start)
        .build();
    page.content().append(&open_folder_button);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Close"), i18n("Open File")));
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let folder_path = path.clone();
    let dialog_weak = dialog.downgrade();
    open_folder_button.connect_clicked(move |_| {
        big_app_kit::desktop::open_containing_folder(&folder_path);
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let file_path = path;
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        big_app_kit::desktop::open_file(&file_path);
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    dialog.present(Some(parent));
}

fn recording_saved_field(label: &str, value: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let title = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("caption");
    title.add_css_class("dim-label");

    let text = gtk::Label::builder()
        .label(value)
        .halign(gtk::Align::Start)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .build();
    row.append(&title);
    row.append(&text);
    row
}

#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    use super::recording_file_has_content;
    use super::{RecordingSavedPaths, recording_metadata_has_content, recording_saved_paths};
    use std::path::Path;
    #[cfg(not(miri))]
    use std::{fs, io::Write};

    #[test]
    fn recording_saved_paths_split_file_and_parent_folder() {
        assert_eq!(
            recording_saved_paths(Path::new("Music/Radio Recordings/show.mp3")),
            RecordingSavedPaths {
                file_name: "show.mp3".into(),
                folder_name: "Music/Radio Recordings".into(),
            }
        );
    }

    #[test]
    fn recording_metadata_has_content_requires_file_with_bytes() {
        assert!(!recording_metadata_has_content(false, 4));
        assert!(!recording_metadata_has_content(true, 0));
        assert!(recording_metadata_has_content(true, 4));
    }

    #[test]
    #[cfg(not(miri))]
    fn recording_file_has_content_requires_existing_non_empty_file() {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "big-audio-recording-test-{}.mp3",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);

        assert!(!recording_file_has_content(&path));

        fs::File::create(&path).expect("create empty recording fixture");
        assert!(!recording_file_has_content(&path));

        let mut file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open recording fixture");
        file.write_all(b"audio").expect("write recording fixture");
        file.sync_all().expect("sync recording fixture");
        assert!(recording_file_has_content(&path));

        fs::remove_file(&path).expect("remove recording fixture");
    }
}
