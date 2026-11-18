//! Sleep timer: labels, settings persistence, and the countdown both players run.

use big_relm4_components::display::sleep_timer::{
    BIG_SLEEP_ACTION_POWEROFF, BIG_SLEEP_ACTION_QUIT, BIG_SLEEP_ACTION_SUSPEND,
    BigSleepTimerAction, BigSleepTimerSetupLabels, standard_sleep_timer_actions,
};
use std::time::Duration;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

/// The volume fades out over the last seconds of the countdown.
const FADE_SECS: u32 = 10;
/// Delay before closing or powering off, so the notice can be read.
const ACTION_GRACE: Duration = Duration::from_secs(2);

pub fn sleep_timer_actions() -> Vec<BigSleepTimerAction> {
    standard_sleep_timer_actions(
        i18n("Stop playback"),
        i18n("Close the player"),
        i18n("Suspend computer"),
        i18n("Shut down computer"),
    )
}

pub fn sleep_timer_setup_labels() -> BigSleepTimerSetupLabels {
    BigSleepTimerSetupLabels {
        duration_group: i18n("Duration"),
        minutes_title: i18n("Minutes"),
        action_group: i18n("Action"),
        action_title: i18n("When timer ends"),
        start_button: i18n("Start Timer"),
    }
}

pub fn load_sleep_timer_preferences(settings: Option<&Settings>) -> (i32, String) {
    let saved_minutes = settings.map_or(30, |settings| {
        let value = settings.get_int(keys::SLEEP_TIMER_MINUTES);
        if value > 0 { value } else { 30 }
    });
    let saved_action = settings.map_or_else(
        || "pause".to_string(),
        |settings| settings.get_string(keys::SLEEP_TIMER_ACTION),
    );

    (saved_minutes, saved_action)
}

pub fn save_sleep_timer_preferences(settings: Option<&Settings>, minutes: u32, action: &str) {
    if let Some(settings) = settings {
        let _ = settings.put_int(keys::SLEEP_TIMER_MINUTES, minutes as i32);
        let _ = settings.put_string(keys::SLEEP_TIMER_ACTION, action);
    }
}

/// A sleep-timer countdown driven by a one-second source the timer owns.
///
/// The volume fades out over the last seconds, starting from the volume of that
/// moment, and is put back when the timer is cancelled or expires.
#[derive(Default)]
pub struct SleepTimer {
    source: Option<glib::SourceId>,
    remaining_secs: u32,
    action: String,
    /// Volume when the fade-out began; `None` until then.
    pre_fade_volume: Option<f64>,
}

#[derive(Debug, PartialEq)]
enum Step {
    Countdown,
    Fade(f64),
    Expire {
        action: String,
        restore_volume: Option<f64>,
    },
}

impl SleepTimer {
    pub fn is_armed(&self) -> bool {
        self.source.is_some()
    }

    pub fn remaining_secs(&self) -> u32 {
        self.remaining_secs
    }

    /// The countdown source, for checks that it does not outlive its player.
    pub fn source(&self) -> Option<&glib::SourceId> {
        self.source.as_ref()
    }

    /// Arm the timer for `minutes`, replacing any armed one; `on_tick` runs every
    /// second and must lead to [`SleepTimer::tick`].
    pub fn start(
        &mut self,
        minutes: u32,
        action: String,
        mpv: Option<&MpvBackend>,
        on_tick: impl Fn() + 'static,
    ) {
        self.cancel(mpv);
        self.arm(minutes, action);
        self.source = Some(glib::timeout_add_seconds_local(1, move || {
            on_tick();
            glib::ControlFlow::Continue
        }));
    }

    /// Disarm the timer, putting back the volume if the fade-out had begun.
    pub fn cancel(&mut self, mpv: Option<&MpvBackend>) {
        self.remove_source();
        if let Some(volume) = self.disarm()
            && let Some(mpv) = mpv
        {
            mpv.set_volume(volume);
        }
    }

    /// Count one second down. On expiry pause playback, run the chosen action and
    /// return the notice to show.
    pub fn tick(&mut self, mpv: Option<&MpvBackend>) -> Option<String> {
        if !self.is_armed() {
            return None;
        }
        match self.advance(|| mpv.map(MpvBackend::get_volume)) {
            Step::Countdown => None,
            Step::Fade(volume) => {
                if let Some(mpv) = mpv {
                    mpv.set_volume(volume);
                }
                None
            }
            Step::Expire {
                action,
                restore_volume,
            } => {
                self.remove_source();
                if let Some(mpv) = mpv {
                    mpv.pause();
                    if let Some(volume) = restore_volume {
                        mpv.set_volume(volume);
                    }
                }
                Some(run_expiry_action(&action))
            }
        }
    }

