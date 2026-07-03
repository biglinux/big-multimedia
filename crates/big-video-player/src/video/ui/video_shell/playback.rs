use super::*;

impl VideoShell {
    /// Reflect an mpv event in the transport display + now-playing title.
    pub(super) fn on_player_event(
        &mut self,
        event: PlayerEvent,
        root: Option<&adw::ApplicationWindow>,
    ) {
        match event {
            PlayerEvent::PositionChanged(pos) => {
                self.controls
                    .emit(VideoControlsBarInput::SetPosition(playback_time(pos)));
                if let Some(frac) = seek_fraction(pos, self.duration_secs) {
                    self.controls
                        .emit(VideoControlsBarInput::SetSeekFraction(frac));
                }
                self.feed_trim_playhead(pos);
                self.apply_play_selection(pos);
            }
            PlayerEvent::DurationChanged(dur) => {
                self.duration_secs = dur;
                self.controls
                    .emit(VideoControlsBarInput::SetDuration(playback_time(dur)));
                self.controls
                    .emit(VideoControlsBarInput::SetDurationSecs(dur));
                self.feed_trim_duration(dur);
            }
            PlayerEvent::PauseChanged(paused) => {
                self.controls.emit(VideoControlsBarInput::SetPaused(paused));
                // Old "keep shown while paused": reveal the OSD when paused, and
                // re-arm the auto-hide when resuming.
                self.osd.is_paused.set(paused);
                if !self.mode.is_background() {
                    if paused {
                        self.osd.show();
                    } else {
                        self.osd.show_and_schedule_hide();
                    }
                }
            }
            PlayerEvent::FileLoaded(_title) => {
                self.controls
                    .emit(VideoControlsBarInput::SetControlsSensitive(true));
                // Hand the loaded *path* to the controls bar so the seek-hover
                // preview can render thumbnail frames (local files only). The
                // `FileLoaded` payload is the media TITLE, not the path — read
                // the real path from mpv (`path` property).
                let current_file = self
                    .mpv
                    .borrow()
                    .as_ref()
                    .and_then(|mpv| mpv.get_current_file());
                // Stream Quality (yt-dlp) only applies to online/streamed media.
                let is_online = current_file.as_deref().is_some_and(is_remote_uri);
                super::actions::set_action_enabled(&self.win_actions, "ytdl-quality", is_online);
                let preview_path = current_file.filter(|p| !is_remote_uri(p));
                self.controls
                    .emit(VideoControlsBarInput::SetCurrentFile(preview_path));
                self.hide_welcome_screen();
                // First frame: drop the idle gradient and restore the gl surface
                // (load-bearing — video would stay invisible otherwise), then
                // let the OSD auto-hide.
                if let Some(display) = gtk::gdk::Display::default() {
                    self.idle_bg.hide(&display);
                }
                self.osd.is_welcome_visible.set(false);
                if !self.mode.is_background() {
                    self.osd.show_and_schedule_hide();
                }
                self.refresh_now_playing(root);
                // Resize the window to the video's aspect (no letterbox bars).
                if !self.mode.is_background() {
                    self.aspect_snap.snap_on_load();
                }
            }
            PlayerEvent::Buffering(buffering) => {
                self.buffering_overlay.set_visible(buffering);
            }
            PlayerEvent::TracksChanged(tracks) => {
                self.controls.emit(VideoControlsBarInput::SetTracks(tracks));
            }
            PlayerEvent::ChaptersChanged(chapters) => {
                self.controls
                    .emit(VideoControlsBarInput::SetChapters(chapters));
            }
            PlayerEvent::MetadataChanged(_) => self.refresh_now_playing(root),
            _ => {}
        }
    }

    /// Set the window title to the playing media's title and re-light the
    /// sidebar row whose path matches the loaded file.
    fn refresh_now_playing(&mut self, root: Option<&adw::ApplicationWindow>) {
        let mpv_ref = self.mpv.borrow();
        let Some(mpv) = mpv_ref.as_ref() else {
            return;
        };
        if let (Some(root), Some(title)) = (root, mpv.get_media_title().filter(|t| !t.is_empty())) {
            root.set_title(Some(&title));
        }
        let current = mpv.get_current_file();
        drop(mpv_ref);
        self.playlist
            .emit(VideoBrowserSidebarInput::SetCurrentFile(current));
    }

