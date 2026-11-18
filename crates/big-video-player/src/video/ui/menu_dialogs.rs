//! Small, self-contained hamburger-menu dialogs/actions: go-to-time,
//! screenshot, chapter list + navigation. Each
//! `impl VideoShell` method drives the live mpv backend; the push-model wiring (a `VideoShellInput::Show*` variant →
//! handler) matches the existing media-info / filters dialogs.

use std::path::PathBuf;

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use super::video_shell::{VideoShell, VideoShellInput};
use crate::i18n::i18n;
use crate::player::mpv_backend::ScreenshotRequestError;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::{BigDialogPage, focus_when_shown},
    preferences::BigPreferencesGroupSpec,
};

/// mpv took the request but did not save the picture; `error` says why.
pub(crate) fn screenshot_not_saved_message(error: &str) -> String {
    i18n("The screenshot was not saved: {error}").replace("{error}", error)
}

// Bound setup tasks as well as accepted native requests. The global admission
// counter covers concurrent windows in the single-process host.
static SCREENSHOT_PREPARATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
struct ScreenshotPreparation;
impl ScreenshotPreparation {
    fn acquire() -> Option<Self> {
        SCREENSHOT_PREPARATIONS
            .fetch_update(
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
                |count| (count < 8).then_some(count + 1),
            )
            .ok()
            .map(|_| Self)
    }
}
impl Drop for ScreenshotPreparation {
    fn drop(&mut self) {
        SCREENSHOT_PREPARATIONS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    }
}

