//! Audio player preferences: a native `adw::PreferencesDialog` with the Music
//! page (volume leveling, gapless, resume, taskbar progress, tray icon) and the Audio page
//! shared with the video player.

use adw::prelude::*;
use std::sync::Arc;

use big_player_ui::audio_preferences::{
    PlayerAudioPreferenceActions, create_player_audio_page as create_audio_page,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};
use big_player_ui::preference_rows::add_combo_row;
use big_relm4_components::input::spin_row::accessible_spin_row;

/// What the shell does when a desktop-integration switch changes, so the
/// choice applies without a restart.
pub struct DesktopToggles {
    /// Taskbar progress: the MPRIS service shows or clears it.
    pub taskbar_progress: Box<dyn Fn(bool) + 'static>,
    /// Tray icon: the shell adds or removes it.
    pub tray_icon: Box<dyn Fn(bool) + 'static>,
}

pub(super) fn create_music_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    toggles: DesktopToggles,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Music"))
        .icon_name("emblem-music-symbolic")
        .build();

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
        &MpvBackend::REPLAYGAIN_MODES,
        keys::REPLAYGAIN,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_replaygain(v);
            }
        },
    );

    let rg_preamp_row = accessible_spin_row()
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

    // Taskbar progress (KDE Plasma / Unity → Unity launcher protocol).
    let taskbar_row = adw::SwitchRow::builder()
        .title(i18n("Progress on the Taskbar"))
        .subtitle(i18n("Shows how far the song has played on the player icon"))
        .active(settings.get_bool_or(keys::TASKBAR_PROGRESS, true))
        .build();
    {
        let s = settings.clone();
        let on_toggle = toggles.taskbar_progress;
        taskbar_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            let _ = s.put_bool(keys::TASKBAR_PROGRESS, enabled);
            on_toggle(enabled);
        });
    }
    playback_group.add(&taskbar_row);

    let tray_row = adw::SwitchRow::builder()
        .title(i18n("Icon in the System Tray"))
        .subtitle(i18n(
            "A one-color icon that follows the panel theme, with playback controls",
        ))
        .active(settings.get_bool_or(keys::TRAY_ICON, false))
        .build();
    {
        let s = settings.clone();
        let on_toggle = toggles.tray_icon;
        tray_row.connect_active_notify(move |row| {
            let enabled = row.is_active();
            let _ = s.put_bool(keys::TRAY_ICON, enabled);
            on_toggle(enabled);
        });
    }
    playback_group.add(&tray_row);

    page.add(&playback_group);

    page
}

fn audio_preference_actions(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    volume_max_changed: impl Fn() + 'static,
) -> Option<PlayerAudioPreferenceActions> {
    let mpv = mpv.cloned()?;
    let settings = settings.clone();
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
        move || crate::shared::audio_effects::apply_voice_change(&settings, &mpv),
    ))
}

/// `toggles` and `volume_max_changed` fire when their preference changes, so
/// the shell applies it without a restart.
pub fn show_audio_preferences(
    parent: &impl IsA<gtk::Widget>,
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    toggles: DesktopToggles,
    volume_max_changed: impl Fn() + 'static,
) {
    let dialog = adw::PreferencesDialog::builder()
        .title(i18n("Preferences"))
        .search_enabled(true)
        .content_width(640)
        .content_height(720)
        .build();
    let pages = [
        create_music_page(settings, mpv, toggles),
        create_audio_page(
            settings,
            audio_preference_actions(settings, mpv, volume_max_changed),
        ),
    ];
    for page in &pages {
        crate::settings::attach_save_status_banner(page, settings);
        dialog.add(page);
    }
    dialog.present(Some(parent));
}