    /// Open `paths`, matching the old folder-scan / playlist semantics:
    ///   • single playlist file (`.m3u`/`.pls`) → parse it into the playlist;
    ///   • single media file → scan its folder flat, start at that file;
    ///   • multiple → keep the media ones, sorted.
    /// The resulting list replaces the mpv playlist; the first entry plays.
    ///
    /// The folder scan / playlist-file parse are blocking disk I/O, so they run
    /// off the GTK main loop; the resolved list is dispatched back as
    /// [`VideoShellInput::PlaylistResolved`] and applied in [`Self::apply_open`].
    pub(super) fn open_files(paths: &[String], sender: &ComponentSender<VideoShell>) {
        if paths.is_empty() {
            return;
        }
        let paths = paths.to_vec();
        let sender = sender.clone();
        gtk::glib::spawn_future_local(async move {
            let resolved = gtk::gio::spawn_blocking(move || resolve_open_files(paths))
                .await
                .ok()
                .flatten();
            if let Some((entries, start, folder)) = resolved {
                sender.input(VideoShellInput::PlaylistResolved {
                    entries,
                    start,
                    folder,
                });
            }
        });
    }

    /// Apply a resolved open request on the main thread: load the playlist into
    /// mpv and push the listing to the sidebar.
    pub(super) fn apply_open(
        &mut self,
        entries: Vec<String>,
        start: usize,
        folder: Option<String>,
    ) {
        if entries.is_empty() {
            return;
        }
        self.load_playlist(&entries, start);
        self.playlist.emit(VideoBrowserSidebarInput::SetPlaylist {
            paths: entries,
            folder,
        });
    }

    /// Load a resolved playlist into mpv in natural order and start at `start`
    /// (so prev/next traverse the whole list and sidebar indices align with
    /// mpv's). No-op until mpv is live. Tracks `playlist_paths` for later
    /// in-place jumps and removals; does NOT push the sidebar listing — the
    /// caller does that (either an external open or a sidebar `PlayFromList`).
    fn load_playlist(&mut self, entries: &[String], start: usize) {
        let mpv_ref = self.mpv.borrow();
        let Some(mpv) = mpv_ref.as_ref() else {
            return;
        };
        let Some(first) = entries.first() else {
            return;
        };
        if self.mode.is_background() {
            mpv.set_panscan(1.0);
        }
        mpv.load_file(first);
        for path in &entries[1..] {
            mpv.load_file_append(path);
        }
        if needs_explicit_start_index(start) {
            mpv.playlist_play_index(start as i64);
        }
        mpv.play();
        drop(mpv_ref);
        self.playlist_paths = entries.to_vec();
        // The sidebar stays hidden until the header playlist toggle opens it
        // (matches the old design).
    }

    /// Hide + drop the welcome overlay (called on the first `FileLoaded`).
    fn hide_welcome_screen(&self) {
        if let Some(overlay) = self.welcome_overlay.borrow_mut().take() {
            self.main_overlay.remove_overlay(&overlay);
        }
    }

    /// Drive mpv from a folder-browser-sidebar intent.
    pub(super) fn on_playlist(&mut self, out: VideoBrowserSidebarOutput) {
        match out {
            VideoBrowserSidebarOutput::PlayFromList { paths, start } => {
                // Same list already loaded → jump in place; else reload mpv.
                if is_same_playlist(&paths, &self.playlist_paths) {
                    if let Some(mpv) = self.mpv.borrow().as_ref() {
                        mpv.playlist_play_index(start as i64);
                    }
                } else {
                    self.load_playlist(&paths, start);
                }
            }
            VideoBrowserSidebarOutput::Enqueue(paths) => self.enqueue_paths(&paths),
            VideoBrowserSidebarOutput::RemoveEntry(path) => self.remove_playlist_entry(&path),
            VideoBrowserSidebarOutput::Clear => {
                if let Some(mpv) = self.mpv.borrow().as_ref() {
                    mpv.playlist_clear();
                }
                self.playlist_paths.clear();
            }
            VideoBrowserSidebarOutput::SendToConverter(paths) => self.send_to_converter(&paths),
            VideoBrowserSidebarOutput::TrashFiles(paths) => self.trash_files(paths),
            VideoBrowserSidebarOutput::Notify(msg) => self.toast(&msg),
        }
    }

