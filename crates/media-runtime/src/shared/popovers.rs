//! Player popover builders re-exported from `big-media-components::player_chrome`
//! (pure-gtk, reusable). The mpv/settings-bound helpers below stay here — they
//! are engine-coupled (`MpvBackend`, audio-effects, settings).
//! bigagents: app-local-adapter; remaining helpers are mpv/settings-bound.

pub use big_media_components::player_chrome::{
    SPEED_PRESETS, VOICE_PRESETS, build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group, setup_hover_controller_exclusive,
};

use gtk::prelude::*;

use crate::player::mpv_backend::MpvBackend;
use crate::settings::{self, Settings, keys};
use crate::shared::audio_effects;
use std::cell::Cell;

/// Persists strength + writes GTCRN controls. Toggles the NR pipeline only
/// when the active state crosses; returns the new `is_active`.
pub fn handle_voice_strength_changed(
    settings: &Settings,
    mpv: Option<&MpvBackend>,
    pct: f64,
    prev_active: &Cell<bool>,
) -> bool {
    let strength = pct / 100.0;
    let _ = settings.put_double(keys::NOISE_STRENGTH, strength);
    audio_effects::write_gtcrn_controls(settings);

    let is_active = strength > 0.0;
    let was_active = prev_active.get();
    if was_active != is_active
        && let Some(mpv) = mpv
    {
        audio_effects::apply_voice_change(settings, mpv);
    }
    prev_active.set(is_active);
    is_active
}

pub fn show_nr_settings_dialog(settings: &Settings, parent: &impl gtk::prelude::IsA<gtk::Window>) {
    let dialog = audio_effects::create_nr_settings_dialog(settings);
    dialog.set_transient_for(Some(parent));
    settings::bind_dialog_size_persistence(&dialog, settings, "nr-settings", 840, 580);
    dialog.present();
}
