//! Audio appearance dialog.

use adw::prelude::*;
use std::rc::Rc;

use crate::i18n::i18n;
use crate::settings::{Settings, keys};

use big_media_runtime::i18n::gettext_noop;
use big_player_ui::appearance::{create_audio_colors_page, show_appearance_window};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

#[derive(Clone, Copy)]
struct AudioAppearanceWindowSpec {
    default_width: i32,
    default_height: i32,
    saved_width_key: keys::IntKey,
    saved_height_key: keys::IntKey,
}

#[derive(Clone, Copy)]
struct AudioEffectsPageSpec {
    title: &'static str,
    icon_name: &'static str,
    group_title: &'static str,
    album_art_background_title: &'static str,
    album_art_background_subtitle: &'static str,
    album_art_background_key: keys::BoolKey,
    album_art_background_default: bool,
}

fn audio_appearance_window_spec() -> AudioAppearanceWindowSpec {
    AudioAppearanceWindowSpec {
        default_width: 500,
        default_height: 550,
        saved_width_key: keys::AUDIO_APPEARANCE_WINDOW_WIDTH,
        saved_height_key: keys::AUDIO_APPEARANCE_WINDOW_HEIGHT,
    }
}

fn audio_effects_page_spec() -> AudioEffectsPageSpec {
    AudioEffectsPageSpec {
        title: gettext_noop("Effects"),
        icon_name: "applications-graphics-symbolic",
        group_title: gettext_noop("Playback Bar"),
        album_art_background_title: gettext_noop("Album Art Background"),
        album_art_background_subtitle: gettext_noop(
            "Show the current cover as a blurred background behind the audio controls",
        ),
        album_art_background_key: keys::AUDIO_ALBUM_ART_BG,
        album_art_background_default: true,
    }
}

pub fn show_audio_appearance_dialog(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_changed: impl Fn() + 'static,
) {
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);
    let window_spec = audio_appearance_window_spec();

    let saved = (
        settings.get_int(window_spec.saved_width_key),
        settings.get_int(window_spec.saved_height_key),
    );

    let pages = [
        create_audio_colors_page(settings, &on_changed),
        create_effects_page(settings, &on_changed),
    ];

    let s = settings.clone();
    show_appearance_window(
        parent,
        saved,
        (window_spec.default_width, window_spec.default_height),
        &pages,
        move |w, h| {
            let _ = s.put_int(window_spec.saved_width_key, w);
            let _ = s.put_int(window_spec.saved_height_key, h);
        },
        || {},
    );
}

/// "Effects" page: a single toggle for the blurred album-art background behind
/// the audio controls. Persisting the key + firing `on_changed` lets the shell
/// re-evaluate the background live.
fn create_effects_page(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesPage {
    let page_spec = audio_effects_page_spec();
    let page = adw::PreferencesPage::builder()
        .title(i18n(page_spec.title))
        .icon_name(page_spec.icon_name)
        .build();

    let group = BigPreferencesGroupSpec::new(i18n(page_spec.group_title)).build();

    let art_bg_row = adw::SwitchRow::builder()
        .title(i18n(page_spec.album_art_background_title))
        .subtitle(i18n(page_spec.album_art_background_subtitle))
        .active(settings.get_bool_or(
            page_spec.album_art_background_key,
            page_spec.album_art_background_default,
        ))
        .build();
    {
        let s = settings.clone();
        let on_changed = on_changed.clone();
        art_bg_row.connect_active_notify(move |row| {
            let _ = s.put_bool(page_spec.album_art_background_key, row.is_active());
            on_changed();
        });
    }
    group.add(&art_bg_row);
    page.add(&group);

    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_window_geometry_and_keys() {
        let spec = audio_appearance_window_spec();

        assert_eq!(spec.default_width, 500);
        assert_eq!(spec.default_height, 550);
        assert_eq!(
            spec.saved_width_key.0,
            keys::AUDIO_APPEARANCE_WINDOW_WIDTH.0
        );
        assert_eq!(
            spec.saved_height_key.0,
            keys::AUDIO_APPEARANCE_WINDOW_HEIGHT.0
        );
    }

    #[test]
    fn effects_page_names_art_background_toggle() {
        let spec = audio_effects_page_spec();

        assert_eq!(spec.title, "Effects");
        assert_eq!(spec.icon_name, "applications-graphics-symbolic");
        assert_eq!(spec.group_title, "Playback Bar");
        assert_eq!(spec.album_art_background_title, "Album Art Background");
        assert_eq!(
            spec.album_art_background_subtitle,
            "Show the current cover as a blurred background behind the audio controls"
        );
        assert_eq!(spec.album_art_background_key.0, keys::AUDIO_ALBUM_ART_BG.0);
        assert!(spec.album_art_background_default);
    }
}
