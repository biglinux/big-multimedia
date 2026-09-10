//! Audio + Music preferences pages (output, processing, language, ReplayGain,
//! crossfade, ListenBrainz). Also exposes `show_audio_preferences` for the
//! standalone audio-player binary.

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use big_player_ui::audio_preferences::{
    PlayerAudioEffects, PlayerAudioPreferenceActions, create_player_audio_page as create_audio_page,
};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::i18n::i18n;
use crate::player::mpv_backend::{AudioEffectsSnapshot, MpvBackend};
use crate::settings::{Settings, keys};
use crate::shared::preference_rows::{AdvancedWidgets, add_combo_row};
use crate::shared::ui_helpers::illustration_auto as illustration;
use big_media_runtime::i18n::gettext_noop;

#[derive(Debug, Clone, Copy, PartialEq)]
struct NumericAdjustmentSpec {
    lower: f64,
    upper: f64,
    step_increment: f64,
    page_increment: f64,
    page_size: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PreferencesPageChromeSpec {
    title: &'static str,
    icon_name: &'static str,
}

#[derive(Clone, Copy)]
struct AudioPreferencesWindowSpec {
    title: &'static str,
    default_width: i32,
    default_height: i32,
    saved_width_key: keys::IntKey,
    saved_height_key: keys::IntKey,
}

fn music_page_chrome_spec() -> PreferencesPageChromeSpec {
    PreferencesPageChromeSpec {
        title: gettext_noop("Music"),
        icon_name: "emblem-music-symbolic",
    }
}

fn replaygain_preamp_adjustment_spec() -> NumericAdjustmentSpec {
    NumericAdjustmentSpec {
        lower: -15.0,
        upper: 15.0,
        step_increment: 0.5,
        page_increment: 1.0,
        page_size: 0.0,
    }
}

fn crossfade_adjustment_spec() -> NumericAdjustmentSpec {
    NumericAdjustmentSpec {
        lower: 0.0,
        upper: 12.0,
        step_increment: 1.0,
        page_increment: 1.0,
        page_size: 0.0,
    }
}

fn audio_preferences_window_spec() -> AudioPreferencesWindowSpec {
    AudioPreferencesWindowSpec {
        title: gettext_noop("Preferences"),
        default_width: 550,
        default_height: 650,
        saved_width_key: keys::AUDIO_PREFS_WINDOW_WIDTH,
        saved_height_key: keys::AUDIO_PREFS_WINDOW_HEIGHT,
    }
}

fn should_create_audio_preference_actions(has_mpv_backend: bool) -> bool {
    has_mpv_backend
}

fn player_audio_effects_snapshot(effects: &PlayerAudioEffects) -> AudioEffectsSnapshot {
    AudioEffectsSnapshot {
        normalize: effects.normalize,
        nr_enabled: effects.nr_enabled,
        nr_strength: effects.nr_strength,
        nr_model: effects.nr_model,
        nr_model_blend: effects.nr_model_blend,
        nr_voice_recovery: effects.nr_voice_recovery,
        gate_enabled: false,
        gate_intensity: 0.0,
        hpf_enabled: false,
        hpf_frequency: 0.0,
        eq_bands: effects.eq_bands.clone(),
        compressor_enabled: false,
        compressor_intensity: 0.0,
    }
}

pub(super) fn create_music_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) -> adw::PreferencesPage {
    let page_spec = music_page_chrome_spec();
    let page = adw::PreferencesPage::builder()
        .title(i18n(page_spec.title))
        .icon_name(page_spec.icon_name)
        .build();

    let playback_group = BigPreferencesGroupSpec::new(i18n("Playback"))
        .description(i18n("Control how your music is played"))
        .build();

    let m = mpv.cloned();
    let rg_row = add_combo_row(
        &playback_group,
        &i18n("Volume Leveling"),
        &i18n(
            "Makes all songs play at a similar volume so you don't need to adjust between tracks",
        ),
        &[&i18n("Off"), &i18n("Per Song"), &i18n("Per Album")],
        &["no", "track", "album"],
        "replaygain",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_replaygain(v);
            }
        },
    );
    rg_row.add_prefix(&illustration("volume_leveling.svg"));

    let replaygain_preamp_adjustment = replaygain_preamp_adjustment_spec();
    let rg_preamp_row = adw::SpinRow::builder()
        .title(i18n("Volume Leveling Boost"))
        .subtitle(i18n(
            "Fine-tune the volume leveling — positive values make it louder, negative softer",
        ))
        .adjustment(&gtk::Adjustment::new(
            settings.get_double(keys::REPLAYGAIN_PREAMP),
            replaygain_preamp_adjustment.lower,
            replaygain_preamp_adjustment.upper,
            replaygain_preamp_adjustment.step_increment,
            replaygain_preamp_adjustment.page_increment,
            replaygain_preamp_adjustment.page_size,
        ))
        .build();
    rg_preamp_row.add_prefix(&illustration("volume_boost.svg"));
    {
        let s = settings.clone();
        let m = mpv.cloned();
        rg_preamp_row.connect_value_notify(move |row| {
            let val = row.value();
            let _ = s.put_double(keys::REPLAYGAIN_PREAMP, val);
            if let Some(ref mpv) = m {
                mpv.set_replaygain_preamp(val);
            }
        });
    }
    playback_group.add(&rg_preamp_row);

    let gapless_row = adw::SwitchRow::builder()
        .title(i18n("Gapless Playback"))
        .subtitle(i18n("Remove silence between songs for a continuous listening experience — great for live albums and DJ mixes"))
        .active(settings.get_bool(keys::GAPLESS_AUDIO))
        .build();
    gapless_row.add_prefix(&illustration("gapless.svg"));
    {
        let s = settings.clone();
        let m = mpv.cloned();
        gapless_row.connect_active_notify(move |row| {
            let _ = s.put_bool(keys::GAPLESS_AUDIO, row.is_active());
            if let Some(ref mpv) = m {
                mpv.set_gapless(row.is_active());
            }
        });
    }
    playback_group.add(&gapless_row);

    let resume_row = adw::SwitchRow::builder()
        .title(i18n("Resume Playback"))
        .subtitle(i18n(
            "When you play a song again, continue from where you left off instead of starting over",
        ))
        .active(settings.get_bool(keys::RESUME_PLAYBACK))
        .build();
    resume_row.add_prefix(&illustration("resume.svg"));
    {
        let s = settings.clone();
        let m = mpv.cloned();
        resume_row.connect_active_notify(move |row| {
            let _ = s.put_bool(keys::RESUME_PLAYBACK, row.is_active());
            if let Some(ref mpv) = m {
                mpv.set_save_position(row.is_active());
            }
        });
    }
    playback_group.add(&resume_row);

    let crossfade_adjustment = crossfade_adjustment_spec();
    let crossfade_row = adw::SpinRow::builder()
        .title(i18n("Crossfade"))
        .subtitle(i18n(
            "Blend the end of one song into the beginning of the next — set to 0 to disable",
        ))
        .adjustment(&gtk::Adjustment::new(
            f64::from(settings.get_int(keys::CROSSFADE_SECS)),
            crossfade_adjustment.lower,
            crossfade_adjustment.upper,
            crossfade_adjustment.step_increment,
            crossfade_adjustment.page_increment,
            crossfade_adjustment.page_size,
        ))
        .build();
    crossfade_row.add_prefix(&illustration("crossfade.svg"));
    {
        let s = settings.clone();
        crossfade_row.connect_value_notify(move |row| {
            let _ = s.put_int(keys::CROSSFADE_SECS, row.value() as i32);
        });
    }
    playback_group.add(&crossfade_row);

    // Taskbar progress (KDE Plasma / Unity → Unity launcher protocol).
    let taskbar_row = adw::SwitchRow::builder()
        .title(i18n("Taskbar Progress"))
        .subtitle(i18n(
            "Show a progress bar on the app icon in the taskbar while music is playing",
        ))
        .active(settings.get_bool_or(keys::TASKBAR_PROGRESS, true))
        .build();
    taskbar_row.add_prefix(&illustration("taskbar_progress.svg"));
    {
        let s = settings.clone();
        taskbar_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            let _ = s.put_bool(keys::TASKBAR_PROGRESS, enabled);
            if let Some(ref cb) = on_taskbar_toggle {
                cb(enabled);
            }
        });
    }
    playback_group.add(&taskbar_row);

    page.add(&playback_group);

    let lb_group = BigPreferencesGroupSpec::new("ListenBrainz")
        .description(i18n("Keep a record of everything you listen to on ListenBrainz — a free, open-source music tracking service"))
        .build();

    // Built empty; the on-disk config is read off the main loop below (a hung
    // $HOME must not block the preferences dialog) and applied when it lands.
    let lb_enabled = adw::SwitchRow::builder()
        .title(i18n("Enable ListenBrainz"))
        .build();
    lb_enabled.add_prefix(&illustration("listenbrainz.svg"));

    let lb_token = adw::EntryRow::builder().title(i18n("User Token")).build();

    // Set while the freshly-loaded config is applied, so the change handlers
    // below don't persist a half-applied value (e.g. enabled with no token yet).
    let suppress = Rc::new(Cell::new(false));

    let enabled_ref = lb_enabled.downgrade();
    let suppress_token = Rc::clone(&suppress);
    lb_token.connect_changed(move |entry| {
        if suppress_token.get() {
            return;
        }
        let Some(enabled_ref) = enabled_ref.upgrade() else {
            return;
        };
        let token = entry.text().to_string();
        let enabled = enabled_ref.is_active();
        let listenbrainz_config = crate::library::listenbrainz::ListenBrainzConfig {
            user_token: token,
            enabled,
        };
        let _ = listenbrainz_config.save();
    });

    let token_ref = lb_token.downgrade();
    let suppress_enabled = Rc::clone(&suppress);
    lb_enabled.connect_active_notify(move |row| {
        if suppress_enabled.get() {
            return;
        }
        let Some(token_ref) = token_ref.upgrade() else {
            return;
        };
        let token = token_ref.text().to_string();
        let listenbrainz_config = crate::library::listenbrainz::ListenBrainzConfig {
            user_token: token,
            enabled: row.is_active(),
        };
        let _ = listenbrainz_config.save();
    });

    lb_group.add(&lb_enabled);
    lb_group.add(&lb_token);
    page.add(&lb_group);

    {
        let lb_enabled = lb_enabled.downgrade();
        let lb_token = lb_token.downgrade();
        glib::spawn_future_local(async move {
            let config =
                gio::spawn_blocking(crate::library::listenbrainz::ListenBrainzConfig::load)
                    .await
                    .ok()
                    .flatten();
            let Some(config) = config else {
                return;
            };
            if let (Some(lb_enabled), Some(lb_token)) = (lb_enabled.upgrade(), lb_token.upgrade()) {
                suppress.set(true);
                lb_token.set_text(&config.user_token);
                lb_enabled.set_active(config.enabled);
                suppress.set(false);
            }
        });
    }

    page
}

