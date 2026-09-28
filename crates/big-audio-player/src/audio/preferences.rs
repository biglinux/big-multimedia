//! Audio player preferences: a native `adw::PreferencesDialog` with the Music
//! page (volume leveling, gapless, resume, taskbar progress) and the Audio page
//! shared with the video player.

use adw::prelude::*;
use std::sync::Arc;

use big_player_ui::audio_preferences::{
    PlayerAudioEffects, PlayerAudioPreferenceActions, create_player_audio_page as create_audio_page,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::{AudioEffectsSnapshot, MpvBackend};
use crate::settings::{Settings, keys};
use crate::shared::preference_rows::add_combo_row;
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

fn should_create_audio_preference_actions(has_mpv_backend: bool) -> bool {
    has_mpv_backend
}

fn player_audio_effects_snapshot(effects: &PlayerAudioEffects) -> AudioEffectsSnapshot {
    AudioEffectsSnapshot {
        normalize: effects.normalize,
        nr_enabled: effects.nr_enabled,
        nr_strength: effects.nr_strength,
        nr_model: effects.nr_model,
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

    page.add(&super::account_security::group());

    let leveling_group = adw::PreferencesGroup::builder()
        .title(i18n("Volume Leveling"))
        .description(i18n(
            "Uses the loudness stored in the files so you do not have to adjust the volume between songs",
        ))
        .build();

    let m = mpv.cloned();
    let leveling_row = add_combo_row(
        &leveling_group,
        &i18n("Level By"),
        "",
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

    let replaygain_preamp_adjustment = replaygain_preamp_adjustment_spec();
    let rg_preamp_row = adw::SpinRow::builder()
        .title(i18n("Leveling Boost"))
        .subtitle(i18n(
            "In decibels; positive values play louder, negative softer",
        ))
        .digits(1)
        .adjustment(&gtk::Adjustment::new(
            settings.get_double(keys::REPLAYGAIN_PREAMP),
            replaygain_preamp_adjustment.lower,
            replaygain_preamp_adjustment.upper,
            replaygain_preamp_adjustment.step_increment,
            replaygain_preamp_adjustment.page_increment,
            replaygain_preamp_adjustment.page_size,
        ))
        .build();
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
    leveling_group.add(&rg_preamp_row);
    // "Off" (index 0) ignores the stored loudness, so the boost has no effect.
    leveling_row
        .bind_property("selected", &rg_preamp_row, "sensitive")
        .transform_to(|_, selected: u32| Some(selected != 0))
        .sync_create()
        .build();
    page.add(&leveling_group);

    let playback_group = adw::PreferencesGroup::builder()
        .title(i18n("Playback"))
        .build();

    let gapless_row = adw::SwitchRow::builder()
        .title(i18n("No Gaps Between Songs"))
        .subtitle(i18n(
            "Albums recorded as one continuous piece play without pauses",
        ))
        .active(settings.get_bool(keys::GAPLESS_AUDIO))
        .build();
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
        .title(i18n("Resume Where You Left Off"))
        .subtitle(i18n("Playing a song again continues from where it stopped"))
        .active(settings.get_bool(keys::RESUME_PLAYBACK))
        .build();
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

    // Do not expose CROSSFADE_SECS until a two-track overlap engine consumes
    // it. Existing saved values are deliberately retained for future migration.

    // Taskbar progress (KDE Plasma / Unity → Unity launcher protocol).
    let taskbar_row = adw::SwitchRow::builder()
        .title(i18n("Progress on the Taskbar"))
        .subtitle(i18n("Shows how far the song has played on the player icon"))
        .active(settings.get_bool_or(keys::TASKBAR_PROGRESS, true))
        .build();
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
    parent: &impl IsA<gtk::Widget>,
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) {
    let dialog = adw::PreferencesDialog::builder()
        .title(i18n("Preferences"))
        .search_enabled(true)
        .content_width(640)
        .content_height(720)
        .build();
    let pages = [
        create_music_page(settings, mpv, on_taskbar_toggle),
        create_audio_page(settings, audio_preference_actions(mpv)),
    ];
    for page in &pages {
        crate::settings::attach_save_status_banner(page, settings);
        dialog.add(page);
    }
    dialog.present(Some(parent));
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
        assert_eq!(snapshot.eq_bands, effects.eq_bands);
        assert!(!snapshot.gate_enabled);
        assert_eq!(snapshot.gate_intensity, 0.0);
        assert!(!snapshot.hpf_enabled);
        assert_eq!(snapshot.hpf_frequency, 0.0);
        assert!(!snapshot.compressor_enabled);
        assert_eq!(snapshot.compressor_intensity, 0.0);
    }
}
