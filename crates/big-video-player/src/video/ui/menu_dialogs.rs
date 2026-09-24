//! Small, self-contained hamburger-menu dialogs/actions ported from the old
//! god-object: go-to-time, screenshot, chapter list + navigation, and the sleep
//! timer. Each `impl VideoShell` method mirrors the old handler and drives the
//! live mpv backend; the push-model wiring (a `VideoShellInput::Show*` variant →
//! handler) matches the existing media-info / filters dialogs.
//!
//! The pure helpers (`parse_goto_seconds`, `sleep_minutes_to_secs`,
//! `screenshot_filename`) carry the testable logic.

use std::path::PathBuf;

use adw::prelude::*;
use gtk::glib;
use relm4::ComponentSender;

use super::video_shell::{VideoShell, VideoShellInput};
use crate::i18n::i18n;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};

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
    /// Show a transient toast in the window's toast overlay. Mirrors the old
    /// `toast_overlay.add_toast` feedback used by every ported handler.
    pub(super) fn notify(&self, message: &str) {
        self.toast_overlay().add_toast(adw::Toast::new(message));
    }

    /// Present the go-to-time entry dialog. Accepts `H:MM:SS`, `M:SS`, or plain
    /// seconds; on confirm it seeks mpv absolutely. Mirrors the old
    /// `show_goto_dialog`; drives `mpv.seek_absolute`.
    pub(super) fn show_goto_time_dialog(&self, root: &adw::ApplicationWindow) {
        let dialog = adw::Dialog::builder()
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

        let accept_button = footer.accept_button().clone();
        entry.connect_changed(move |row| {
            accept_button.set_sensitive(accepted_goto_seconds(&row.text()).is_some());
        });

        let accept_button = footer.accept_button().clone();
        entry.connect_apply(move |_| {
            if accept_button.is_sensitive() {
                accept_button.emit_clicked();
            }
        });

        let dialog_ref = dialog.downgrade();
        footer.cancel_button().connect_clicked(move |_| {
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        });

        let mpv = self.mpv_handle().clone();
        let dialog_ref = dialog.downgrade();
        let entry_for_accept = entry.clone();
        footer.accept_button().connect_clicked(move |_| {
            if let Some(secs) = accepted_goto_seconds(&entry_for_accept.text()) {
                if let Some(mpv) = mpv.borrow().as_ref() {
                    mpv.seek_absolute(secs);
                }
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
        });

        let entry_for_idle = entry.clone();
        dialog.connect_map(move |_| {
            let entry = entry_for_idle.clone();
            glib::idle_add_local_once(move || {
                entry.grab_focus();
                entry.select_region(0, -1);
            });
        });
        dialog.present(Some(root));
    }

    /// Request off-thread staging; success is delivered by a terminal mpv reply.
    pub(super) fn take_screenshot(&self, sender: &ComponentSender<Self>) {
        let Some(permit) = ScreenshotPreparation::acquire() else {
            self.notify(&i18n("Screenshot queue is full; wait for pending captures"));
            return;
        };
        let Some(mpv) = self.mpv_handle().borrow().as_ref().cloned() else {
            self.notify(&i18n("Screenshot failed"));
            return;
        };
        if mpv.get_current_file().is_none() {
            self.notify(&i18n("Screenshot failed"));
            return;
        }
        let Some(directory) = dirs::picture_dir().or_else(dirs::home_dir) else {
            self.notify(&i18n("Screenshot failed"));
            return;
        };
        // Metadata titles can contain secrets or exceed NAME_MAX. The public
        // filename is deliberately independent of the media title/URL.
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
            let result = gio::spawn_blocking(move || {
                std::fs::create_dir_all(&directory)?;
                mpv.screenshot_to_file_async(&target, alive)
            })
            .await;
            if !matches!(result, Ok(Ok(_))) {
                let _ = input.send(VideoShellInput::ScreenshotQueueFailed);
            }
        });
    }

    /// Jump to chapter `idx` (from the chapter list dialog). Drives
    /// `mpv.set_chapter`.
    pub(super) fn set_chapter(&self, idx: i64) {
        if let Some(mpv) = self.mpv_handle().borrow().as_ref() {
            mpv.set_chapter(idx);
        }
    }

    /// Present the chapter list: one activatable row per chapter (time + title)
    /// that jumps to it via `SetChapter`. Mirrors the old chapter navigation
    /// (`chapter_next`/`chapter_prev` + `set_chapter`). Toasts "No chapters"
    /// when the media has none. Reads titles/times from the public mpv property
    /// accessors so it stays VideoShell-local.
    pub(super) fn show_chapters_dialog(
        &self,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        let mpv_ref = self.mpv_handle();
        let mpv_borrow = mpv_ref.borrow();
        let Some(mpv) = mpv_borrow.as_ref() else {
            self.notify(&i18n("No chapters"));
            return;
        };
        let count = mpv.get_chapter_count();
        if count <= 0 {
            self.notify(&i18n("No chapters"));
            return;
        }
        let current = mpv.get_current_chapter();

        let group = BigPreferencesGroupSpec::untitled().build();
        for i in 0..count {
            let time = mpv.get_property_f64(&format!("chapter-list/{i}/time"));
            let title = chapter_title(
                mpv.get_property_string(&format!("chapter-list/{i}/title")),
                i,
            );
            let mut spec = BigNavigationRowSpec::new(&title)
                .subtitle(big_media_components::time::playback_time(time))
                .action_label(i18n("Jump to chapter"));
            if i == current {
                spec = spec.prefix_icon_name("media-playback-start-symbolic");
            }
            let row = BigNavigationRow::new(spec).into_root();
            let sender = sender.clone();
            row.connect_activated(move |_| {
                sender.input(VideoShellInput::SetChapter(i));
            });
            group.add(&row);
        }
        drop(mpv_borrow);

        let page = BigDialogPage::scrolled_clamped(420, 360);
        page.content().append(&group);
        let dialog = adw::Dialog::builder()
            .title(i18n("Chapters"))
            .content_width(420)
            .content_height(520)
            .child(&page.into_root())
            .build();
        dialog.present(Some(root));
    }

    /// Present the sleep-timer dialog: the active countdown (with cancel) when a
    /// timer is armed, otherwise the setup page (duration + on-expiry action).
    /// Mirrors the old `show_sleep_timer_dialog` and the audio player's port,
    /// reusing the shared `BigSleepTimer*` components + `shared::sleep_timer`
    /// label/persistence helpers.
    pub(super) fn show_sleep_timer_dialog(
        &self,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        use big_relm4_components::display::sleep_timer::{
            BigSleepTimerActiveLabels, BigSleepTimerActivePage, BigSleepTimerActiveSpec,
            BigSleepTimerSetupPage, BigSleepTimerSetupSpec, connect_sleep_timer_start_with_close,
        };

        let active = self.sleep_timer_is_armed();

        let dialog = adw::Dialog::builder()
            .title(i18n("Sleep Timer"))
            .content_width(600)
            .content_height(if active { 420 } else { 560 })
            .build();

        let icon = || gtk::Image::from_icon_name("preferences-system-time-symbolic");

        let page = if active {
            let remaining = self.sleep_timer_remaining_secs();
            let active_page = BigSleepTimerActivePage::new(
                BigSleepTimerActiveSpec {
                    labels: BigSleepTimerActiveLabels {
                        title: i18n("Timer Active"),
                        cancel_button: i18n("Cancel Timer"),
                    },
                    remaining_seconds: remaining,
                    icon_size: 80,
                },
                Some(icon().upcast()),
            );
            active_page.start_preview_countdown(remaining);

            let input = sender.input_sender().clone();
            let dialog_ref = dialog.downgrade();
            active_page.cancel_button().connect_clicked(move |_| {
                let _ = input.send(VideoShellInput::SleepTimerCancel);
                if let Some(d) = dialog_ref.upgrade() {
                    d.close();
                }
            });
            active_page.into_page()
        } else {
            let settings = self.settings_handle();
            let (saved_mins, saved_action) =
                crate::shared::sleep_timer::load_sleep_timer_preferences(Some(settings));
            let setup_page = BigSleepTimerSetupPage::new(
                BigSleepTimerSetupSpec {
                    labels: crate::shared::sleep_timer::sleep_timer_setup_labels(),
                    actions: crate::shared::sleep_timer::sleep_timer_actions(),
                    saved_minutes: saved_mins,
                    saved_action_id: saved_action,
                    icon_size: 72,
                },
                Some(icon().upcast()),
                None,
                None,
            );

            let input = sender.input_sender().clone();
            let settings = settings.clone();
            let dialog_ref = dialog.downgrade();
            connect_sleep_timer_start_with_close(
                &setup_page,
                root,
                move |_, minutes, action| {
                    crate::shared::sleep_timer::save_sleep_timer_preferences(
                        Some(&settings),
                        minutes,
                        &action,
                    );
                    let _ = input.send(VideoShellInput::SleepTimerStart { minutes, action });
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
        dialog.present(Some(root));
    }

    /// Arm the sleep timer: a 1 s `SleepTick` source drives the countdown. Stores
    /// the `SourceId` so it is removed on disarm / re-arm / drop (no leak).
    pub(super) fn start_sleep_timer(
        &mut self,
        total_secs: u32,
        action: String,
        sender: &ComponentSender<Self>,
    ) {
        self.cancel_sleep_timer();
        self.set_sleep_timer_remaining(total_secs);
        self.set_sleep_timer_action(action);
        let pre_fade = self
            .mpv_handle()
            .borrow()
            .as_ref()
            .map_or(100.0, |m| m.get_volume());
        self.set_sleep_timer_volume(pre_fade);

        let input = sender.input_sender().clone();
        let source = glib::timeout_add_seconds_local(1, move || {
            let _ = input.send(VideoShellInput::SleepTick);
            glib::ControlFlow::Continue
        });
        self.set_sleep_timer_source(Some(source));
        self.notify(&i18n("Sleep timer started"));
    }

    /// Disarm the sleep timer (no-op if none armed). Removes the stored source.
    pub(super) fn cancel_sleep_timer(&mut self) {
        if let Some(source) = self.take_sleep_timer_source() {
            source.remove();
        }
        self.set_sleep_timer_remaining(0);
    }

    /// One countdown tick: fade the last 10 s, then on expiry pause + run the
    /// chosen action (close / suspend / power off), restoring the volume.
    /// Mirrors the audio player's `sleep_tick`.
    pub(super) fn sleep_tick(&mut self) {
        if !self.sleep_timer_is_armed() {
            return;
        }
        let remaining = self.sleep_timer_remaining_secs();
        let pre_fade = self.sleep_timer_volume_value();
        if remaining <= 1 {
            if let Some(mpv) = self.mpv_handle().borrow().as_ref() {
                mpv.pause();
                mpv.set_volume(pre_fade);
            }
            let action = self.take_sleep_timer_action();
            self.cancel_sleep_timer();
            self.run_sleep_action(&action);
            return;
        }
        let next = remaining - 1;
        self.set_sleep_timer_remaining(next);
        if next <= 10
            && let Some(mpv) = self.mpv_handle().borrow().as_ref()
        {
            mpv.set_volume(pre_fade * (f64::from(next) / 10.0));
        }
    }

    /// Run the chosen on-expiry action after the fade-out completes. A 2 s grace
    /// before quit/suspend/poweroff lets the toast show. Mirrors the audio port.
    fn run_sleep_action(&self, action: &str) {
        match action {
            "quit" => {
                self.notify(&i18n("Sleep timer: closing player"));
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
                    if let Some(app) = gtk::gio::Application::default() {
                        crate::settings::saving::quit_after_writes(&app);
                    }
                });
            }
            "suspend" => {
                self.notify(&i18n("Sleep timer: suspending computer"));
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
                    crate::settings::saving::after_writes(|| {
                        if let Err(error) =
                            big_app_kit::desktop::BigSystemPowerAction::Suspend.spawn_detached()
                        {
                            log::warn!("sleep timer: suspend failed: {error}");
                        }
                    });
                });
            }
            "poweroff" => {
                self.notify(&i18n("Sleep timer: shutting down"));
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
                    crate::settings::saving::after_writes(|| {
                        if let Err(error) =
                            big_app_kit::desktop::BigSystemPowerAction::PowerOff.spawn_detached()
                        {
                            log::warn!("sleep timer: poweroff failed: {error}");
                        }
                    });
                });
            }
            _ => self.notify(&i18n("Sleep timer: playback paused")),
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