fn audio_preference_actions(mpv: Option<&Arc<MpvBackend>>) -> Option<PlayerAudioPreferenceActions> {
    if should_create_audio_preference_actions(mpv.is_some()) {
        let mpv = mpv.cloned()?;
        return Some(PlayerAudioPreferenceActions::new(
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
        ));
    }
    None
}

fn apply_player_audio_effects(mpv: &MpvBackend, effects: PlayerAudioEffects) {
    let snapshot = player_audio_effects_snapshot(&effects);
    mpv.apply_audio_effects(&snapshot.as_effects());
}

/// `on_taskbar_toggle` fires when the taskbar-progress switch changes → MPRIS
/// service updates without a restart.
pub fn show_audio_preferences(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) {
    let window_spec = audio_preferences_window_spec();
    let saved = (
        settings.get_int(window_spec.saved_width_key),
        settings.get_int(window_spec.saved_height_key),
    );

    let mpv: Option<Arc<MpvBackend>> = None;
    let is_advanced = settings.get_bool(keys::ADVANCED_MODE);
    let adv_widgets: AdvancedWidgets = Rc::new(RefCell::new(Vec::new()));
    let pages = [
        create_audio_page(
            settings,
            audio_preference_actions(mpv.as_ref()),
            is_advanced,
            &adv_widgets,
            None,
        ),
        create_music_page(settings, mpv.as_ref(), on_taskbar_toggle),
    ];

    let s = settings.clone();
    big_player_ui::appearance::build_preferences_window(
        parent,
        &i18n(window_spec.title),
        true,
        saved,
        (window_spec.default_width, window_spec.default_height),
        &pages,
        move |w, h| {
            let _ = s.put_int(window_spec.saved_width_key, w);
            let _ = s.put_int(window_spec.saved_height_key, h);
        },
        || {},
    )
    .present();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_audio_effects_fixture() -> PlayerAudioEffects {
        PlayerAudioEffects {
            normalize: true,
            nr_enabled: true,
            nr_strength: 0.75,
            nr_model: 3,
            nr_model_blend: true,
            nr_voice_recovery: 0.5,
            eq_bands: vec![(60.0, -3.0), (1_000.0, 2.5)],
        }
    }

    #[test]
    fn music_page_chrome_names_music_surface() {
        assert_eq!(
            music_page_chrome_spec(),
            PreferencesPageChromeSpec {
                title: "Music",
                icon_name: "emblem-music-symbolic",
            }
        );
    }

    #[test]
    fn playback_adjustment_specs_keep_expected_ranges() {
        assert_eq!(
            replaygain_preamp_adjustment_spec(),
            NumericAdjustmentSpec {
                lower: -15.0,
                upper: 15.0,
                step_increment: 0.5,
                page_increment: 1.0,
                page_size: 0.0,
            }
        );
        assert_eq!(
            crossfade_adjustment_spec(),
            NumericAdjustmentSpec {
                lower: 0.0,
                upper: 12.0,
                step_increment: 1.0,
                page_increment: 1.0,
                page_size: 0.0,
            }
        );
    }

    #[test]
    fn audio_preferences_window_spec_uses_saved_geometry_keys() {
        let spec = audio_preferences_window_spec();

        assert_eq!(spec.title, "Preferences");
        assert_eq!(spec.default_width, 550);
        assert_eq!(spec.default_height, 650);
        assert_eq!(spec.saved_width_key.0, keys::AUDIO_PREFS_WINDOW_WIDTH.0);
        assert_eq!(spec.saved_height_key.0, keys::AUDIO_PREFS_WINDOW_HEIGHT.0);
    }

    #[test]
    fn audio_preference_actions_exist_only_with_backend() {
        assert!(should_create_audio_preference_actions(true));
        assert!(!should_create_audio_preference_actions(false));
    }

    #[test]
    fn player_audio_effects_snapshot_preserves_player_fields_and_disables_app_only_filters() {
        let effects = player_audio_effects_fixture();
        let snapshot = player_audio_effects_snapshot(&effects);

        assert_eq!(snapshot.normalize, effects.normalize);
        assert_eq!(snapshot.nr_enabled, effects.nr_enabled);
        assert_eq!(snapshot.nr_strength, effects.nr_strength);
        assert_eq!(snapshot.nr_model, effects.nr_model);
        assert_eq!(snapshot.nr_model_blend, effects.nr_model_blend);
        assert_eq!(snapshot.nr_voice_recovery, effects.nr_voice_recovery);
        assert_eq!(snapshot.eq_bands, effects.eq_bands);
        assert!(!snapshot.gate_enabled);
        assert_eq!(snapshot.gate_intensity, 0.0);
        assert!(!snapshot.hpf_enabled);
        assert_eq!(snapshot.hpf_frequency, 0.0);
        assert!(!snapshot.compressor_enabled);
        assert_eq!(snapshot.compressor_intensity, 0.0);
    }
}
