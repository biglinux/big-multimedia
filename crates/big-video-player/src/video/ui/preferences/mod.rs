//! Video player preferences: a native `adw::PreferencesDialog` with General,
//! Video, Audio, Subtitles and Advanced pages. Rows persist to
//! `crate::settings` and, when a player is running, apply to mpv at once.
//! Technical options live on the Advanced page instead of behind a mode switch.

use adw::prelude::*;
use big_player_ui::audio_preferences::{
    PlayerAudioEffects, PlayerAudioPreferenceActions, create_player_audio_page,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::{AudioEffects, MpvBackend};
use crate::settings::{Settings, keys};
use crate::shared::preference_rows::{
    add_color_row, add_combo_row, add_entry_row, add_spin_double_row, add_spin_int_row,
    add_switch_row,
};

mod advanced;
mod general;
mod subtitles;
mod video;

/// Present the preferences over `parent`. `mpv` is `None` before playback
/// starts; rows then only store their value. `on_restore_defaults` receives the
/// dialog so the confirmation can stack on it and close it afterwards.
/// Returns the presented dialog, for callers that react to it closing.
pub fn show_preferences(
    parent: &impl IsA<gtk::Widget>,
    settings: &Settings,
    mpv: Option<Arc<MpvBackend>>,
    on_restore_defaults: impl Fn(&adw::PreferencesDialog) + 'static,
) -> adw::PreferencesDialog {
    let mpv = mpv.as_ref();
    let dialog = adw::PreferencesDialog::builder()
        .title(i18n("Preferences"))
        .search_enabled(true)
        .content_width(720)
        .content_height(760)
        .build();

    let restore = {
        let dialog = dialog.downgrade();
        move || {
            if let Some(dialog) = dialog.upgrade() {
                on_restore_defaults(&dialog);
            }
        }
    };
    let (advanced_page, refresh_preset_rows) = advanced::page(settings, mpv);
    let audio_page = create_player_audio_page(settings, audio_preference_actions(mpv));
    let pages = [
        general::page(settings, mpv, restore),
        video::page(settings, mpv, refresh_preset_rows),
        audio_page,
        subtitles::page(settings, mpv),
        advanced_page,
    ];
    for page in &pages {
        crate::settings::attach_save_status_banner(page, settings);
        dialog.add(page);
    }
    dialog.present(Some(parent));
    dialog
}

/// Hand every stored preference to a freshly started player. The rows only
/// push values while the dialog is open, so without this a restart silently
/// fell back to mpv's defaults. Keep this list in step with the pages.
pub fn apply_saved_preferences(mpv: &MpvBackend, s: &Settings) {
    // The preset first: the individual rows below may refine what it set.
    mpv.apply_render_profile(&s.string(keys::RENDER_PROFILE.0));

    // General
    mpv.set_loop_mode(&s.string(keys::LOOP_MODE.0));
    mpv.set_save_position(s.boolean("resume-playback"));
    mpv.set_keep_open(&s.string("keep-open"));
    mpv.set_keep_open_pause(s.boolean("keep-open-pause"));

    // Video
    mpv.set_hwdec(&s.string(keys::HWDEC.0));
    mpv.set_deinterlace(s.boolean("deinterlace"));

    // Audio
    mpv.set_audio_pitch_correction(s.boolean("audio-pitch-correction"));
    mpv.set_audio_channels(&s.string("audio-channels"));
    mpv.set_preferred_audio_lang(&s.string("preferred-audio-lang"));
    crate::shared::audio_effects::apply_voice_change(s, mpv);

    // Subtitles
    mpv.set_preferred_sub_lang(&s.string("preferred-sub-lang"));
    mpv.set_sub_auto(&s.string("sub-auto"));
    mpv.set_sub_font(&s.string(keys::SUB_FONT.0));
    mpv.set_sub_font_size(i64::from(s.int("subtitle-font-size")));
    mpv.set_sub_bold(s.boolean("sub-bold"));
    mpv.set_sub_color(&s.string("sub-color"));
    mpv.set_sub_border_style(&s.string("sub-border-style"));
    mpv.set_sub_border_size(s.double("sub-border-size"));
    mpv.set_sub_border_color(&s.string("sub-border-color"));
    mpv.set_sub_shadow_offset(s.double("sub-shadow-offset"));
    mpv.set_sub_shadow_color(&s.string("sub-shadow-color"));
    mpv.set_sub_margin_y(i64::from(s.int("sub-margin-y")));
    mpv.set_sub_scale_with_window(s.boolean("sub-scale-with-window"));
    mpv.set_sub_ass_scale_with_window(s.boolean("sub-ass-scale-with-window"));
    mpv.set_sub_filter_sdh(s.boolean("sub-filter-sdh"));
    mpv.set_sub_filter_sdh_harder(s.boolean("sub-filter-sdh-harder"));
    mpv.set_sub_gauss(s.double("sub-gauss"));
    mpv.set_sub_gray(s.boolean("sub-gray"));

    // Advanced
    mpv.set_tone_mapping(&s.string("tone-mapping"));
    mpv.set_hdr_compute_peak(s.boolean(keys::HDR_COMPUTE_PEAK.0));
    mpv.set_target_peak(i64::from(s.int("target-peak")));
    mpv.set_gamut_mapping_mode(&s.string("gamut-mapping-mode"));
    let icc_profile = s.string("icc-profile");
    if !icc_profile.is_empty() {
        mpv.set_icc_profile(&icc_profile);
    }
    mpv.set_icc_intent(i64::from(s.int(keys::ICC_INTENT.0)));
    mpv.set_cache(s.boolean("cache"));
    mpv.set_cache_secs(i64::from(s.int("cache-secs")));
    mpv.set_cache_pause(s.boolean("cache-pause"));
    mpv.set_cache_on_disk(s.boolean("cache-on-disk"));
    mpv.set_demuxer_readahead_secs(s.double("demuxer-readahead-secs"));
    mpv.set_screenshot_png_compression(i64::from(s.int("screenshot-png-compression")));
    mpv.set_screenshot_tag_colorspace(s.boolean("screenshot-tag-colorspace"));
    mpv.set_screenshot_high_bit_depth(s.boolean("screenshot-high-bit-depth"));
    mpv.set_glsl_shaders(&s.string("glsl-shaders"));
    crate::video::ui::filters::apply_saved_enhancements(mpv, s);
}

/// Forward a row's new value to the running player, if there is one.
fn live<T: 'static>(
    mpv: Option<&Arc<MpvBackend>>,
    apply: fn(&MpvBackend, T),
) -> impl Fn(T) + 'static {
    let mpv = mpv.cloned();
    move |value| {
        if let Some(mpv) = &mpv {
            apply(mpv, value);
        }
    }
}

