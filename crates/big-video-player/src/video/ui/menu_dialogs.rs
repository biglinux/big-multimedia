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
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};

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
        let (dialog, entry) = big_app_kit::dialogs::entry_dialog(
            &i18n("Go to Time"),
            &i18n("Enter time as H:MM:SS, M:SS, or seconds"),
            &i18n("Cancel"),
            "go",
            &i18n("Go"),
            "0:00:00",
            &i18n("Playback time"),
        );

        let mpv = self.mpv_handle().clone();
        dialog.connect_response(None, move |_dialog, response| {
            if response != "go" {
                return;
            }
            let text = entry.text().trim().to_string();
            if let Some(secs) = parse_goto_seconds(&text) {
                if let Some(mpv) = mpv.borrow().as_ref() {
                    mpv.seek_absolute(secs);
                }
            }
        });
        dialog.present(Some(root));
    }

    /// Capture the current frame to the XDG Pictures dir (home fallback) via
    /// `mpv.screenshot_to_file(path, "video")`, then toast the result with an
    /// Open button. Mirrors the old screenshot overlay (it shelled out to ffmpeg;
    /// the libmpv command is simpler and stays in-process). No-op + "failed"
    /// toast when nothing is playing.
    pub(super) fn take_screenshot(&self) {
        let mpv_ref = self.mpv_handle();
        let mpv_borrow = mpv_ref.borrow();
        let Some(mpv) = mpv_borrow.as_ref() else {
            self.notify(&i18n("Screenshot failed"));
            return;
        };
        if mpv.get_current_file().is_none() {
            self.notify(&i18n("Screenshot failed"));
            return;
        }

        let dir = screenshot_dir();
        let _ = std::fs::create_dir_all(&dir);
        let title = mpv.get_media_title().unwrap_or_default();
        let timestamp = glib::real_time() / 1_000_000; // µs → s
        let path = dir.join(screenshot_filename(&title, timestamp));

        // mpv writes the file synchronously; the path is app-derived (no
        // untrusted input). "video" = the decoded frame without OSD/subtitles.
        mpv.screenshot_to_file(&path.to_string_lossy(), "video");
        drop(mpv_borrow);

        if path.exists() {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let toast = adw::Toast::builder()
                .title(format!("{}: {name}", i18n("Screenshot saved")))
                .button_label(i18n("Open"))
                .timeout(5)
                .build();
            toast.connect_button_clicked(move |_| {
                big_app_kit::desktop::open_file(&path);
            });
            self.toast_overlay().add_toast(toast);
        } else {
            self.notify(&i18n("Screenshot failed"));
        }
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

        let group = adw::PreferencesGroup::new();
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

        let page = adw::PreferencesPage::new();
        page.add(&group);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&page));
        let dialog = adw::Dialog::builder()
            .title(i18n("Chapters"))
            .content_width(420)
            .content_height(520)
            .child(&toolbar)
            .build();
        dialog.present(Some(root));
    }

    /// Present the sleep-timer dialog: the active countdown (with cancel) when a
    /// timer is armed, otherwise the setup page (duration + on-expiry action).
    /// Mirrors the old `show_sleep_timer_dialog` and the audio player's port,
    /// reusing the shared `BigSleepTimer*` components + `shared::sleep_timer`
    /// label/persistence helpers.
    #[allow(deprecated)] // PreferencesWindow (adw 1.6) kept for resizability.
    pub(super) fn show_sleep_timer_dialog(
        &self,
        root: &adw::ApplicationWindow,
        sender: &ComponentSender<Self>,
    ) {
        use big_relm4_components::display::sleep_timer::{
            connect_sleep_timer_start, BigSleepTimerActiveLabels, BigSleepTimerActivePage,
            BigSleepTimerActiveSpec, BigSleepTimerSetupPage, BigSleepTimerSetupSpec,
        };

        let active = self.sleep_timer_is_armed();

        #[allow(deprecated)] // PreferencesDialog (adw 1.6) is not resizable.
        let dialog = adw::PreferencesWindow::builder()
            .title(i18n("Sleep Timer"))
            .default_width(600)
            .default_height(if active { 440 } else { 620 })
            .modal(true)
            .transient_for(root)
            .search_enabled(false)
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
            connect_sleep_timer_start(&setup_page, &dialog, root, move |_, minutes, action| {
                crate::shared::sleep_timer::save_sleep_timer_preferences(
                    Some(&settings),
                    minutes,
                    &action,
                );
                let _ = input.send(VideoShellInput::SleepTimerStart { minutes, action });
            });
            setup_page.into_page()
        };

        dialog.add(&page);
        dialog.present();
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
        if next <= 10 {
            if let Some(mpv) = self.mpv_handle().borrow().as_ref() {
                mpv.set_volume(pre_fade * (f64::from(next) / 10.0));
            }
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
                        app.quit();
                    }
                });
            }
            "suspend" => {
                self.notify(&i18n("Sleep timer: suspending computer"));
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
                    let _ = big_app_kit::desktop::BigSystemPowerAction::Suspend.spawn_detached();
                });
            }
            "poweroff" => {
                self.notify(&i18n("Sleep timer: shutting down"));
                glib::timeout_add_local_once(std::time::Duration::from_secs(2), || {
                    let _ = big_app_kit::desktop::BigSystemPowerAction::PowerOff.spawn_detached();
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

/// XDG Pictures dir for screenshots, falling back to home.
fn screenshot_dir() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Derive a safe screenshot filename from the media title and a unix timestamp:
/// `<sanitized-title>_<ts>.png`. Non-alphanumeric chars (except `-_.`) become
/// `_`, and any `..` run is collapsed to `_`; an empty/blank title falls back to
/// `screenshot`. Path-safe — the result never contains separators or `..`.
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
        chapter_title_with_prefix, parse_goto_seconds, screenshot_filename, sleep_minutes_to_secs,
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
