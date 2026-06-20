use super::input::shell_input_sender;
use super::playback::is_remote_uri;
use super::*;

impl VideoShell {
    /// Present the cast device picker in a dialog (same mount/unparent pattern
    /// as the IPTV browser: the controller's widget is long-lived).
    #[cfg(feature = "chromecast")]
    pub(super) fn show_cast_dialog(&self, root: &adw::ApplicationWindow) {
        present_controller_dialog(self.cast.widget(), &i18n("Cast to Device"), 400, 420, root);
        // Restart discovery + the poll timer (the timer stops on close).
        self.cast
            .emit(crate::video::ui::cast::VideoCastDialogInput::Reopened);
    }

    /// Launch a fresh filters dialog bound to the live mpv backend and present
    /// it. Requires mpv to be up (the dialog reads/writes the filter chain); a
    /// no-op before the surface realizes. Launched per-open and `.detach()`ed —
    /// it owns no playback state, so a new instance each time is clean.
    pub(super) fn show_filters_dialog(&self, root: &adw::ApplicationWindow) {
        let Some(mpv) = self.mpv.borrow().as_ref().map(Arc::clone) else {
            return;
        };
        #[cfg(feature = "chromecast")]
        let on_filter_changed = {
            let cast_sync = self.cast.model().filter_sync();
            let snapshot_mpv = self.mpv.clone();
            let snapshot_settings = self.settings.clone();
            let snapshot_transform = self.video_transform.clone();
            Rc::new(move || {
                let filters = super::cast_snapshot::build_cast_filter_snapshot(
                    &snapshot_mpv,
                    &snapshot_settings,
                    &snapshot_transform,
                );
                let position_secs = snapshot_mpv
                    .borrow()
                    .as_ref()
                    .map_or(0.0, |mpv| mpv.get_position());
                cast_sync(filters, position_secs);
            })
        };
        #[cfg(not(feature = "chromecast"))]
        let on_filter_changed = Rc::new(|| {});
        let dialog = VideoFiltersDialog::builder()
            .launch(VideoFiltersDialogInit {
                mpv,
                settings: self.settings.clone(),
                on_filter_changed,
            })
            .detach();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(dialog.widget()));
        let adw_dialog = adw::Dialog::builder()
            .title(i18n("Video Filters"))
            .content_width(400)
            .content_height(560)
            .child(&toolbar)
            .build();
        // Own the controller for the dialog's lifetime: a `Cell` lets the `Fn`
        // close handler drop it (taking the runtime down) when the dialog goes
        // away, without requiring an `FnMut`.
        let held = std::cell::Cell::new(Some(dialog));
        adw_dialog.connect_closed(move |_| {
            held.take();
        });
        adw_dialog.present(Some(root));
    }

    /// Present the audio equalizer dialog (shared dialog, persists EQ bands via
    /// `Settings`, applies to the live mpv backend if up).
    pub(super) fn show_equalizer_dialog(&self, root: &adw::ApplicationWindow) {
        let mpv = self.mpv.borrow().as_ref().map(Arc::clone);
        media_adjust::present_equalizer_dialog(&self.settings, mpv, root);
    }

    /// Present the audio & subtitle delay dialog. No-op before mpv is up (the
    /// sliders drive the live backend directly).
    pub(super) fn show_delay_dialog(&self, root: &adw::ApplicationWindow) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            media_adjust::present_delay_dialog(mpv, root);
        }
    }

    /// Present the image & video adjustment dialog. No-op before mpv is up (the
    /// sliders read current mpv values and drive the live backend directly).
    pub(super) fn show_image_adjust_dialog(&self, root: &adw::ApplicationWindow) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            media_adjust::present_image_adjust_dialog(mpv, &self.video_transform, root);
        }
    }

    pub(super) fn show_ytdl_quality_dialog(&self, root: &adw::ApplicationWindow) {
        let Some(mpv) = self.mpv.borrow().as_ref().map(Arc::clone) else {
            return;
        };
        let group = adw::PreferencesGroup::new();
        let choices = [
            (
                "Best available",
                "bestvideo*+bestaudio/best",
                "Highest video and audio quality",
            ),
            (
                "Up to 1080p",
                "bestvideo*[height<=1080]+bestaudio/best[height<=1080]/best",
                "Prefer Full HD or lower",
            ),
            (
                "Up to 720p",
                "bestvideo*[height<=720]+bestaudio/best[height<=720]/best",
                "Prefer HD or lower",
            ),
            (
                "Up to 480p",
                "bestvideo*[height<=480]+bestaudio/best[height<=480]/best",
                "Prefer SD or lower",
            ),
            ("Audio only", "bestaudio/best", "Use the best audio stream"),
        ];

        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        content.append(&group);
        toolbar.set_content(Some(&content));

        let dialog = adw::Dialog::builder()
            .title(i18n("Online Video Quality"))
            .content_width(420)
            .content_height(360)
            .child(&toolbar)
            .build();
        for (title, format, subtitle) in choices {
            let row = BigNavigationRow::new(
                BigNavigationRowSpec::new(i18n(title))
                    .subtitle(i18n(subtitle))
                    .action_label(i18n("Use this quality")),
            )
            .into_root();
            row.connect_activated({
                let mpv = Arc::clone(&mpv);
                let dialog = dialog.downgrade();
                move |_| {
                    mpv.set_ytdl_format(format);
                    if let Some(dialog) = dialog.upgrade() {
                        dialog.close();
                    }
                }
            });
            group.add(&row);
        }
        dialog.present(Some(root));
    }

    /// Present the live media-information dialog for the loaded media. No-op
    /// before mpv is up.
    pub(super) fn show_media_info(&self, root: &adw::ApplicationWindow) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            show_video_media_info_dialog(root, mpv);
        }
    }

    /// Present the IPTV browser in a dialog over the main window. The browser
    /// widget is unparented on close so it can be re-presented later (a relm4
    /// `Controller`'s widget is a single long-lived instance).
    pub(super) fn show_iptv_browser(&self, root: &adw::ApplicationWindow) {
        present_controller_dialog(self.iptv.widget(), &i18n("IPTV"), 720, 720, root);
    }
    /// Present the open-file picker (video + playlist filters, multi-select).
    pub(super) fn show_open_file_dialog(root: &impl gtk::prelude::IsA<gtk::Window>) {
        let sender = shell_input_sender();
        FilePicker::new(i18n("Open Video"))
            .filter(&i18n("Video files"), VIDEO_EXTENSIONS)
            .filter(&i18n("Playlist files"), &["m3u", "m3u8", "pls"])
            .pattern_filter(&i18n("All files"), &["*"])
            .open_multiple(root, move |paths| {
                let strs: Vec<String> = paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                if !strs.is_empty() {
                    let _ = sender.send(VideoShellInput::OpenFiles(strs));
                }
            });
    }

    /// Present the open-URL entry dialog → `open_files([url])`. Validates the URL
    /// shape and rejects a leading `-` (option-injection guard for mpv).
    pub(super) fn show_open_url_dialog(root: &adw::ApplicationWindow) {
        let (dialog, entry) = big_app_kit::dialogs::entry_dialog(
            &i18n("Open URL"),
            &i18n("Enter a URL to play (YouTube, HTTP streams, etc.)"),
            &i18n("Cancel"),
            "play",
            &i18n("Play"),
            "https://...",
            &i18n("Media URL"),
        );
        entry.set_width_chars(48);
        let sender = shell_input_sender();
        dialog.connect_response(None, move |_dialog, response| {
            if response != "play" {
                return;
            }
            let url = entry.text().trim().to_string();
            if is_remote_uri(&url) {
                let _ = sender.send(VideoShellInput::OpenFiles(vec![url]));
            }
        });
        dialog.present(Some(root));
    }

    /// Present the recent-files chooser → `open_files([path])` on activation.
    pub(super) fn show_recent_dialog(root: &adw::ApplicationWindow) {
        let entries = crate::recent::load_recent_for("video");

        let dialog = adw::Dialog::builder()
            .title(i18n("Recent Files"))
            .content_width(520)
            .content_height(480)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());

        let scroll = gtk::ScrolledWindow::builder().vexpand(true).build();
        let clamp = adw::Clamp::builder()
            .maximum_size(500)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(12)
            .build();

        if entries.is_empty() {
            let status = adw::StatusPage::builder()
                .icon_name("document-open-recent-symbolic")
                .title(i18n("No Recent Files"))
                .build();
            clamp.set_child(Some(&status));
        } else {
            let group = adw::PreferencesGroup::new();
            let sender = shell_input_sender();
            for entry in &entries {
                let display = crate::recent::display_name(&entry.path);
                let row = BigNavigationRow::new(
                    BigNavigationRowSpec::new(&display)
                        .subtitle(&entry.path)
                        .action_label(i18n("Open recent file")),
                )
                .into_root();
                let path = entry.path.clone();
                let sender = sender.clone();
                let dlg = dialog.downgrade();
                row.connect_activated(move |_| {
                    let _ = sender.send(VideoShellInput::OpenFiles(vec![path.clone()]));
                    if let Some(d) = dlg.upgrade() {
                        d.close();
                    }
                });
                group.add(&row);
            }
            clamp.set_child(Some(&group));
        }
        scroll.set_child(Some(&clamp));
        toolbar.set_content(Some(&scroll));
        dialog.set_child(Some(&toolbar));
        dialog.present(Some(root));
    }

    /// Present the open-disc entry dialog (DVD / Blu-ray device or ISO/folder).
    /// Validates the device string (rejects empty and the `--` option-injection
    /// sequence) before forming the mpv `dvd://` / `bd://` URI. Mirrors the old
    /// `show_open_disc_dialog`.
    pub(super) fn show_open_disc_dialog(root: &adw::ApplicationWindow) {
        let (dialog, entry) = big_app_kit::dialogs::entry_choice_dialog(
            &i18n("Open Disc"),
            &i18n("Enter the disc device or path to ISO/folder:"),
            &i18n("Cancel"),
            "dvd",
            "/dev/sr0",
            &i18n("Disc device path"),
            &[
                ("dvd", "DVD", adw::ResponseAppearance::Default),
                ("bd", "Blu-ray", adw::ResponseAppearance::Default),
            ],
        );
        entry.set_text("/dev/sr0");
        entry.add_css_class("monospace");

        let sender = shell_input_sender();
        dialog.connect_response(None, move |_dlg, response| {
            if let Some(uri) = build_disc_uri(response, &entry.text()) {
                let _ = sender.send(VideoShellInput::OpenFiles(vec![uri]));
            }
        });
        dialog.present(Some(root));
    }

    /// Open the categorized preferences window bound to the live settings + mpv
    /// backend (rows persist + apply mpv properties live). Mirrors the old
    /// `show_preferences`.
    pub(super) fn show_preferences(&self, root: &adw::ApplicationWindow) {
        let mpv = self.mpv.borrow().as_ref().map(Arc::clone);
        crate::video::ui::preferences::show_preferences(root, &self.settings, mpv);
    }

    /// Open the appearance dialog (palette + OSD layout + button-layout editor).
    /// `on_changed` re-applies the OSD CSS live through the app-owned provider
    /// slot and reapplies the bottom OSD button zones exposed by the controls
    /// bar.
    pub(super) fn show_appearance(&self, root: &adw::ApplicationWindow) {
        let settings = self.settings.clone();
        let apply_settings = settings.clone();
        let button_layout = self.controls.model().button_layout();
        crate::video::ui::appearance::show_appearance_dialog(
            root,
            &settings,
            move || {
                reapply_theme(&apply_settings);
                button_layout.apply_settings(&apply_settings);
            },
            || {},
        );
    }

    /// Confirm + reset every setting to its default (restart-required toast in
    /// the body). Mirrors the old `show_restore_defaults_dialog`.
    pub(super) fn show_restore_defaults_dialog(root: &adw::ApplicationWindow) {
        let alert = big_app_kit::dialogs::confirm_dialog(
            &i18n("Restore Defaults"),
            &i18n("Reset all settings to their default values? The application will need to be restarted for all changes to take effect."),
            &i18n("Cancel"),
            "restore",
            &i18n("Restore"),
            true,
        );
        let sender = shell_input_sender();
        alert.connect_response(None, move |_dialog, response| {
            if response == "restore" {
                let _ = sender.send(VideoShellInput::RestoreDefaultsConfirmed);
            }
        });
        alert.present(Some(root));
    }

    /// Apply a confirmed restore-to-defaults: reset the settings store. A
    /// restart picks up the new values (mpv is configured at startup).
    pub(super) fn restore_all_defaults(&self) {
        self.settings.reset_to_defaults();
        log::info!("video: all settings restored to defaults");
    }
}