    /// Show a transient toast on the player's toast overlay.
    fn toast(&self, message: &str) {
        self.toast_overlay.add_toast(adw::Toast::new(message));
    }

    /// Launch big-video-converter with the chosen files (it accepts files via
    /// `HANDLES_OPEN`, so an already-running instance enqueues them).
    fn send_to_converter(&self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }
        // argv array (no shell); paths are existing local files from the scan.
        let spawned = big_app_kit::subprocess::BigSubprocessSpec::builder()
            .program("big-video-converter")
            .args(paths.iter().cloned())
            .build()
            .spawn_detached();
        if spawned.is_ok() {
            self.toast(&i18n("Opening in converter…"));
        } else {
            self.toast(&i18n("Could not launch the converter"));
        }
    }

    /// Move files to the trash, drop any from the queue, refresh the browser,
    /// and offer an Undo toast that restores them. The `gio trash` calls run off
    /// the main thread (they spawn a subprocess per file).
    fn trash_files(&mut self, paths: Vec<String>) {
        // Queue cleanup is instant (in-memory + a fast mpv ipc) — do it now.
        for path in &paths {
            if let Some(i) = playlist_index_of(&self.playlist_paths, path) {
                if let Some(mpv) = self.mpv.borrow().as_ref() {
                    mpv.playlist_remove(i as i64);
                }
                self.playlist_paths.remove(i);
            }
        }

        let toast_overlay = self.toast_overlay.clone();
        let pl_sender = self.playlist.sender().clone();
        let to_trash = paths.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || {
                use big_media_runtime::media::playlist;
                to_trash
                    .into_iter()
                    .filter(|p| playlist::trash_file(p).is_ok())
                    .collect::<Vec<String>>()
            },
            move |trashed: Vec<String>| {
                if trashed.is_empty() {
                    toast_overlay.add_toast(adw::Toast::new(&i18n("Could not move to trash")));
                    return;
                }
                let _ = pl_sender.send(VideoBrowserSidebarInput::RefreshFolder);
                let title = ni18n(
                    "{} file moved to trash",
                    "{} files moved to trash",
                    trashed.len() as u64,
                )
                .replace("{}", &trashed.len().to_string());
                let toast = adw::Toast::builder()
                    .title(title)
                    .button_label(i18n("Undo"))
                    .build();
                let restore_sender = pl_sender.clone();
                toast.connect_button_clicked(move |_| {
                    let restore_sender = restore_sender.clone();
                    let to_restore = trashed.clone();
                    big_media_components::player_chrome::spawn_blocking_result(
                        move || {
                            use big_media_runtime::media::playlist;
                            for path in &to_restore {
                                let _ = playlist::restore_from_trash(path);
                            }
                        },
                        move |()| {
                            let _ = restore_sender.send(VideoBrowserSidebarInput::RefreshFolder);
                        },
                    );
                });
                toast_overlay.add_toast(toast);
            },
        );
    }

    /// Append paths to the mpv playlist without disrupting playback; if nothing
    /// is loaded yet, start them. Pushes the updated queue back to the sidebar
    /// (which reveals Playlist mode).
    fn enqueue_paths(&mut self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }
        if self.playlist_paths.is_empty() {
            self.load_playlist(paths, 0);
        } else if let Some(mpv) = self.mpv.borrow().as_ref() {
            for path in paths {
                mpv.load_file_append(path);
            }
            self.playlist_paths.extend_from_slice(paths);
        } else {
            return;
        }
        self.playlist.emit(VideoBrowserSidebarInput::SetPlaylist {
            paths: self.playlist_paths.clone(),
            folder: None,
        });
    }

    /// Remove `path` from the mpv playlist (the sidebar already dropped it from
    /// its own listing). No-op if the listing has since diverged from mpv.
    fn remove_playlist_entry(&mut self, path: &str) {
        if let Some(i) = playlist_index_of(&self.playlist_paths, path) {
            if let Some(mpv) = self.mpv.borrow().as_ref() {
                mpv.playlist_remove(i as i64);
            }
            self.playlist_paths.remove(i);
        }
    }

    /// Route an IPTV browser intent: play the selected channel URL through the
    /// existing playback path (replaces the playlist with the single stream).
    pub(super) fn on_iptv(out: VideoIptvBrowserOutput, sender: &ComponentSender<VideoShell>) {
        match out {
            VideoIptvBrowserOutput::PlayChannel { url, .. } => {
                Self::open_files(std::slice::from_ref(&url), sender);
            }
        }
    }

    /// React to a cast-dialog intent: a cast started → pause local playback so
    /// audio/video does not double up with the casting device; stopped → no-op
    /// for now (local playback stays as the user left it).
    #[cfg(feature = "chromecast")]
    pub(super) fn on_cast(&self, out: VideoCastDialogOutput) {
        match out {
            VideoCastDialogOutput::Started { .. } => {
                if let Some(mpv) = self.mpv.borrow().as_ref() {
                    if !mpv.is_paused() {
                        mpv.toggle_pause();
                    }
                }
            }
            VideoCastDialogOutput::Stopped => {}
        }
    }

    /// Drive mpv from a controls-bar intent.
    pub(super) fn on_controls(
        &self,
        out: VideoControlsBarOutput,
        root: Option<&adw::ApplicationWindow>,
    ) {
        if let VideoControlsBarOutput::ToggleFullscreen = out {
            if let Some(root) = root {
                root.set_fullscreened(!root.is_fullscreen());
            }
            return;
        }
        if let VideoControlsBarOutput::GotoTimeClicked = out {
            if let Some(root) = root {
                self.show_goto_time_dialog(root);
            }
            return;
        }
        if let VideoControlsBarOutput::SegmentEdgeAdjusted { id, start, end } = out {
            self.adjust_trim_segment(id, start, end);
            return;
        }
        let mpv_ref = self.mpv.borrow();
        let Some(mpv) = mpv_ref.as_ref() else {
            return;
        };
        match out {
            VideoControlsBarOutput::TogglePlay => mpv.toggle_pause(),
            VideoControlsBarOutput::Next => mpv.playlist_next(),
            VideoControlsBarOutput::Previous => mpv.playlist_prev(),
            VideoControlsBarOutput::SeekChanged(frac) => {
                if let Some(target) = seek_target_secs(frac, self.duration_secs) {
                    mpv.seek_absolute(target);
                }
            }
            // Handled before the mpv borrow above (early return).
            VideoControlsBarOutput::ToggleFullscreen
            | VideoControlsBarOutput::GotoTimeClicked
            | VideoControlsBarOutput::SegmentEdgeAdjusted { .. } => {}
        }
    }

    /// Pause/resume video playback when a pane host hides or reveals the
    /// desktop content. Auto-resume only undoes a pause caused by this method.
    pub(super) fn set_pane_activity(&self, activity: big_relm4_components::pane::PaneActivity) {
        let Some(mpv) = self.mpv.borrow().as_ref().cloned() else {
            return;
        };
        match pane_activity_action(
            activity,
            self.activity_paused_playback.get(),
            mpv.is_paused(),
        ) {
            PaneAction::Resume => {
                self.activity_paused_playback.set(false);
                mpv.play();
            }
            PaneAction::PauseAndMark => {
                self.activity_paused_playback.set(true);
                mpv.pause();
            }
            PaneAction::PauseAndClear => {
                self.activity_paused_playback.set(false);
                mpv.pause();
            }
            PaneAction::Nothing => {}
        }
    }
}

