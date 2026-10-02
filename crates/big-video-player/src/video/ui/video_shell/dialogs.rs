use super::input::shell_input_sender;
use super::playback::is_remote_uri;
use super::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

impl VideoShell {
    /// "Play on a TV", the settings panel's page: hand the loaded video,
    /// where it is now, to a Chromecast or DLNA TV, pausing it here once the
    /// TV plays.
    pub(super) fn tv_page(&self) -> gtk::Widget {
        use crate::cast::daemon::PlaybackStart;
        use crate::shared::tv_cast::{TvMedia, tv_page};
        let mpv = self.mpv.borrow().clone();
        let current = mpv.clone();
        let media = move || {
            let mpv = current.as_ref()?;
            Some(TvMedia {
                source: mpv.get_current_file()?,
                title: mpv.get_media_title().unwrap_or_default(),
                audio: false,
                start: PlaybackStart {
                    seconds: mpv.get_position(),
                    paused: false,
                    volume: (mpv.get_volume() / 100.0).clamp(0.0, 1.0),
                    muted: mpv.get_mute(),
                    height: 0,
                },
            })
        };
        tv_page(&self.tv, &self.settings, true, media, move || {
            if let Some(mpv) = &mpv {
                mpv.pause();
            }
            // The card over the video says it from here; the panel repeating
            // it beside the card only doubled it.
            super::send_to_shell(VideoShellInput::ClosePanel);
        })
    }

