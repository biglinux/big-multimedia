//! Video player preferences dialog — categorized `adw::PreferencesPage`s
//! (Video / Audio / Subtitles / General) of switch/spin/dropdown rows bound to
//! `crate::settings` keys, some also applying an mpv property live. Ported
//! faithfully from the old god-object's `src/video/preferences/**` (same
//! categories, rows, keys, and live mpv application); the only change is the
//! app-local module path and the standalone `show_preferences` entry point that
//! the Relm4 `VideoShell` calls instead of the old `Window` method.

use crate::shared::preference_rows::{
    AdvancedWidgets, add_color_row, add_combo_row, add_entry_row, add_spin_double_row,
    add_spin_int_row, add_switch_row,
};
use adw::prelude::*;
use big_player_ui::audio_preferences::{
    PlayerAudioEffects, PlayerAudioPreferenceActions, create_player_audio_page as create_audio_page,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::settings::{Settings, keys};

use crate::player::mpv_backend::{AudioEffects, MpvBackend};

fn find_header_bar(widget: &gtk::Widget) -> Option<adw::HeaderBar> {
    if let Ok(hb) = widget.clone().downcast::<adw::HeaderBar>() {
        return Some(hb);
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(hb) = find_header_bar(&c) {
            return Some(hb);
        }
        child = c.next_sibling();
    }
    None
}

/// Present the multi-category preferences window over `parent`, bound to
/// `settings` and (when up) the live mpv backend. `mpv` may be `None` before
/// playback starts — rows then only persist their settings.
pub fn show_preferences(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    mpv: Option<Arc<MpvBackend>>,
    on_restore_defaults: impl Fn(&gtk::Window) + 'static,
) {
    let saved = (
        settings.get_int(keys::PREFS_WINDOW_WIDTH),
        settings.get_int(keys::PREFS_WINDOW_HEIGHT),
    );

    let is_advanced = settings.get_bool(keys::ADVANCED_MODE);
    let adv_widgets: AdvancedWidgets = Rc::new(RefCell::new(Vec::new()));
    let simple_widgets: AdvancedWidgets = Rc::new(RefCell::new(Vec::new()));

    let pages = [
        create_video_page(
            settings,
            mpv.as_ref(),
            is_advanced,
            &adv_widgets,
            &simple_widgets,
        ),
        create_audio_page(
            settings,
            audio_preference_actions(mpv.as_ref()),
            is_advanced,
            &adv_widgets,
            None,
        ),
        create_subtitle_page(settings, mpv.as_ref(), is_advanced, &adv_widgets),
        create_advanced_page(settings, mpv.as_ref(), is_advanced, &adv_widgets),
    ];

    let s = settings.clone();
    let win = big_player_ui::appearance::build_preferences_window(
        parent,
        &i18n("Preferences"),
        true,
        saved,
        (600, 700),
        &pages,
        move |w, h| {
            let _ = s.put_int(keys::PREFS_WINDOW_WIDTH, w);
            let _ = s.put_int(keys::PREFS_WINDOW_HEIGHT, h);
        },
        || {},
    );

    // Advanced switch shared across all pages → goes in header bar, not per-page.
    if let Some(hb) = find_header_bar(win.upcast_ref::<gtk::Widget>()) {
        let switch_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();

        let label = gtk::Label::builder()
            .label(i18n("Advanced"))
            .css_classes(["dim-label"])
            .build();

        let switch = gtk::Switch::builder()
            .active(is_advanced)
            .valign(gtk::Align::Center)
            .build();
        crate::shared::tooltip::set(
            &switch,
            &i18n("Show all technical options across every page"),
        );

        switch_box.append(&label);
        switch_box.append(&switch);
        hb.pack_end(&switch_box);

        // "Restore Defaults" lives here now (was a loose top-menu entry). Keep
        // the confirmation parented to this preferences window so it is always
        // visible above the modal preferences surface.
        let restore_defaults_button = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .css_classes(["flat"])
            .build();
        crate::shared::tooltip::set(&restore_defaults_button, &i18n("Restore Defaults"));
        restore_defaults_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Restore Defaults"))]);
        {
            let preferences_window = win.downgrade();
            restore_defaults_button.connect_clicked(move |_| {
                if let Some(preferences_window) = preferences_window.upgrade() {
                    on_restore_defaults(preferences_window.upcast_ref::<gtk::Window>());
                }
            });
        }
        hb.pack_start(&restore_defaults_button);

        let settings = settings.clone();
        let advanced_widgets = adv_widgets.clone();
        let simple_widgets = simple_widgets.clone();
        switch.connect_active_notify(move |toggle| {
            let active = toggle.is_active();
            let _ = settings.put_bool(keys::ADVANCED_MODE, active);
            for widget in advanced_widgets.borrow().iter() {
                widget.set_visible(active);
            }
            for widget in simple_widgets.borrow().iter() {
                widget.set_visible(!active);
            }
        });
    }

    win.present();
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

mod advanced;
mod subtitles;
mod subtitles_groups;
mod video;

use advanced::create_advanced_page;
use subtitles::create_subtitle_page;
use video::create_video_page;