/// Side effect `set_pane_activity` should apply, decided purely from the pane
/// signal + whether *this* method previously auto-paused + mpv's pause state.
#[derive(Debug, PartialEq, Eq)]
enum PaneAction {
    /// Pane visible again and we auto-paused it earlier → resume + clear the mark.
    Resume,
    /// Pane occluded/hidden/on-battery while playing → pause + mark as ours.
    PauseAndMark,
    /// User explicitly paused via the pane → pause + drop any auto-pause mark.
    PauseAndClear,
    /// Leave playback untouched.
    Nothing,
}

/// Pure decision for [`VideoShell::set_pane_activity`]: only auto-resume a pause
/// this method caused, and only auto-pause content that is currently playing.
fn pane_activity_action(
    activity: big_relm4_components::pane::PaneActivity,
    auto_paused_by_us: bool,
    mpv_is_paused: bool,
) -> PaneAction {
    use big_relm4_components::pane::PaneActivity;
    match activity {
        PaneActivity::Visible if auto_paused_by_us => PaneAction::Resume,
        PaneActivity::Occluded | PaneActivity::Hidden | PaneActivity::OnBattery
            if !mpv_is_paused =>
        {
            PaneAction::PauseAndMark
        }
        PaneActivity::UserPaused => PaneAction::PauseAndClear,
        _ => PaneAction::Nothing,
    }
}