thread_local! {
    /// Display-wide OSD-theme CSS provider slot. Single slot replaced in place
    /// on boot and on every Appearance-dialog change (the provider is attached
    /// to the default `gdk::Display`, which is process-global, so a thread-local
    /// is the right ownership home — no per-shell duplication, no stacking).
    static LIVE_THEME_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// (Re)apply the OSD palette + layout CSS into the shared provider slot. Called
/// at boot and from the Appearance dialog's `on_changed`.
pub(super) fn reapply_theme(settings: &Settings) {
    LIVE_THEME_PROVIDER.with(|cell| {
        let mut slot = cell.borrow_mut();
        crate::video::ui::appearance::apply_appearance(settings, &mut slot);
    });
}

pub(super) fn build_disc_uri(response: &str, device: &str) -> Option<String> {
    if device.is_empty() || device.contains("--") {
        return None;
    }
    match response {
        "dvd" => Some(format!("dvd://--dvd-device={device}")),
        "bd" => Some(format!("bd://--bluray-device={device}")),
        _ => None,
    }
}

/// Mount a controller's long-lived widget in a dialog and present it, detaching
/// the widget on close so it can be re-mounted next open (mirrors the IPTV
/// browser pattern).
pub(super) fn present_controller_dialog(
    widget: &impl IsA<gtk::Widget>,
    title: &str,
    content_width: i32,
    content_height: i32,
    root: &adw::ApplicationWindow,
) {
    let widget = widget.as_ref();
    if widget.parent().is_some() {
        return;
    }
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(widget));
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(content_width)
        .content_height(content_height)
        .child(&toolbar)
        .build();
    let toolbar_weak = toolbar.downgrade();
    let widget = widget.clone();
    dialog.connect_closed(move |_| {
        if let Some(toolbar) = toolbar_weak.upgrade() {
            toolbar.set_content(None::<&gtk::Widget>);
        }
        widget.unparent();
    });
    dialog.present(Some(root));
}

#[cfg(test)]
mod tests {
    use super::build_disc_uri;

    #[test]
    fn disc_uri_builds_dvd_and_bluray() {
        assert_eq!(
            build_disc_uri("dvd", "/dev/sr0").as_deref(),
            Some("dvd://--dvd-device=/dev/sr0")
        );
        assert_eq!(
            build_disc_uri("bd", "/dev/sr1").as_deref(),
            Some("bd://--bluray-device=/dev/sr1")
        );
    }

    #[test]
    fn disc_uri_rejects_empty_device() {
        assert_eq!(build_disc_uri("dvd", ""), None);
        assert_eq!(build_disc_uri("bd", ""), None);
    }

    #[test]
    fn disc_uri_rejects_option_injection() {
        // A `--` in the device must never reach mpv as a smuggled option.
        assert_eq!(build_disc_uri("dvd", "/dev/sr0 --no-config"), None);
        assert_eq!(build_disc_uri("bd", "--foo"), None);
    }

    #[test]
    fn disc_uri_rejects_unknown_response() {
        assert_eq!(build_disc_uri("cancel", "/dev/sr0"), None);
        assert_eq!(build_disc_uri("", "/dev/sr0"), None);
    }
}
