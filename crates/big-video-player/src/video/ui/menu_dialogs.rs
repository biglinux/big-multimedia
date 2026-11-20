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
use big_media_components::time::playback_time;
use big_relm4_components::layout::name_prompt::{BigNamePromptSpec, prompt_name};

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
    /// Present the go-to-time entry dialog, filled with the current position.
    /// Accepts `H:MM:SS`, `M:SS`, or plain seconds; on confirm the shell seeks
    /// there, on the TV when one plays.
    pub(super) fn show_goto_time_dialog(&self, root: &adw::ApplicationWindow) {
        let spec = BigNamePromptSpec {
            title: i18n("Go to Time"),
            description: Some(i18n("Use H:MM:SS, M:SS, or seconds.")),
            entry_title: i18n("Playback time"),
            prefill: playback_time(self.playback_position()),
            accept_label: i18n("Go"),
            content_width: 420,
            content_height: -1,
        };
        let validate = |text: &str| accepted_goto_seconds(text).map(|_| text.to_owned());
        prompt_name(Some(root.upcast_ref()), &spec, validate, |text| {
            let Some(secs) = accepted_goto_seconds(&text) else {
                return false;
            };
            // Through the shell, which sends it to the TV when one plays.
            crate::video::ui::video_shell::send_to_shell(
                crate::video::ui::video_shell::VideoShellInput::SeekTo(secs),
            );
            true
        });
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
        let chosen = std::path::PathBuf::from(
            settings.get_string(crate::settings::keys::SCREENSHOT_DIRECTORY),
        );
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
