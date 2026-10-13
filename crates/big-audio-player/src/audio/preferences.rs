//! Audio player preferences: a native `adw::PreferencesDialog` with the Music
//! page (volume leveling, gapless, resume, taskbar progress) and the Audio page
//! shared with the video player.

use adw::prelude::*;
use std::sync::Arc;

use big_player_ui::audio_preferences::{
    PlayerAudioEffects, PlayerAudioPreferenceActions, create_player_audio_page as create_audio_page,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::{AudioEffects, MpvBackend};
use crate::settings::{Settings, keys};
use crate::shared::preference_rows::add_combo_row;

pub(super) fn create_music_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Music"))
        .icon_name("emblem-music-symbolic")
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

    let rg_preamp_row = adw::SpinRow::builder()
        .title(i18n("Leveling Boost"))
        .subtitle(i18n(
            "In decibels; positive values play louder, negative softer",
        ))
        .digits(1)
        .adjustment(&gtk::Adjustment::new(
            settings.get_double(keys::REPLAYGAIN_PREAMP),
            -15.0,
            15.0,
            0.5,
            1.0,
            0.0,
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
        nr_strength: effects.nr_strength,
        nr_model: effects.nr_model,
        nr_mono: effects.nr_mono,
        nr_voice_only: effects.nr_voice_only,
        eq_bands: &effects.eq_bands,
    });
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