/// Resolve an open request to `(entries, start, folder)` off the main thread.
/// Pure + `Send` (captures only `String`s); the blocking folder scan / playlist
/// parse run here. Returns `None` when nothing playable was found.
fn resolve_open_files(paths: Vec<String>) -> Option<(Vec<String>, usize, Option<String>)> {
    use big_media_runtime::media::playlist;

    if paths.is_empty() {
        return None;
    }

    if paths.len() == 1 && playlist::is_playlist_file(&paths[0]) {
        let parsed = playlist::parse_playlist_file(&paths[0]);
        if parsed.is_empty() {
            return None;
        }
        return Some((parsed, 0, None));
    }

    if paths.len() == 1 && !is_remote_uri(&paths[0]) {
        // Single local file → scan its folder, start at this file, and show that
        // folder as the sidebar's browse context.
        return match std::path::Path::new(&paths[0]).parent() {
            Some(parent) => {
                let folder = playlist::scan_folder_flat(parent);
                let pos = folder.iter().position(|p| p == &paths[0]).unwrap_or(0);
                if folder.is_empty() {
                    Some((vec![paths[0].clone()], 0, None))
                } else {
                    Some((folder, pos, Some(parent.to_string_lossy().into_owned())))
                }
            }
            None => Some((vec![paths[0].clone()], 0, None)),
        };
    }

    if paths.len() == 1 {
        // Single remote URI / stream → just that entry.
        return Some((vec![paths[0].clone()], 0, None));
    }

    let mut sorted: Vec<String> = paths
        .iter()
        .filter(|p| playlist::is_media_file(p))
        .cloned()
        .collect();
    sorted.sort_by_key(|a| a.to_lowercase());
    if sorted.is_empty() {
        return None;
    }
    Some((sorted, 0, None))
}

pub(super) fn is_remote_uri(s: &str) -> bool {
    let s = s.trim();
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("rtsp://")
        || s.starts_with("rtmp://")
        || s.starts_with("dvd://")
        || s.starts_with("bd://")
}

/// Seek-bar fraction for `pos` within a clip of `duration` seconds, clamped to
/// `[0, 1]`. `None` when the duration is still unknown (zero or negative), so
/// the caller leaves the seek bar untouched.
fn seek_fraction(pos: f64, duration: f64) -> Option<f64> {
    if duration > 0.0 {
        Some((pos / duration).clamp(0.0, 1.0))
    } else {
        None
    }
}

/// Absolute seek target (seconds) for a `[0,1]` seek-bar `frac`, or `None` while
/// the duration is unknown (so the caller leaves mpv alone).
fn seek_target_secs(frac: f64, duration: f64) -> Option<f64> {
    (duration > 0.0).then_some(frac * duration)
}

/// Whether a resolved playlist needs an explicit `playlist_play_index` after
/// loading — only when the start is past the first entry (index 0 already plays).
fn needs_explicit_start_index(start: usize) -> bool {
    start > 0
}