/// [`live`] for text values, which arrive borrowed.
fn live_str(
    mpv: Option<&Arc<MpvBackend>>,
    apply: fn(&MpvBackend, &str),
) -> impl Fn(&str) + 'static {
    let mpv = mpv.cloned();
    move |value| {
        if let Some(mpv) = &mpv {
            apply(mpv, value);
        }
    }
}

fn audio_preference_actions(mpv: Option<&Arc<MpvBackend>>) -> Option<PlayerAudioPreferenceActions> {
    let mpv = mpv.cloned()?;
    Some(PlayerAudioPreferenceActions::new(
        {
            let mpv = Arc::clone(&mpv);
            move |value| mpv.set_audio_channels(value)
        },
        {
            let mpv = Arc::clone(&mpv);
            move |value| mpv.set_audio_pitch_correction(value)
        },
        {
            let mpv = Arc::clone(&mpv);
            move |value| mpv.set_volume_max(value)
        },
        {
            let mpv = Arc::clone(&mpv);
            move |value| mpv.set_preferred_audio_lang(value)
        },
        move |effects| apply_player_audio_effects(&mpv, effects),
    ))
}

fn apply_player_audio_effects(mpv: &MpvBackend, effects: PlayerAudioEffects) {
    mpv.apply_audio_effects(&AudioEffects {
        normalize: effects.normalize,
        nr_enabled: effects.nr_enabled,
        nr_strength: effects.nr_strength,
        nr_model: effects.nr_model,
        nr_mono: effects.nr_mono,
        gate_enabled: false,
        gate_intensity: 0.0,
        hpf_enabled: false,
        hpf_frequency: 0.0,
        eq_bands: &effects.eq_bands,
        compressor_enabled: false,
        compressor_intensity: 0.0,
    });
}