impl VideoShell {
    /// Present the go-to-time entry dialog. Accepts `H:MM:SS`, `M:SS`, or plain
    /// seconds; on confirm the shell seeks there, on the TV when one plays.
    pub(super) fn show_goto_time_dialog(root: &adw::ApplicationWindow) {
        let dialog = big_gtk_kit::dialog::Dialog::builder()
            .title(i18n("Go to Time"))
            .content_width(420)
            .content_height(260)
            .build();
        let page = BigDialogPage::plain();
        page.content().set_spacing(12);

        let group = BigPreferencesGroupSpec::untitled()
            .description(i18n("Use H:MM:SS, M:SS, or seconds."))
            .build();
        let entry = adw::EntryRow::builder()
            .title(i18n("Playback time"))
            .text("0:00:00")
            .show_apply_button(true)
            .build();
        group.add(&entry);
        page.content().append(&group);

        let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Go")));
        footer
            .accept_button()
            .set_sensitive(accepted_goto_seconds(&entry.text()).is_some());
        page.root().add_bottom_bar(footer.root());
        dialog.set_child(Some(page.root()));

        let accept_button = footer.accept_button().downgrade();
        entry.connect_changed(move |row| {
            if let Some(accept_button) = accept_button.upgrade() {
                accept_button.set_sensitive(accepted_goto_seconds(&row.text()).is_some());
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
            if let Some(secs) = accepted_goto_seconds(&entry_for_accept.text()) {
                // Through the shell, which sends it to the TV when one plays.
                crate::video::ui::video_shell::send_to_shell(
                    crate::video::ui::video_shell::VideoShellInput::SeekTo(secs),
                );
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        });

        focus_when_shown(&entry, 0, -1);
        dialog.present(Some(root));
    }

    /// Request off-thread staging; success is delivered by a terminal mpv reply.
    pub(super) fn take_screenshot(&self, sender: &ComponentSender<Self>) {
        let Some(permit) = ScreenshotPreparation::acquire() else {
            self.notify(&i18n(
                "Still saving the previous screenshots. Try again in a moment.",
            ));
            return;
        };
        let Some(mpv) = self.mpv_handle().borrow().as_ref().cloned() else {
            if !self.report_missing_engine() {
                self.notify(&i18n("Open a video to take a screenshot"));
            }
            return;
        };
        if mpv.get_current_file().is_none() {
            self.notify(&i18n("Open a video to take a screenshot"));
            return;
        }
        let settings = self.settings_handle();
        let chosen = std::path::PathBuf::from(settings.string("screenshot-directory"));
        let Some(directory) = chosen
            .is_absolute()
            .then_some(chosen)
            .or_else(|| big_os_kit::xdg_dirs::user_dir(big_os_kit::xdg_dirs::UserDir::Pictures))
            .or_else(big_os_kit::xdg_dirs::home_dir)
        else {
            self.notify(&i18n(
                "There is no Pictures or home folder to save screenshots in. Choose a folder in Preferences.",
            ));
            return;
        };
        // Metadata titles can contain secrets or exceed NAME_MAX. The public
        // filename is independent of the media title/URL.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let target = directory.join(format!(
            "screenshot-{}-{}.png",
            glib::real_time(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let alive = self.screenshot_owner().clone();
        let input = sender.input_sender().clone();
        glib::spawn_future_local(async move {
            let _permit = permit;
            let folder = directory.clone();
            let result = gio::spawn_blocking(move || {
                std::fs::create_dir_all(&directory).map_err(ScreenshotRequestError::Folder)?;
                mpv.screenshot_to_file_async(&target, alive)
            })
            .await;
            let message = match result {
                Ok(Ok(_) | Err(ScreenshotRequestError::Closed)) => return,
                Ok(Err(ScreenshotRequestError::Folder(error))) => {
                    i18n("Cannot save to {folder}: {error}. Choose another folder in Preferences.")
                        .replace("{folder}", &folder.display().to_string())
                        .replace("{error}", &error.to_string())
                }
                Ok(Err(ScreenshotRequestError::Busy)) => {
                    i18n("Still saving the previous screenshots. Try again in a moment.")
                }
                Ok(Err(ScreenshotRequestError::Rejected(error))) => {
                    screenshot_not_saved_message(&error)
                }
                Err(_) => i18n("The screenshot was not saved because of an unexpected error"),
            };
            let _ = input.send(VideoShellInput::ScreenshotFailed(message));
        });
    }

    /// Jump to chapter `idx` (from the chapter list dialog). Drives
    /// `mpv.set_chapter`.
    pub(super) fn set_chapter(&self, idx: i64) {
        if let Some(mpv) = self.mpv_handle().borrow().as_ref() {
            mpv.set_chapter(idx);
        }
    }
}

/// Parse a go-to-time string (`H:MM:SS`, `M:SS`, or seconds) into seconds.
/// Delegates to the shared media-time parser. Returns `None` on invalid input.
fn parse_goto_seconds(input: &str) -> Option<f64> {
    big_media_components::time::parse_playback_time(input)
}

fn accepted_goto_seconds(input: &str) -> Option<f64> {
    let text = input.trim();
    if text.is_empty() {
        None
    } else {
        parse_goto_seconds(text)
    }
}

/// Resolve a non-empty chapter title, falling back to `Chapter N` (1-based) when
/// mpv reports an empty or placeholder title.
pub(super) fn chapter_title(raw: String, index: i64) -> String {
    chapter_title_with_prefix(raw, index, &i18n("Chapter"))
}

fn chapter_title_with_prefix(raw: String, index: i64, chapter_prefix: &str) -> String {
    if raw.is_empty()
        || raw.eq_ignore_ascii_case("(unnamed)")
        || raw.eq_ignore_ascii_case("unnamed")
    {
        format!("{chapter_prefix} {}", index + 1)
    } else {
        raw
    }
}

/// Called only after terminal completion and no-replace publication.
pub(super) fn show_screenshot_result(
    notice: &big_media_components::surface::MediaNotice,
    path: PathBuf,
    durable: bool,
) {
    let message = if durable {
        i18n("Screenshot saved")
    } else {
        i18n("Screenshot saved; disk synchronization could not be confirmed")
    };
    notice.show_action(&message, &i18n("Open"), move || {
        big_app_kit::desktop::open_file(&path);
    });
}

#[cfg(test)]
mod tests {
    use super::chapter_title_with_prefix;

    #[test]
    fn blank_chapter_title_falls_back() {
        assert_eq!(
            chapter_title_with_prefix(String::new(), 0, "Chapter"),
            "Chapter 1"
        );
        assert_eq!(
            chapter_title_with_prefix("(unnamed)".to_string(), 2, "Chapter"),
            "Chapter 3"
        );
        assert_eq!(
            chapter_title_with_prefix("Intro".to_string(), 0, "Chapter"),
            "Intro"
        );
    }
}