/// Whether the incoming list is the one already loaded into mpv (jump in place
/// instead of reloading).
fn is_same_playlist(incoming: &[String], loaded: &[String]) -> bool {
    incoming == loaded
}

/// Index of `target` within the tracked mpv playlist, if present.
fn playlist_index_of(paths: &[String], target: &str) -> Option<usize> {
    paths.iter().position(|p| p.as_str() == target)
}

#[cfg(test)]
mod tests {
    use super::{is_remote_uri, seek_fraction};

    #[test]
    fn seek_fraction_none_without_known_duration() {
        assert_eq!(seek_fraction(5.0, 0.0), None);
        assert_eq!(seek_fraction(5.0, -1.0), None);
    }

    #[test]
    fn seek_fraction_maps_and_clamps() {
        assert_eq!(seek_fraction(0.0, 100.0), Some(0.0));
        assert_eq!(seek_fraction(50.0, 100.0), Some(0.5));
        assert_eq!(seek_fraction(100.0, 100.0), Some(1.0));
        // Position past the end clamps to 1.0; before start clamps to 0.0.
        assert_eq!(seek_fraction(150.0, 100.0), Some(1.0));
        assert_eq!(seek_fraction(-10.0, 100.0), Some(0.0));
    }

    #[test]
    fn remote_uri_detects_streaming_and_disc_schemes() {
        for uri in [
            "http://h/v.mp4",
            "https://h/v.mp4",
            "rtsp://h/s",
            "rtmp://h/s",
            "dvd://1",
            "bd://2",
            "  https://trimmed ",
        ] {
            assert!(is_remote_uri(uri), "{uri} should be remote");
        }
    }

    #[test]
    fn remote_uri_rejects_local_paths() {
        for path in ["/home/u/v.mkv", "file:///v.mp4", "video.mp4", "ftp://h/x"] {
            assert!(!is_remote_uri(path), "{path} should be local");
        }
    }
}

#[cfg(test)]
mod resolve_open_files_tests {
    use super::resolve_open_files;
    #[cfg(not(miri))]
    use std::fs;
    #[cfg(not(miri))]
    use std::path::Path;

