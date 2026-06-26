//! Audio cleanup dialog adapter.

use adw::prelude::*;
use big_media_components::audio::cleanup_dialog::{
    BigAudioCleanupDialogControls, BigAudioCleanupDialogText, show_audio_cleanup_dialog,
};

use crate::converter::ui::sidebar::Sidebar;
use crate::i18n::i18n;

/// Present the audio-cleanup dialog with the equalizer page enabled.
pub fn show(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    show_impl(parent, sidebar, true);
}

/// Present the audio-cleanup dialog without the equalizer page —
/// used when the equalizer is reachable through its own dedicated
/// dialog instead.
pub fn show_without_equalizer(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar) {
    show_impl(parent, sidebar, false);
}

fn show_impl(parent: &impl IsA<gtk::Widget>, sidebar: &Sidebar, include_equalizer: bool) {
    show_audio_cleanup_dialog(
        parent,
        BigAudioCleanupDialogText::translated(i18n),
        BigAudioCleanupDialogControls {
            noise_backend_available: crate::shared::audio_effects::has_noise_reduction_backend(),
            noise_enabled_switch: &sidebar.nr_enabled_switch,
            noise_strength_adjustment: &sidebar.nr_speech_adj,
            noise_model_dropdown: &sidebar.nr_model_combo,
            noise_model_labels: crate::shared::audio_effects::noise_model_labels(),
            noise_recovery_adjustment: &sidebar.nr_recovery_adj,
            gate_enabled_switch: &sidebar.gate_enabled_switch,
            gate_intensity_adjustment: &sidebar.gate_intensity_adj,
            high_pass_enabled_switch: &sidebar.hpf_enabled_switch,
            high_pass_frequency_adjustment: &sidebar.hpf_freq_adj,
            normalize_switch: &sidebar.normalize_switch,
            compressor_enabled_switch: &sidebar.compressor_enabled_switch,
            compressor_intensity_adjustment: &sidebar.compressor_intensity_adj,
            equalizer_enabled_switch: &sidebar.eq_enabled_switch,
            equalizer_preset_dropdown: &sidebar.eq_preset_combo,
            equalizer_band_adjustments: &sidebar.eq_band_adjs,
        },
        include_equalizer,
    );
}
