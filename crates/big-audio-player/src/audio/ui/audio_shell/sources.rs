use super::queue_state::{
    LoadedAfterSwap, loaded_after_swap, podcast_episode_tracks, start_podcast_queue,
    swap_queue_for_tab,
};
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
        if self
            .scan_slot
            .0
            .borrow()
            .as_ref()
            .is_some_and(|control| !control.finished.load(Ordering::Acquire))
        {
            self.notify(&i18n(
                "A library scan is already running. Cancel it or wait for its result.",
            ));
            return;
        }
        let Some(db) = self.writer() else {
            return;
        };
        let Some(admission) = library_scan_engine::ScanAdmission::reserve() else {
            self.notify(&i18n(
                "Library scanning is busy. Try again after another scan finishes.",
            ));
            return;
        };
        let control = Arc::new(library_scan_engine::ScanControl::default());
        *self.scan_slot.0.borrow_mut() = Some(control.clone());
        if let Some(parent) = self.window() {
            super::super::scan_progress::show(&parent, control.clone());
        }
        self.playback_bar
            .emit(AudioPlaybackBarInput::SetStatus(i18n("Scanning library…")));
        let worker_control = control.clone();
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                let _admission = admission;
                let folders = match folders {
                    Some(folders) => folders,
                    None => library_scan_engine::watch_folder_paths_cancellable(
                        &db,
                        &worker_control.cancelled,
                    )?,
                };
                for folder in folders {
                    library_scan_engine::scan_folder_cancellable(&db, &folder, &worker_control)?;
                }
                Ok::<(), String>(())
            })
            .await;
            let failed =
                !matches!(result, Ok(Ok(()))) && !control.cancelled.load(Ordering::Acquire);
            control.failed.store(failed, Ordering::Release);
            control.finished.store(true, Ordering::Release);
            let report = control.report();
            let _ = input.send(AudioShellInput::LibraryScanFinished { control, report });
        });
    }

    /// Stop monitoring a watch folder, preserving its tracks, then refresh.
    pub(super) fn remove_watch_folder(&self, id: i64, sender: &ComponentSender<Self>) {
        let Some(db) = self.writer() else {
            return;
        };
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let complete = matches!(
                gio::spawn_blocking(move || {
                    library_scan_engine::remove_watch_folder_blocking(&db, id)
                })
                .await,
                Ok(true)
            );
            let _ = input.send(AudioShellInput::ScanFinished(complete));
        });
    }

    /// Rescan one folder at a time under the same bounded cancellation token.
    pub(super) fn scan_all_watch_folders(&self, sender: &ComponentSender<Self>) {
        self.start_library_scan(None, sender);
    }

    /// Resolve watched folders off-thread, then open the folder manager dialog.
    pub(super) fn resolve_watch_folders(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.reader() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(db, queries::get_watch_folders, move |folders| {
            let message = match folders {
                Ok(folders) => AudioShellInput::ManageFoldersResolved(folders),
                Err(_) => AudioShellInput::Notify(i18n(
                    "Watch folders could not be read. Previous settings were kept.",
                )),
            };
            let _ = input.send(message);
        });
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
                if let Some(db) = self.writer() {
                    let input = sender.input_sender().clone();
                    spawn_db_query(
                        db,
                        move |conn| {
                            queries::insert_radio_station(conn, &name, &url, &favicon, &group_name)
                        },
                        move |saved| {
                            let _ = input.send(after_station_write(saved, || {
                                i18n("The station could not be saved.")
                            }));
                        },
                    );
                }
            }
            RadioPanelOutput::DeleteStation(id) => {
                if let Some(db) = self.writer() {
                    let input = sender.input_sender().clone();
                    spawn_db_query(
                        db,
                        move |conn| queries::delete_radio_station(conn, id),
                        move |deleted| {
                            let _ = input.send(after_station_write(deleted, || {
                                i18n("The station could not be removed.")
                            }));
                        },
                    );
                }
            }
            RadioPanelOutput::PlayStation { url, name } => {
                if self.report_missing_engine() {
                    return;
                }
                self.mpris_state.invalidate();
                self.is_playing = true;
                self.is_stream = true;
                self.radio_station_name = name;
                if let Some(mpv) = &self.mpv {
                    mpv.load_file_with_audio_profile(
                        &url,
                        TargetSource::List(ListOrigin::Network),
                        AudioCacheProfile::Radio,
                    );
                    mpv.play();
                }
                self.current_file.clone_from(&url);
                // A stream is not a queue/library track; clear those indicators.
                self.library.emit(LibraryPanelInput::SetPlayingPath {
                    path: None,
                    playback: PlaybackStatus::Stopped,
                });
                self.playlist.emit(PlaylistPanelInput::SetPlaying {
                    idx: None,
                    playback: PlaybackStatus::Stopped,
                });
                self.radio.emit(RadioPanelInput::NowPlayingChanged {
                    url,
                    is_stream: true,
                });
                self.sync_now_playing_card();
            }
            RadioPanelOutput::RecordStream => self.record_radio_stream(sender),
            RadioPanelOutput::StopRecording => self.stop_radio_recording(sender),
        }
    }

    /// Tell the person something, in the row above the transport.
    pub(super) fn notify(&self, message: &str) {
        self.notice.show(message);
    }

    /// Say again why nothing can play when the engine failed to start; true then.
    pub(super) fn report_missing_engine(&self) -> bool {
        let Some(failure) = &self.engine_failure else {
            return false;
        };
        self.notify(failure);
        true
    }

    /// Start recording the current radio stream into `~/Music/Radio Recordings`.
    pub(super) fn record_radio_stream(&mut self, sender: &ComponentSender<Self>) {
        if self.report_missing_engine() {
            return;
        }
        let Some(music_dir) = big_os_kit::xdg_dirs::user_dir(big_os_kit::xdg_dirs::UserDir::Music)
            .or_else(|| big_os_kit::xdg_dirs::home_dir().map(|home| home.join("Music")))
        else {
            self.notify(&i18n(
                "There is no Music or home folder to save recordings in.",
            ));
            return;
        };
        let rec_dir = music_dir.join("Radio Recordings");
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

        self.recording_generation = self.recording_generation.wrapping_add(1);
        let generation = self.recording_generation;
        let source = self.current_file.clone();
        let input = sender.input_sender().clone();
        spawn_blocking_result(
            move || std::fs::create_dir_all(&rec_dir).map_err(|error| error.to_string()),
            move |result| {
                let _ = input.send(AudioShellInput::RadioRecordingPrepared {
                    generation,
                    source,
                    path,
                    result,
                });
            },
        );
    }

    pub(super) fn start_prepared_recording(
        &mut self,
        generation: u64,
        source: &str,
        path: PathBuf,
        result: Result<(), String>,
    ) {
        if generation != self.recording_generation || !self.is_stream || source != self.current_file
        {
            return;
        }
        if let Err(error) = result {
            log::warn!("cannot prepare recording directory: {error}");
            self.notify(&i18n("Recording was not saved"));
            return;
        }
        let Some(mpv) = &self.mpv else {
            return;
        };
        // mpv takes a UTF-8 path; an empty one would silently disable recording.
        let Some(record_path) = path.to_str() else {
            self.notify(&i18n("Recording was not saved"));
            return;
        };
        mpv.set_stream_record(record_path);
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        self.notify(&format!("{}: {filename}", i18n("Recording to")));
        self.recording_path = Some(path);
    }

    /// Stop the in-progress recording and show a saved dialog with open actions.
    pub(super) fn stop_radio_recording(&mut self, sender: &ComponentSender<Self>) {
        self.recording_generation = self.recording_generation.wrapping_add(1);
        if let Some(mpv) = &self.mpv {
            mpv.set_stream_record("");
        }
        let Some(path) = self.recording_path.take() else {
            return;
        };
        let parent = self.window().map(|window| window.downgrade());
        let input = sender.input_sender().clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(750), move || {
            spawn_blocking_result(
                move || {
                    let has_content = recording_file_has_content(&path);
                    (path, has_content)
                },
                move |(path, has_content)| {
                    if !has_content {
                        let _ =
                            input.send(AudioShellInput::Notify(i18n("Recording was not saved")));
                    } else if let Some(parent) = parent.as_ref().and_then(glib::WeakRef::upgrade) {
                        present_recording_saved_dialog(&parent, path);
                    }
                },
            );
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
                spawn_blocking_result(
                    move || crate::library::podcast::fetch_feed(&url),
                    move |feed| {
                        let Some(feed) = feed else {
                            let _ = input.send(AudioShellInput::PodcastFeedResolved {
                                title: String::new(),
                                episodes: Vec::new(),
                                skipped: 0,
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
                            skipped: feed.skipped,
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
        self.cancel_pending_opens();
        start_podcast_queue(
            &mut self.podcast_mode,
            &mut self.queue,
            &mut self.saved_music_queue,
            podcast_episode_tracks(&title, episodes),
        );
        self.current_playlist_name = title;
        self.play_current();
        self.commit_queue();
    }

    /// Stash/restore the music vs podcast queues on view-stack tab switches.
    pub(super) fn handle_tab_changed(&mut self, name: &str, sender: &ComponentSender<Self>) {
        self.cancel_pending_opens();
        if !swap_queue_for_tab(
            name,
            &mut self.podcast_mode,
            &mut self.queue,
            &mut self.saved_music_queue,
            &mut self.saved_podcast_queue,
        ) {
            return;
        }
        let loaded = loaded_after_swap(&self.queue, &self.current_file);
        if !self.is_stream && loaded != LoadedAfterSwap::Keep {
            self.handle_mpris_command(MprisCommand::Stop, sender);
            if loaded == LoadedAfterSwap::Select {
                self.play_current_as(PlaybackStatus::Stopped);
            } else {
                self.current_file.clear();
            }
            self.now_playing_gen = self.now_playing_gen.wrapping_add(1);
            self.sync_now_playing_card();
        }
        self.commit_queue();
    }

    /// Load the saved radio stations off-thread → `RadioStationsResolved`.
    pub(super) fn load_radio_stations(&self, sender: &ComponentSender<Self>) {
        let Some(db) = self.deferred_reader() else {
            return;
        };
        let input = sender.input_sender().clone();
        spawn_db_query(db, queries::get_all_radio_stations, move |stations| {
            let _ = input.send(loaded_stations_message(stations));
        });
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
    let dialog = big_gtk_kit::dialog::Dialog::builder()
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

/// A failed station write keeps the shown list and says so; a successful one rereads it.
fn after_station_write<T>(
    written: big_media_runtime::Result<T>,
    failure: impl FnOnce() -> String,
) -> AudioShellInput {
    match written {
        Ok(_) => AudioShellInput::Radio(RadioPanelOutput::RequestStations),
        Err(error) => {
            log::warn!("radio station could not be written: {error}");
            AudioShellInput::Notify(failure())
        }
    }
}

fn loaded_stations_message(
    stations: big_media_runtime::Result<Vec<RadioStation>>,
) -> AudioShellInput {
    match stations {
        Ok(stations) => AudioShellInput::RadioStationsResolved(stations),
        Err(error) => {
            log::warn!("radio stations could not be read: {error}");
            AudioShellInput::Notify(i18n("Saved radio stations could not be read."))
        }
    }
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
    fn saved_paths_split_file_and_folder() {
        assert_eq!(
            recording_saved_paths(Path::new("Music/Radio Recordings/show.mp3")),
            RecordingSavedPaths {
                file_name: "show.mp3".into(),
                folder_name: "Music/Radio Recordings".into(),
            }
        );
    }

    #[test]
    fn metadata_content_needs_file_bytes() {
        assert!(!recording_metadata_has_content(false, 4));
        assert!(!recording_metadata_has_content(true, 0));
        assert!(recording_metadata_has_content(true, 4));
    }

    #[test]
    #[cfg(not(miri))]
    fn file_content_needs_non_empty_file() {
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

    #[test]
    #[cfg(not(miri))]
    fn station_failures_keep_the_list() {
        use super::{
            AudioShellInput, RadioPanelOutput, after_station_write, loaded_stations_message,
        };
        use crate::db::{queries, schema};

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        let saved = queries::insert_radio_station(&conn, "A", "https://a.test", "", "");
        assert!(matches!(
            after_station_write(saved, || "save failed".into()),
            AudioShellInput::Radio(RadioPanelOutput::RequestStations)
        ));

        conn.execute_batch(
            "CREATE TEMP TRIGGER refuse BEFORE INSERT ON radio_stations
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();
        let refused = queries::insert_radio_station(&conn, "B", "https://b.test", "", "");
        assert!(matches!(
            after_station_write(refused, || "save failed".into()),
            AudioShellInput::Notify(message) if message == "save failed"
        ));

        conn.execute_batch("DROP TABLE radio_stations").unwrap();
        assert!(matches!(
            loaded_stations_message(queries::get_all_radio_stations(&conn)),
            AudioShellInput::Notify(_)
        ));
    }
}