    #[cfg(not(miri))]
    fn touch(dir: &Path, name: &str) -> String {
        let path = dir.join(name);
        fs::write(&path, b"x").unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn empty_input_resolves_to_nothing() {
        assert_eq!(resolve_open_files(vec![]), None);
    }

    #[test]
    fn single_remote_uri_passes_through_without_folder_context() {
        // Not a playlist, not local → the bare entry, no browse context.
        assert_eq!(
            resolve_open_files(vec!["rtsp://host/stream".to_string()]),
            Some((vec!["rtsp://host/stream".to_string()], 0, None))
        );
    }

    #[test]
    fn multiple_inputs_keep_only_media_sorted_case_insensitively() {
        let out = resolve_open_files(vec![
            "/x/B.mp4".to_string(),
            "/x/a.mkv".to_string(),
            "/x/notes.txt".to_string(),
        ]);
        assert_eq!(
            out,
            Some((
                vec!["/x/a.mkv".to_string(), "/x/B.mp4".to_string()],
                0,
                None
            ))
        );
    }

    #[test]
    fn multiple_non_media_inputs_resolve_to_nothing() {
        assert_eq!(
            resolve_open_files(vec!["/x/a.txt".to_string(), "/x/b.doc".to_string()]),
            None
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn single_local_file_scans_its_folder_and_locates_the_opened_file() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "a.mp4");
        let opened = touch(dir.path(), "b.mp4");
        touch(dir.path(), "c.mp4");
        let (folder, pos, context) = resolve_open_files(vec![opened.clone()]).unwrap();
        assert_eq!(folder.len(), 3, "all siblings scanned: {folder:?}");
        // Position must index the opened file specifically (sorted a,b,c → 1).
        assert_eq!(pos, 1);
        assert_eq!(folder[pos], opened);
        assert_eq!(
            context.as_deref(),
            Some(dir.path().to_string_lossy().as_ref())
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn single_playlist_returns_its_entries_without_context() {
        let dir = tempfile::tempdir().unwrap();
        let a = touch(dir.path(), "a.mp4");
        let b = touch(dir.path(), "b.mp4");
        let playlist = dir.path().join("list.m3u8");
        fs::write(&playlist, format!("#EXTM3U\n{a}\n{b}\n")).unwrap();
        let (entries, pos, context) =
            resolve_open_files(vec![playlist.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(entries.len(), 2, "both playlist entries kept: {entries:?}");
        assert_eq!(pos, 0);
        assert_eq!(context, None);
    }

    #[cfg(not(miri))]
    #[test]
    fn single_empty_playlist_resolves_to_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let playlist = dir.path().join("empty.m3u8");
        fs::write(&playlist, "#EXTM3U\n").unwrap();
        assert_eq!(
            resolve_open_files(vec![playlist.to_string_lossy().into_owned()]),
            None
        );
    }
}

#[cfg(test)]
mod dispatch_logic_tests {
    use super::{
        is_same_playlist, needs_explicit_start_index, pane_activity_action, playlist_index_of,
        seek_target_secs, PaneAction,
    };
    use big_relm4_components::pane::PaneActivity;

    #[test]
    fn seek_target_none_until_duration_known() {
        assert_eq!(seek_target_secs(0.5, 0.0), None);
        assert_eq!(seek_target_secs(0.5, -1.0), None);
    }

    #[test]
    fn seek_target_scales_fraction_by_duration() {
        assert_eq!(seek_target_secs(0.0, 200.0), Some(0.0));
        assert_eq!(seek_target_secs(0.25, 200.0), Some(50.0));
        assert_eq!(seek_target_secs(1.0, 200.0), Some(200.0));
    }

    #[test]
    fn explicit_start_index_only_past_the_first_entry() {
        assert!(!needs_explicit_start_index(0));
        assert!(needs_explicit_start_index(1));
        assert!(needs_explicit_start_index(9));
    }

    #[test]
    fn same_playlist_compares_full_lists() {
        let a = vec!["x".to_string(), "y".to_string()];
        assert!(is_same_playlist(&a, &a.clone()));
        assert!(!is_same_playlist(&a, &["x".to_string()]));
        assert!(!is_same_playlist(&a, &["y".to_string(), "x".to_string()]));
    }

    #[test]
    fn playlist_index_locates_only_exact_matches() {
        let paths = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(playlist_index_of(&paths, "a"), Some(0));
        assert_eq!(playlist_index_of(&paths, "c"), Some(2));
        assert_eq!(playlist_index_of(&paths, "z"), None);
        assert_eq!(playlist_index_of(&[], "a"), None);
    }

    #[test]
    fn pane_resume_only_when_we_auto_paused() {
        assert_eq!(
            pane_activity_action(PaneActivity::Visible, true, true),
            PaneAction::Resume
        );
        // Visible but not our pause → leave it (kills the guard-flip mutant).
        assert_eq!(
            pane_activity_action(PaneActivity::Visible, false, true),
            PaneAction::Nothing
        );
    }

    #[test]
    fn pane_auto_pauses_only_playing_content_when_backgrounded() {
        for activity in [
            PaneActivity::Occluded,
            PaneActivity::Hidden,
            PaneActivity::OnBattery,
        ] {
            assert_eq!(
                pane_activity_action(activity, false, false),
                PaneAction::PauseAndMark,
                "{activity:?} while playing must auto-pause"
            );
            // Already paused → nothing to do (kills the `!is_paused` guard mutants).
            assert_eq!(
                pane_activity_action(activity, false, true),
                PaneAction::Nothing,
                "{activity:?} already paused must be a no-op"
            );
        }
    }

    #[test]
    fn pane_user_pause_always_pauses_and_clears() {
        assert_eq!(
            pane_activity_action(PaneActivity::UserPaused, false, false),
            PaneAction::PauseAndClear
        );
        assert_eq!(
            pane_activity_action(PaneActivity::UserPaused, true, true),
            PaneAction::PauseAndClear
        );
    }
}
