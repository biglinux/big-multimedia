//! Audio appearance dialog.

use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

use crate::i18n::i18n;
use crate::settings::{keys, Settings};

use big_player_ui::appearance::create_audio_colors_page;

#[allow(deprecated)]
pub fn show_audio_appearance_dialog(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_changed: impl Fn() + 'static,
) {
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);

    let saved_w = settings.get_int(keys::AUDIO_APPEARANCE_WINDOW_WIDTH);
    let saved_h = settings.get_int(keys::AUDIO_APPEARANCE_WINDOW_HEIGHT);
    let width = if saved_w > 0 { saved_w } else { 500 };
    let height = if saved_h > 0 { saved_h } else { 550 };

    let win = adw::PreferencesWindow::builder()
        .title(i18n("Appearance"))
        .search_enabled(false)
        .default_width(width)
        .default_height(height)
        .transient_for(parent.as_ref())
        .modal(true)
        .build();

    win.add(&create_audio_colors_page(settings, &on_changed));
    win.add(&create_effects_page(settings, &on_changed));

    let s = settings.clone();
    win.connect_close_request(move |w| {
        let _ = s.put_int(keys::AUDIO_APPEARANCE_WINDOW_WIDTH, w.width());
        let _ = s.put_int(keys::AUDIO_APPEARANCE_WINDOW_HEIGHT, w.height());
        glib::Propagation::Proceed
    });

    win.present();
}

/// "Effects" page: a single toggle for the blurred album-art background behind
/// the audio controls. Persisting the key + firing `on_changed` lets the shell
/// re-evaluate the background live.
fn create_effects_page(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Effects"))
        .icon_name("applications-graphics-symbolic")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(i18n("Playback Bar"))
        .build();

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
