use super::*;

impl AudioShell {
    /// Push the current queue snapshot into the queue panel and re-light the
    /// now-playing indicator across both panels.
    pub(super) fn sync_queue_to_panels(&self) {
        let tracks = self.queue.tracks().to_vec();
        let current_idx = self.queue.current_index();
        self.playlist.emit(PlaylistPanelInput::SetQueue {
            tracks,
            current_idx,
            playlist_name: self.current_playlist_name.clone(),
            podcasts: self.podcast_mode,
        });
        self.sync_now_playing();
    }

    /// After a move within the queue (next, previous, jump): scroll the queue
    /// panel to the new current track and re-light it. The tracks are
    /// unchanged, so the panel keeps its rows instead of rebuilding all of them.
    pub(super) fn sync_current_to_panels(&self) {
        if let Some(idx) = self.queue.current_index() {
            self.playlist
                .emit(PlaylistPanelInput::ScrollToCurrent { idx });
        }
        self.sync_now_playing();
    }

    /// Re-light the now-playing indicator in both panels from the queue's
    /// current track (matched by file path in the library detail view), while
    /// it plays or is paused. While a radio stream plays, nothing in the
    /// queue/library is "playing", so clear it.
    pub(super) fn sync_now_playing(&self) {
        let status = *self
            .mpris_state
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let playback = if self.is_stream {
            PlaybackStatus::Stopped
        } else if self.is_playing {
            PlaybackStatus::Playing
        } else if status == PlaybackStatus::Paused {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Stopped
        };
        let current = (playback != PlaybackStatus::Stopped)
            .then(|| self.queue.current_index())
            .flatten();
        self.playlist.emit(PlaylistPanelInput::SetPlaying {
            idx: current,
            playback,
        });
        let path = current
            .and_then(|_| self.queue.current())
            .map(|track| track.file_path.clone());
        self.library
            .emit(LibraryPanelInput::SetPlayingPath { path, playback });
    }

    /// Refresh the sidebar-bottom now-playing card from the current track (or a
    /// radio stream). Hidden when nothing is loaded.
    pub(super) fn sync_now_playing_card(&self) {
        if self.is_stream {
            if self.radio_station_name.is_empty() {
                self.np_box.set_visible(false);
                return;
            }
            self.np_box.set_visible(true);
            self.np_title.set_label(&self.radio_station_name);
            self.np_artist.set_label(&i18n("Radio"));
            self.np_album.set_visible(false);
            self.np_art.set_icon_name(Some("icon_radio"));
            return;
        }
        let Some(track) = self.queue.current() else {
            self.np_box.set_visible(false);
            return;
        };
        self.np_box.set_visible(true);
        self.np_title.set_label(track.display_title());
        self.np_artist.set_label(track.display_artist());
        let album = track.album.clone().unwrap_or_default();
        self.np_album.set_label(&album);
        self.np_album.set_visible(!album.is_empty());
    }

    /// The top-level window (resolved via a mounted child widget).
    pub(super) fn window(&self) -> Option<gtk::Window> {
        self.playback_bar
            .widget()
            .root()
            .and_downcast::<gtk::Window>()
    }

    /// Parent for feedback raised by a playlist-library action. That library is
    /// its own modal `adw::Window`, so an alert parented to the main window is
    /// created and announced but stays underneath it, unseen and undismissable.
    pub(super) fn playlist_library_parent(&self) -> Option<gtk::Window> {
        let library = self.playlist_library_dialog.widget();
        if library.is_visible() {
            return Some(library.clone().upcast());
        }
        self.window()
    }

    /// Bundled illustration loader (audio-player gresource path).
    pub(super) fn illustration(filename: &str) -> gtk::Image {
        crate::shared::illustration::illustration(
            "/br/com/biglinux/AudioPlayer/illustrations/",
            filename,
        )
    }

    /// "Play on a TV": hand the current track, at its current position, to a
    /// Chromecast or DLNA TV, pausing it here once the TV plays.
    pub(super) fn show_tv_dialog(&self) {
        use crate::cast::daemon::PlaybackStart;
        use crate::shared::tv_cast::{TvMedia, present_tv_dialog};
        let Some(win) = self.window() else {
            return;
        };
        let mpv = self.mpv.clone();
        let current = mpv.clone();
        let file = self.current_file.clone();
        let media = move || {
            (!file.is_empty()).then(|| TvMedia {
                source: file.clone(),
                title: current
                    .as_ref()
                    .and_then(|mpv| mpv.get_media_title())
                    .unwrap_or_default(),
                audio: true,
                start: current
                    .as_ref()
                    .map_or_else(PlaybackStart::default, |mpv| PlaybackStart {
                        seconds: mpv.get_position(),
                        paused: false,
                        volume: (mpv.get_volume() / 100.0).clamp(0.0, 1.0),
                        muted: mpv.get_mute(),
                        height: 0,
                    }),
            })
        };
        present_tv_dialog(&win, &self.tv, &self.settings, media, move || {
            if let Some(mpv) = &mpv {
                mpv.pause();
            }
        });
    }

