//! Equalizer dialog adapter.

use adw::prelude::*;
use big_media_components::audio::equalizer_dialog::{
    BigAudioEqualizerDialogControls, BigAudioEqualizerDialogText, show_audio_equalizer_dialog,
};

use crate::converter::ui::sidebar::Sidebar;
use crate::i18n::i18n;

/// Present the equalizer dialog modally on `parent`, binding the
/// preset dropdown and band adjustments to `sidebar`.
pub fn show(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    show_audio_equalizer_dialog(
        parent,
        BigAudioEqualizerDialogText::translated(i18n),
        BigAudioEqualizerDialogControls {
            enabled_switch: &sidebar.eq_enabled_switch,
            preset_dropdown: &sidebar.eq_preset_combo,
            band_adjustments: &sidebar.eq_band_adjs,
        },
    );
}
