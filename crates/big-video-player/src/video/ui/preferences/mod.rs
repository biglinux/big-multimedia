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
use big_player_ui::preference_rows::{
    add_combo_row, add_entry_row, add_index_combo_row, add_mpv_color_row, add_spin_double_row,
    add_spin_int_row, add_switch_row,
};

mod advanced;
pub(crate) mod general;
mod subtitles;
mod video;

/// Present the preferences over `parent`. `mpv` is `None` before playback
/// starts; rows then only store their value. `volume_max_changed` fires after
/// mpv took a new Maximum Volume. `on_restore_defaults` receives the dialog so
/// the confirmation can stack on it and close it afterwards.
/// Returns the presented dialog, for callers that react to it closing.
pub fn show_preferences(
    parent: &impl IsA<gtk::Widget>,
    settings: &Settings,
    mpv: Option<Arc<MpvBackend>>,
    volume_max_changed: impl Fn() + 'static,
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
    let audio_page =
        create_player_audio_page(settings, audio_preference_actions(mpv, volume_max_changed));
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
    mpv.apply_render_profile(&s.get_string(keys::RENDER_PROFILE));

    // General
    mpv.set_loop_mode(&s.get_string(keys::LOOP_MODE));
    mpv.set_save_position(s.get_bool(keys::RESUME_PLAYBACK));
    mpv.set_keep_open(&s.get_string(keys::KEEP_OPEN));
    mpv.set_keep_open_pause(s.get_bool(keys::KEEP_OPEN_PAUSE));
    // An empty playlist with `idle=no` would make mpv quit; a load applies it.
    if mpv.get_current_file().is_some() {
        mpv.set_idle(&s.get_string(keys::IDLE_MODE));
    }

    // Video
    mpv.set_hwdec(&s.get_string(keys::HWDEC));
    mpv.set_deinterlace(s.get_bool(keys::DEINTERLACE));

    // Audio
    mpv.set_volume_max(i64::from(s.volume_max_percent()));
    mpv.set_audio_pitch_correction(s.get_bool(keys::AUDIO_PITCH_CORRECTION));
    mpv.set_audio_channels(&s.get_string(keys::AUDIO_CHANNELS));
    mpv.set_preferred_audio_lang(&s.get_string(keys::PREFERRED_AUDIO_LANG));
    crate::shared::audio_effects::apply_voice_change(s, mpv);

    // Subtitles
    mpv.set_preferred_sub_lang(&s.get_string(keys::PREFERRED_SUB_LANG));
    mpv.set_sub_auto(&s.get_string(keys::SUB_AUTO));
    mpv.set_sub_font(&s.get_string(keys::SUB_FONT));
    mpv.set_sub_font_size(i64::from(s.get_int(keys::SUBTITLE_FONT_SIZE)));
    mpv.set_sub_bold(s.get_bool(keys::SUB_BOLD));
    mpv.set_sub_color(&s.get_string(keys::SUB_COLOR));
    mpv.set_sub_border_style(&s.get_string(keys::SUB_BORDER_STYLE));
    mpv.set_sub_border_size(s.get_double(keys::SUB_BORDER_SIZE));
    mpv.set_sub_border_color(&s.get_string(keys::SUB_BORDER_COLOR));
    mpv.set_sub_shadow_offset(s.get_double(keys::SUB_SHADOW_OFFSET));
    mpv.set_sub_shadow_color(&s.get_string(keys::SUB_SHADOW_COLOR));
    mpv.set_sub_margin_y(i64::from(s.get_int(keys::SUB_MARGIN_Y)));
    mpv.set_sub_scale_with_window(s.get_bool(keys::SUB_SCALE_WITH_WINDOW));
    mpv.set_sub_ass_scale_with_window(s.get_bool(keys::SUB_ASS_SCALE_WITH_WINDOW));
    mpv.set_sub_filter_sdh(s.get_bool(keys::SUB_FILTER_SDH));
    mpv.set_sub_filter_sdh_harder(s.get_bool(keys::SUB_FILTER_SDH_HARDER));
    mpv.set_sub_gauss(s.get_double(keys::SUB_GAUSS));
    mpv.set_sub_gray(s.get_bool(keys::SUB_GRAY));

    // Advanced
    mpv.set_tone_mapping(&s.get_string(keys::TONE_MAPPING));
    mpv.set_hdr_compute_peak(s.get_bool(keys::HDR_COMPUTE_PEAK));
    mpv.set_target_peak(i64::from(s.get_int(keys::TARGET_PEAK)));
    mpv.set_gamut_mapping_mode(&s.get_string(keys::GAMUT_MAPPING_MODE));
    mpv.set_icc_profile(&s.get_string(keys::ICC_PROFILE));
    mpv.set_icc_intent(i64::from(s.get_int(keys::ICC_INTENT)));
    mpv.set_cache(s.get_bool(keys::CACHE));
    mpv.set_cache_secs(i64::from(s.get_int(keys::CACHE_SECS)));
    mpv.set_cache_pause(s.get_bool(keys::CACHE_PAUSE));
    mpv.set_cache_on_disk(s.get_bool(keys::CACHE_ON_DISK));
    mpv.set_demuxer_readahead_secs(s.get_double(keys::DEMUXER_READAHEAD_SECS));
    mpv.set_screenshot_png_compression(i64::from(s.get_int(keys::SCREENSHOT_PNG_COMPRESSION)));
    mpv.set_screenshot_tag_colorspace(s.get_bool(keys::SCREENSHOT_TAG_COLORSPACE));
    mpv.set_screenshot_high_bit_depth(s.get_bool(keys::SCREENSHOT_HIGH_BIT_DEPTH));
    mpv.set_glsl_shaders(&s.get_string(keys::GLSL_SHADERS));
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

fn audio_preference_actions(
    mpv: Option<&Arc<MpvBackend>>,
    volume_max_changed: impl Fn() + 'static,
) -> Option<PlayerAudioPreferenceActions> {
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
            move |value| {
                mpv.set_volume_max(value);
                volume_max_changed();
            }
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
        nr_strength: effects.nr_strength,
        nr_model: effects.nr_model,
        nr_mono: effects.nr_mono,
        nr_voice_only: effects.nr_voice_only,
        eq_bands: &effects.eq_bands,
    });
}
