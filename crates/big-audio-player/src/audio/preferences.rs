//! Audio + Music preferences pages (output, processing, language, ReplayGain,
//! crossfade, ListenBrainz). Also exposes `show_audio_preferences` for the
//! standalone audio-player binary.

use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use big_player_ui::audio_preferences::{
    create_player_audio_page as create_audio_page, PlayerAudioEffects, PlayerAudioPreferenceActions,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::{AudioEffects, MpvBackend};
use crate::settings::{keys, Settings};
use crate::shared::preference_rows::{add_combo_row, AdvancedWidgets};
use crate::shared::ui_helpers::illustration_auto as illustration;

pub(super) fn create_music_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Music"))
        .icon_name("emblem-music-symbolic")
        .build();

    let playback_group = adw::PreferencesGroup::builder()
        .title(i18n("Playback"))
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

    let rg_preamp_row = adw::SpinRow::builder()
        .title(i18n("Volume Leveling Boost"))
        .subtitle(i18n(
            "Fine-tune the volume leveling — positive values make it louder, negative softer",
        ))
        .adjustment(&gtk::Adjustment::new(
            settings.get_double(keys::REPLAYGAIN_PREAMP),
            -15.0,
            15.0,
            0.5,
            1.0,
            0.0,
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

    let crossfade_row = adw::SpinRow::builder()
        .title(i18n("Crossfade"))
        .subtitle(i18n(
            "Blend the end of one song into the beginning of the next — set to 0 to disable",
        ))
        .adjustment(&gtk::Adjustment::new(
            f64::from(settings.get_int(keys::CROSSFADE_SECS)),
            0.0,
            12.0,
            1.0,
            1.0,
            0.0,
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

    let lb_group = adw::PreferencesGroup::builder()
        .title("ListenBrainz")
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
        nr_model_blend: effects.nr_model_blend,
        nr_voice_recovery: effects.nr_voice_recovery,
        gate_enabled: false,
        gate_intensity: 0.0,
        hpf_enabled: false,
        hpf_frequency: 0.0,
        eq_bands: &effects.eq_bands,
        compressor_enabled: false,
        compressor_intensity: 0.0,
    });
}

/// `on_taskbar_toggle` fires when the taskbar-progress switch changes → MPRIS
/// service updates without a restart.
pub fn show_audio_preferences(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_taskbar_toggle: Option<Box<dyn Fn(bool) + 'static>>,
) {
    let saved = (
        settings.get_int(keys::AUDIO_PREFS_WINDOW_WIDTH),
        settings.get_int(keys::AUDIO_PREFS_WINDOW_HEIGHT),
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
        &i18n("Preferences"),
        true,
        saved,
        (550, 650),
        &pages,
        move |w, h| {
            let _ = s.put_int(keys::AUDIO_PREFS_WINDOW_WIDTH, w);
            let _ = s.put_int(keys::AUDIO_PREFS_WINDOW_HEIGHT, h);
        },
        || {},
    )
    .present();
}
