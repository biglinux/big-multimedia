//! Audio appearance dialog.

use adw::prelude::*;
use std::rc::Rc;

use crate::i18n::i18n;
use crate::settings::{keys, Settings};

use big_player_ui::appearance::{create_audio_colors_page, show_appearance_window};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

pub fn show_audio_appearance_dialog(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_changed: impl Fn() + 'static,
) {
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);

    let saved = (
        settings.get_int(keys::AUDIO_APPEARANCE_WINDOW_WIDTH),
        settings.get_int(keys::AUDIO_APPEARANCE_WINDOW_HEIGHT),
    );

    let pages = [
        create_audio_colors_page(settings, &on_changed),
        create_effects_page(settings, &on_changed),
    ];

    let s = settings.clone();
    show_appearance_window(
        parent,
        saved,
        (500, 550),
        &pages,
        move |w, h| {
            let _ = s.put_int(keys::AUDIO_APPEARANCE_WINDOW_WIDTH, w);
            let _ = s.put_int(keys::AUDIO_APPEARANCE_WINDOW_HEIGHT, h);
        },
        || {},
    );
}

/// "Effects" page: a single toggle for the blurred album-art background behind
/// the audio controls. Persisting the key + firing `on_changed` lets the shell
/// re-evaluate the background live.
fn create_effects_page(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Effects"))
        .icon_name("applications-graphics-symbolic")
        .build();

    let group = BigPreferencesGroupSpec::new(i18n("Playback Bar")).build();

    let art_bg_row = adw::SwitchRow::builder()
        .title(i18n("Album Art Background"))
        .subtitle(i18n(
            "Show the current cover as a blurred background behind the audio controls",
        ))
        .active(settings.get_bool_or(keys::AUDIO_ALBUM_ART_BG, true))
        .build();
    {
        let s = settings.clone();
        let on_changed = on_changed.clone();
        art_bg_row.connect_active_notify(move |row| {
            let _ = s.put_bool(keys::AUDIO_ALBUM_ART_BG, row.is_active());
            on_changed();
        });
    }
    group.add(&art_bg_row);
    page.add(&group);

    page
}