/// Sleep-timer minutes → total seconds (`u32`, saturating). Pure conversion.
pub(super) fn sleep_minutes_to_secs(minutes: u32) -> u32 {
    minutes.saturating_mul(60)
}

/// Resolve a non-empty chapter title, falling back to `Chapter N` (1-based) when
/// mpv reports an empty or placeholder title.
fn chapter_title(raw: String, index: i64) -> String {
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
pub(super) fn show_screenshot_result(overlay: &adw::ToastOverlay, path: PathBuf, durable: bool) {
    let message = if durable {
        i18n("Screenshot saved")
    } else {
        i18n("Screenshot saved; disk synchronization could not be confirmed")
    };
    let toast = adw::Toast::builder()
        .title(message)
        .button_label(i18n("Open"))
        .timeout(5)
        .build();
    toast.connect_button_clicked(move |_| {
        big_app_kit::desktop::open_file(&path);
    });
    overlay.add_toast(toast);
}

/// Derive a safe screenshot filename from the media title and a unix timestamp:
/// `<sanitized-title>_<ts>.png`. Non-alphanumeric chars (except `-_.`) become
/// `_`, and any `..` run is collapsed to `_`; an empty/blank title falls back to
/// `screenshot`. Path-safe — the result never contains separators or `..`.
#[cfg(test)]
fn screenshot_filename(title: &str, timestamp: i64) -> String {
    let sanitized: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // Collapse any `..` (would form a parent-dir token if a separator ever crept
    // in) — defence in depth even though the result carries no separators.
    let safe = sanitized.replace("..", "_");
    let stem = if safe.trim_matches(['_', '.']).is_empty() {
        "screenshot"
    } else {
        safe.as_str()
    };
    format!("{stem}_{timestamp}.png")
}

#[cfg(test)]
mod tests {
    use super::{
        accepted_goto_seconds, chapter_title_with_prefix, parse_goto_seconds, screenshot_filename,
        sleep_minutes_to_secs,
    };

    #[test]
    fn goto_parses_plain_seconds() {
        assert_eq!(parse_goto_seconds("123.5"), Some(123.5));
    }

    #[test]
    fn goto_parses_minute_second() {
        assert_eq!(parse_goto_seconds("2:30"), Some(150.0));
    }

    #[test]
    fn goto_parses_hour_minute_second() {
        assert_eq!(parse_goto_seconds("1:02:03"), Some(3723.0));
    }

    #[test]
    fn goto_rejects_garbage() {
        assert_eq!(parse_goto_seconds("abc:def"), None);
    }

    #[test]
    fn goto_accepts_trimmed_dialog_input() {
        assert_eq!(accepted_goto_seconds(" 2:30 "), Some(150.0));
    }

    #[test]
    fn goto_rejects_blank_dialog_input() {
        assert_eq!(accepted_goto_seconds("   "), None);
    }

    #[test]
    fn sleep_minutes_convert_to_seconds() {
        assert_eq!(sleep_minutes_to_secs(0), 0);
        assert_eq!(sleep_minutes_to_secs(1), 60);
        assert_eq!(sleep_minutes_to_secs(30), 1800);
    }

    #[test]
    fn sleep_minutes_saturate_instead_of_overflow() {
        assert_eq!(sleep_minutes_to_secs(u32::MAX), u32::MAX);
    }

    #[test]
    fn screenshot_filename_sanitizes_and_appends_timestamp() {
        assert_eq!(
            screenshot_filename("My Movie: Part 1", 1700),
            "My_Movie__Part_1_1700.png"
        );
    }

    #[test]
    fn screenshot_filename_keeps_safe_chars() {
        assert_eq!(
            screenshot_filename("clip-01_final.v2", 42),
            "clip-01_final.v2_42.png"
        );
    }

    #[test]
    fn screenshot_filename_falls_back_on_blank_title() {
        assert_eq!(screenshot_filename("", 9), "screenshot_9.png");
        assert_eq!(screenshot_filename("   ", 9), "screenshot_9.png");
    }

    #[test]
    fn screenshot_filename_has_no_path_separators() {
        let name = screenshot_filename("a/b/../c", 1);
        assert!(!name.contains('/'));
        assert!(!name.contains(".."));
    }

    #[test]
    fn chapter_title_falls_back_on_empty_or_placeholder() {
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
