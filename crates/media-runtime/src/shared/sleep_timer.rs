//! Shared sleep timer labels, actions, and settings persistence.
//! bigagents: app-local-adapter; binds component labels to app i18n/settings.

use big_relm4_components::display::sleep_timer::{
    BigSleepTimerAction, BigSleepTimerSetupLabels, standard_sleep_timer_actions,
};

use crate::i18n::i18n;
use crate::settings::{Settings, keys};

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