    /// Present the sleep-timer dialog: the active countdown (with cancel) when a
    /// timer is armed, otherwise the setup page (duration + on-expiry action).
    pub(super) fn show_sleep_timer(&self, sender: &ComponentSender<Self>) {
        use big_relm4_components::display::sleep_timer::{
            BigSleepTimerActiveLabels, BigSleepTimerActivePage, BigSleepTimerActiveSpec,
            BigSleepTimerSetupPage, BigSleepTimerSetupSpec, connect_sleep_timer_start_with_close,
        };
        use big_relm4_components::layout::dialog_page::BigDialogPage;

        let Some(win) = self.window() else {
            return;
        };
        let active = self.sleep_timer.is_armed();

        let dialog = big_gtk_kit::dialog::Dialog::builder()
            .title(i18n("Sleep Timer"))
            .content_width(600)
            .content_height(if active { 420 } else { 560 })
            .build();

        let page = if active {
            let remaining = self.sleep_timer.remaining_secs();
            let active_page = BigSleepTimerActivePage::new(
                BigSleepTimerActiveSpec {
                    labels: BigSleepTimerActiveLabels {
                        title: i18n("Timer Active"),
                        cancel_button: i18n("Cancel Timer"),
                    },
                    remaining_seconds: remaining,
                    icon_size: 80,
                },
                Some(Self::illustration("sleep_timer.svg").upcast()),
            );
            active_page.start_preview_countdown(remaining);

            let input = sender.input_sender().clone();
            let dialog_ref = dialog.downgrade();
            active_page.cancel_button().connect_clicked(move |_| {
                let _ = input.send(AudioShellInput::SleepTimerCancel);
                if let Some(d) = dialog_ref.upgrade() {
                    d.close();
                }
            });
            active_page.into_page()
        } else {
            let (saved_mins, saved_action) =
                crate::shared::sleep_timer::load_sleep_timer_preferences(Some(&self.settings));
            let setup_page = BigSleepTimerSetupPage::new(
                BigSleepTimerSetupSpec {
                    labels: crate::shared::sleep_timer::sleep_timer_setup_labels(),
                    actions: crate::shared::sleep_timer::sleep_timer_actions(),
                    saved_minutes: saved_mins,
                    saved_action_id: saved_action,
                    icon_size: 72,
                },
                Some(Self::illustration("sleep_timer.svg").upcast()),
                Some(Self::illustration("timer_clock.svg").upcast()),
                Some(Self::illustration("sleep_action.svg").upcast()),
            );

            let input = sender.input_sender().clone();
            let settings = self.settings.clone();
            let dialog_ref = dialog.downgrade();
            connect_sleep_timer_start_with_close(
                &setup_page,
                &win,
                move |_, minutes, action| {
                    crate::shared::sleep_timer::save_sleep_timer_preferences(
                        Some(&settings),
                        minutes,
                        &action,
                    );
                    let _ = input.send(AudioShellInput::SleepTimerStart { minutes, action });
                },
                move || {
                    if let Some(dialog) = dialog_ref.upgrade() {
                        dialog.close();
                    }
                },
            );
            setup_page.into_page()
        };

        let dialog_page = BigDialogPage::scrolled_clamped(560, 480);
        dialog_page.content().append(&page);
        dialog.set_child(Some(&dialog_page.into_root()));
        dialog.present(Some(&win));
    }

    /// Show the read-only track metadata dialog (reuses the shared media dialog).
    pub(super) fn show_track_metadata_dialog(&self, track: &Track) {
        let Some(win) = self.window() else {
            return;
        };
        let labels = BigMediaFileMetadataDialogLabels::translated(i18n);
        let spec = BigMediaFileMetadataDialogSpec::new(PathBuf::from(&track.file_path), labels)
            .duration_secs(track.duration_secs);
        show_media_file_metadata_dialog(&win, spec);
    }
}