    fn arm(&mut self, minutes: u32, action: String) {
        self.remaining_secs = minutes.saturating_mul(60);
        self.action = action;
        self.pre_fade_volume = None;
    }

    fn disarm(&mut self) -> Option<f64> {
        self.remaining_secs = 0;
        self.pre_fade_volume.take()
    }

    fn remove_source(&mut self) {
        if let Some(source) = self.source.take() {
            big_gtk_kit::source::remove(source);
        }
    }

    /// `current_volume` is read only on the first second of the fade-out.
    fn advance(&mut self, current_volume: impl FnOnce() -> Option<f64>) -> Step {
        if self.remaining_secs <= 1 {
            let action = std::mem::take(&mut self.action);
            let restore_volume = self.disarm();
            return Step::Expire {
                action,
                restore_volume,
            };
        }
        self.remaining_secs -= 1;
        if self.remaining_secs > FADE_SECS {
            return Step::Countdown;
        }
        if self.pre_fade_volume.is_none() {
            self.pre_fade_volume = current_volume();
        }
        let Some(pre_fade) = self.pre_fade_volume else {
            return Step::Countdown;
        };
        Step::Fade(pre_fade * f64::from(self.remaining_secs) / f64::from(FADE_SECS))
    }
}

impl Drop for SleepTimer {
    fn drop(&mut self) {
        self.remove_source();
    }
}

/// Run `action` after a grace period and return its notice; any other action
/// only pauses.
#[expect(
    clippy::disallowed_methods,
    reason = "the owner of power actions, run after settings writes"
)]
fn run_expiry_action(action: &str) -> String {
    match action {
        BIG_SLEEP_ACTION_QUIT => {
            glib::timeout_add_local_once(ACTION_GRACE, || {
                if let Some(app) = gio::Application::default() {
                    crate::settings::saving::quit_after_writes(&app);
                }
            });
            i18n("Sleep timer: closing player")
        }
        BIG_SLEEP_ACTION_SUSPEND => {
            glib::timeout_add_local_once(ACTION_GRACE, || {
                crate::settings::saving::after_writes(|| {
                    if let Err(error) =
                        big_app_kit::desktop::BigSystemPowerAction::Suspend.spawn_detached()
                    {
                        log::warn!("sleep timer: suspend failed: {error}");
                    }
                });
            });
            i18n("Sleep timer: suspending computer")
        }
        BIG_SLEEP_ACTION_POWEROFF => {
            glib::timeout_add_local_once(ACTION_GRACE, || {
                crate::settings::saving::after_writes(|| {
                    if let Err(error) =
                        big_app_kit::desktop::BigSystemPowerAction::PowerOff.spawn_detached()
                    {
                        log::warn!("sleep timer: poweroff failed: {error}");
                    }
                });
            });
            i18n("Sleep timer: shutting down")
        }
        _ => i18n("Sleep timer: playback paused"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn armed(minutes: u32) -> SleepTimer {
        let mut timer = SleepTimer::default();
        timer.arm(minutes, "pause".into());
        timer
    }

    fn advance_to(timer: &mut SleepTimer, remaining_secs: u32, volume: f64) {
        while timer.remaining_secs > remaining_secs {
            timer.advance(|| Some(volume));
        }
    }

    #[test]
    fn fade_starts_from_current_volume() {
        let mut timer = armed(1);
        advance_to(&mut timer, 11, 80.0);
        assert_eq!(timer.advance(|| Some(40.0)), Step::Fade(40.0));
        assert_eq!(timer.advance(|| Some(90.0)), Step::Fade(36.0));
    }

    #[test]
    fn cancel_mid_fade_restores_volume() {
        let mut timer = armed(1);
        advance_to(&mut timer, 5, 60.0);
        assert_eq!(timer.disarm(), Some(60.0));
        assert_eq!(timer.remaining_secs, 0);

        let mut timer = armed(1);
        advance_to(&mut timer, 30, 60.0);
        assert_eq!(timer.disarm(), None, "no fade, nothing to restore");
    }

    #[test]
    fn expiry_returns_action_and_volume() {
        let mut timer = armed(1);
        timer.action = BIG_SLEEP_ACTION_SUSPEND.into();
        advance_to(&mut timer, 1, 70.0);
        assert_eq!(
            timer.advance(|| Some(0.0)),
            Step::Expire {
                action: BIG_SLEEP_ACTION_SUSPEND.into(),
                restore_volume: Some(70.0),
            }
        );
        assert_eq!(timer.remaining_secs, 0);
    }

    #[test]
    fn minutes_saturate_to_seconds() {
        assert_eq!(armed(30).remaining_secs, 1800);
        assert_eq!(armed(u32::MAX).remaining_secs, u32::MAX);
    }
}
