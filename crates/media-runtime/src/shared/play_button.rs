//! Play/Pause paused-state sync (icon + tooltip + accessible label). The pulse
//! animation is re-exported from `big-media-components::player_chrome`.
//! bigagents: app-local-adapter; binds shared pulse/icon helpers to runtime i18n.

use gtk::prelude::*;

use big_media_components::playback::icons::playback_icon;

pub use big_media_components::player_chrome::pulse_play_button;

pub fn update_play_button(button: &gtk::Button, paused: bool) {
    let icon = playback_icon(paused);
    let label = if paused {
        crate::i18n::i18n("Play")
    } else {
        crate::i18n::i18n("Pause")
    };
    button.set_icon_name(icon);
    // `update` is a no-op pre-`setup_tooltips`; safe in tests.
    crate::shared::tooltip::update(button, &label);
    button.update_property(&[gtk::accessible::Property::Label(&label)]);
}