    pub(super) fn show_ytdl_quality_dialog(&self, root: &adw::ApplicationWindow) {
        let Some(mpv) = self.mpv.borrow().as_ref().map(Arc::clone) else {
            return;
        };
        let group = BigPreferencesGroupSpec::untitled().build();
        // Titles/subtitles are wrapped in `i18n()` on string literals here (not
        // `i18n(title)` in the loop) so `xgettext` can extract them into the POT.
        let choices = [
            (
                i18n("Best available"),
                "bestvideo*+bestaudio/best",
                i18n("Highest video and audio quality"),
            ),
            (
                i18n("Up to 1080p"),
                "bestvideo*[height<=1080]+bestaudio/best[height<=1080]/best",
                i18n("Prefer Full HD or lower"),
            ),
            (
                i18n("Up to 720p"),
                "bestvideo*[height<=720]+bestaudio/best[height<=720]/best",
                i18n("Prefer HD or lower"),
            ),
            (
                i18n("Up to 480p"),
                "bestvideo*[height<=480]+bestaudio/best[height<=480]/best",
                i18n("Prefer SD or lower"),
            ),
            (
                i18n("Audio only"),
                "bestaudio/best",
                i18n("Use the best audio stream"),
            ),
        ];

        let page = BigDialogPage::scrolled_clamped(420, 360);
        page.content().append(&group);

        let dialog = adw::Dialog::builder()
            .title(i18n("Online Video Quality"))
            .content_width(420)
            .content_height(360)
            .child(&page.into_root())
            .build();
        for (title, format, subtitle) in choices {
            let row = BigNavigationRow::new(
                BigNavigationRowSpec::new(title)
                    .subtitle(subtitle)
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

    /// Present the live media-details dialog for the loaded media. No-op
    /// before mpv is up.
    pub(super) fn show_media_details(&self, root: &adw::ApplicationWindow) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            show_video_media_details_dialog(root, mpv);
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
        let Some(sender) = shell_input_sender() else {
            log::warn!("video open-file dialog ignored before shell input registration");
            return;
        };
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
        let Some(sender) = shell_input_sender() else {
            log::warn!("video open-url dialog ignored before shell input registration");
            return;
        };
        let dialog = adw::Dialog::builder()
            .title(i18n("Open URL"))
            .content_width(460)
            .content_height(260)
            .build();
        let page = BigDialogPage::plain();
        page.content().set_spacing(12);

        let group = BigPreferencesGroupSpec::untitled()
            .description(i18n(
                "Supports YouTube, HTTP streams, RTSP, RTMP, DVD, and Blu-ray URLs.",
            ))
            .build();
        let entry = adw::EntryRow::builder()
            .title(i18n("Media URL"))
            .show_apply_button(true)
            .build();
        group.add(&entry);
        page.content().append(&group);

        let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Play")));
        footer.accept_button().set_sensitive(false);
        page.root().add_bottom_bar(footer.root());
        dialog.set_child(Some(page.root()));

        let accept_button = footer.accept_button().downgrade();
        entry.connect_changed(move |row| {
            if let Some(accept_button) = accept_button.upgrade() {
                accept_button.set_sensitive(accepted_open_url(&row.text()).is_some());
            }
        });

        let accept_button = footer.accept_button().downgrade();
        entry.connect_apply(move |_| {
            if let Some(accept_button) = accept_button.upgrade()
                && accept_button.is_sensitive()
            {
                accept_button.emit_clicked();
            }
        });

        let dialog_ref = dialog.downgrade();
        footer.cancel_button().connect_clicked(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        });

        let dialog_ref = dialog.downgrade();
        let entry_for_accept = entry.clone();
        footer.accept_button().connect_clicked(move |_| {
            if let Some(url) = accepted_open_url(&entry_for_accept.text()) {
                let _ = sender.send(VideoShellInput::OpenFiles(vec![url]));
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        });

        let entry_for_idle = entry.clone();
        dialog.connect_map(move |_| {
            let entry = entry_for_idle.clone();
            gtk::glib::idle_add_local_once(move || {
                entry.grab_focus();
            });
        });
        dialog.present(Some(root));
    }

    /// Present the open-disc entry dialog (DVD / Blu-ray device or ISO/folder).
    /// Validates the device string (rejects empty and the `--` option-injection
    /// sequence) before forming the mpv `dvd://` / `bd://` URI.
    pub(super) fn show_open_disc_dialog(root: &adw::ApplicationWindow) {
        let Some(sender) = shell_input_sender() else {
            log::warn!("video open-disc dialog ignored before shell input registration");
            return;
        };
        let dialog = adw::Dialog::builder()
            .title(i18n("Open Disc"))
            .content_width(460)
            .content_height(320)
            .build();
        let page = BigDialogPage::plain();
        page.content().set_spacing(12);

        let group = BigPreferencesGroupSpec::untitled()
            .description(i18n(
                "Choose a disc type and enter the device, ISO, or folder path.",
            ))
            .build();
        let disc_type_row = BigDropdownRow::new(BigDropdownRowSpec::new(
            i18n("Disc type"),
            [i18n("DVD"), i18n("Blu-ray")],
        ));
        let entry = adw::EntryRow::builder()
            .title(i18n("Disc device path"))
            .text("/dev/sr0")
            .show_apply_button(true)
            .build();
        entry.add_css_class("monospace");
        group.add(disc_type_row.root());
        group.add(&entry);
        page.content().append(&group);

        let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Open")));
        page.root().add_bottom_bar(footer.root());
        dialog.set_child(Some(page.root()));

        let refresh_accept: Rc<dyn Fn()> = Rc::new({
            let accept_button = footer.accept_button().downgrade();
            let dropdown = disc_type_row.dropdown().downgrade();
            let entry = entry.downgrade();
            move || {
                let (Some(accept_button), Some(dropdown), Some(entry)) =
                    (accept_button.upgrade(), dropdown.upgrade(), entry.upgrade())
                else {
                    return;
                };
                accept_button
                    .set_sensitive(build_disc_uri(dropdown.selected(), &entry.text()).is_some());
            }
        });
        refresh_accept();

        entry.connect_changed({
            let refresh_accept = Rc::clone(&refresh_accept);
            move |_| refresh_accept()
        });
        disc_type_row.dropdown().connect_selected_notify({
            let refresh_accept = Rc::clone(&refresh_accept);
            move |_| refresh_accept()
        });

        let accept_button = footer.accept_button().downgrade();
        entry.connect_apply(move |_| {
            if let Some(accept_button) = accept_button.upgrade()
                && accept_button.is_sensitive()
            {
                accept_button.emit_clicked();
            }
        });

        let dialog_ref = dialog.downgrade();
        footer.cancel_button().connect_clicked(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        });

        let dialog_ref = dialog.downgrade();
        let dropdown = disc_type_row.dropdown().clone();
        let entry_for_accept = entry.clone();
        footer.accept_button().connect_clicked(move |_| {
            if let Some(uri) = build_disc_uri(dropdown.selected(), &entry_for_accept.text()) {
                let _ = sender.send(VideoShellInput::OpenFiles(vec![uri]));
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        });

        let entry_for_idle = entry.clone();
        dialog.connect_map(move |_| {
            let entry = entry_for_idle.clone();
            gtk::glib::idle_add_local_once(move || {
                entry.grab_focus();
                entry.select_region(0, -1);
            });
        });
        dialog.present(Some(root));
    }

    /// Open the categorized preferences window bound to the live settings + mpv
    /// backend (rows persist + apply mpv properties live).
    pub(super) fn show_preferences(&self, root: &adw::ApplicationWindow) {
        let mpv = self.mpv.borrow().as_ref().map(Arc::clone);
        let dialog = crate::video::ui::preferences::show_preferences(
            root,
            &self.settings,
            mpv,
            |preferences| {
                // The preferences show the old values after a reset, so close them.
                let preferences_ref = preferences.downgrade();
                Self::show_restore_defaults_dialog(preferences, move || {
                    if let Some(preferences) = preferences_ref.upgrade() {
                        preferences.close();
                    }
                });
            },
        );
        if let Some(sender) = shell_input_sender() {
            dialog.connect_closed(move |_| {
                let _ = sender.send(VideoShellInput::PreferencesClosed);
            });
        }
    }

    /// Open the appearance dialog (palette + OSD layout + button-layout editor).
    /// `on_changed` re-applies the OSD CSS live through the app-owned provider
    /// slot and reapplies the bottom OSD button zones exposed by the controls
    /// bar.
    pub(super) fn show_appearance(&self, root: &adw::ApplicationWindow) {
        let settings = self.settings.clone();
        let button_layout = self.controls.model().button_layout();
        crate::video::ui::appearance::show_appearance_dialog(root, &self.settings, move || {
            reapply_theme(&settings);
            button_layout.apply_settings(&settings);
        });
    }

    /// Confirm, then reset every setting to its default; `on_confirmed` runs
    /// after the reset request is sent.
    pub(super) fn show_restore_defaults_dialog(
        parent: &impl IsA<gtk::Widget>,
        on_confirmed: impl Fn() + 'static,
    ) {
        let Some(sender) = shell_input_sender() else {
            log::warn!("video restore-defaults dialog ignored before shell input registration");
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading(i18n("Restore Default Settings?"))
            .body(i18n(
                "Every setting, including appearance, goes back to its default value. Some changes take effect after reopening the player.",
            ))
            .default_response("cancel")
            .close_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", &i18n("Cancel")), ("restore", &i18n("Restore"))]);
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        dialog.connect_response(Some("restore"), move |_, _| {
            let _ = sender.send(VideoShellInput::RestoreDefaultsConfirmed);
            on_confirmed();
        });
        dialog.present(Some(parent));
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

/// The mpv URI for the disc in `device`: Blu-ray for the dropdown's second
/// entry, else DVD.
fn build_disc_uri(selected: u32, device: &str) -> Option<String> {
    if device.is_empty() || device.contains("--") {
        return None;
    }
    Some(if selected == 1 {
        format!("bd://--bluray-device={device}")
    } else {
        format!("dvd://--dvd-device={device}")
    })
}

fn accepted_open_url(input: &str) -> Option<String> {
    let url = input.trim();
    if url.starts_with('-') || !is_remote_uri(url) {
        return None;
    }
    let (_, target) = url.split_once("://")?;
    if target.trim().is_empty() {
        return None;
    }
    Some(url.to_string())
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
    let page = BigDialogPage::plain();
    page.root().set_content(Some(widget));
    let dialog_root = page.into_root();
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(content_width)
        .content_height(content_height)
        .child(&dialog_root)
        .build();
    let dialog_root_weak = dialog_root.downgrade();
    let widget = widget.clone();
    // leak-guard: allow(closed-unparent) This is AdwDialog, not GtkPopover.
    dialog.connect_closed(move |_| {
        if let Some(dialog_root) = dialog_root_weak.upgrade() {
            dialog_root.set_content(None::<&gtk::Widget>);
        }
        widget.unparent();
    });
    dialog.present(Some(root));
}

#[cfg(test)]
mod tests {
    use super::{accepted_open_url, build_disc_uri};

    #[test]
    fn open_url_accepts_trimmed_remote_media_urls() {
        assert_eq!(
            accepted_open_url("  https://example.test/video.mp4  ").as_deref(),
            Some("https://example.test/video.mp4")
        );
        assert_eq!(
            accepted_open_url("rtsp://camera.local/stream").as_deref(),
            Some("rtsp://camera.local/stream")
        );
    }

    #[test]
    fn open_url_rejects_local_option_and_empty_targets() {
        for input in [
            "",
            "/home/user/video.mp4",
            "file:///home/user/video.mp4",
            "--profile=evil",
            "-https://example.test/video.mp4",
            "https://",
            "rtmp://   ",
        ] {
            assert_eq!(accepted_open_url(input), None, "{input} should be rejected");
        }
    }

    #[test]
    fn disc_uri_builds_dvd_and_bluray() {
        assert_eq!(
            build_disc_uri(0, "/dev/sr0").as_deref(),
            Some("dvd://--dvd-device=/dev/sr0")
        );
        assert_eq!(
            build_disc_uri(1, "/dev/sr1").as_deref(),
            Some("bd://--bluray-device=/dev/sr1")
        );
    }

    #[test]
    fn disc_uri_rejects_empty_device() {
        assert_eq!(build_disc_uri(0, ""), None);
        assert_eq!(build_disc_uri(1, ""), None);
    }

    #[test]
    fn disc_uri_rejects_option_injection() {
        // A `--` in the device must never reach mpv as a smuggled option.
        assert_eq!(build_disc_uri(0, "/dev/sr0 --no-config"), None);
        assert_eq!(build_disc_uri(1, "--foo"), None);
    }
}
