// SPDX-License-Identifier: GPL-3.0-or-later
//! Button-layout editor page on the shared
//! `big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor`.
//! The video player owns only the SPEC (its 14 controls + the
//! top/bottom bars) and the SETTINGS mapping; the editor widget is shared.
//!
//! The runtime applier ([`super::super::super::video_controls_bar::zones`]) reads
//! the same `ui-zone-*` / `ui-show-*` keys.

use std::rc::Rc;

use adw::prelude::*;

use big_relm4_components::input::zone_layout::{
    BarDecoration, BarSpec, ZoneControl, ZoneLayout, ZoneLayoutSpec, ZonePlacement, slot_key,
};
use big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::i18n::{gettext_noop, i18n};
use crate::settings::{Settings, keys};

/// One control the Buttons page places, and where it goes when no
/// `ui-zone-*` setting names it.
pub(crate) struct OsdControl {
    pub(crate) id: &'static str,
    /// Its name in the editor, as a msgid.
    pub(crate) label: &'static str,
    pub(crate) icon: &'static str,
    pub(crate) bar: &'static str,
    pub(crate) placement: ZonePlacement,
    pub(crate) show: keys::BoolKey,
}

/// The 14 video controls, in the order of the `ui-zone-*` defaults in
/// `big_media_data::settings::keys`.
pub(crate) const CONTROLS: &[OsdControl] = &[
    OsdControl {
        id: "pin",
        label: gettext_noop("Keep Above"),
        icon: "view-pin-symbolic",
        bar: "top",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_PIN,
    },
    OsdControl {
        id: "open",
        label: gettext_noop("Open"),
        icon: "document-open-symbolic",
        bar: "top",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_OPEN,
    },
    OsdControl {
        id: "playlist",
        label: gettext_noop("Files and Playlist"),
        icon: "view-list-symbolic",
        bar: "top",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_PLAYLIST,
    },
    OsdControl {
        id: "menu",
        label: gettext_noop("Settings"),
        icon: "open-menu-symbolic",
        bar: "top",
        placement: ZonePlacement::End,
        show: keys::UI_SHOW_MENU,
    },
    OsdControl {
        id: "volume",
        label: gettext_noop("Volume"),
        icon: "audio-volume-high-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_VOLUME,
    },
    OsdControl {
        id: "speed",
        label: gettext_noop("Speed"),
        icon: "power-profile-performance-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_SPEED,
    },
    OsdControl {
        id: "voice",
        label: gettext_noop("Voice Enhancement"),
        icon: "audio-input-microphone-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Start,
        show: keys::UI_SHOW_VOICE,
    },
    OsdControl {
        id: "prev",
        label: gettext_noop("Previous"),
        icon: "media-skip-backward-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Center,
        show: keys::UI_SHOW_PREV,
    },
    OsdControl {
        id: "play",
        label: gettext_noop("Play/Pause"),
        icon: "media-playback-start-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Center,
        show: keys::UI_SHOW_PLAY,
    },
    OsdControl {
        id: "next",
        label: gettext_noop("Next"),
        icon: "media-skip-forward-symbolic",
        bar: "bottom",
        placement: ZonePlacement::Center,
        show: keys::UI_SHOW_NEXT,
    },
    OsdControl {
        id: "subtitle",
        label: gettext_noop("Subtitles"),
        icon: "media-view-subtitles-symbolic",
        bar: "bottom",
        placement: ZonePlacement::End,
        show: keys::UI_SHOW_SUBTITLE,
    },
    OsdControl {
        id: "audio-track",
        label: gettext_noop("Audio Track"),
        icon: "audio-x-generic-symbolic",
        bar: "bottom",
        placement: ZonePlacement::End,
        show: keys::UI_SHOW_AUDIO_TRACK,
    },
    OsdControl {
        id: "tv",
        label: gettext_noop("Play on a TV"),
        icon: "tv-symbolic",
        bar: "bottom",
        placement: ZonePlacement::End,
        show: keys::UI_SHOW_TV,
    },
    OsdControl {
        id: "fullscreen",
        label: gettext_noop("Fullscreen"),
        icon: "view-fullscreen-symbolic",
        bar: "bottom",
        placement: ZonePlacement::End,
        show: keys::UI_SHOW_FULLSCREEN,
    },
];

/// `(bar, placement, the `ui-zone-*` key)`: maps the editor's slots to the
/// settings keys the runtime controls bar reads.
pub(crate) const ZONE_KEYS: &[(&str, ZonePlacement, keys::StringKey)] = &[
    ("top", ZonePlacement::Start, keys::UI_ZONE_TOP_LEFT),
    ("top", ZonePlacement::End, keys::UI_ZONE_TOP_RIGHT),
    ("bottom", ZonePlacement::Start, keys::UI_ZONE_BOTTOM_LEFT),
    ("bottom", ZonePlacement::Center, keys::UI_ZONE_BOTTOM_CENTER),
    ("bottom", ZonePlacement::End, keys::UI_ZONE_BOTTOM_RIGHT),
];

/// Build the editor spec: the 14 controls + a top bar drawn as the window
/// header (its zones around the title, window controls at the end) and a
/// bottom bar (transport, with a seek slider above it).
fn build_spec() -> ZoneLayoutSpec {
    let controls = CONTROLS
        .iter()
        .map(|control| {
            ZoneControl::new(
                control.id,
                i18n(control.label),
                control.icon,
                slot_key(control.bar, control.placement),
            )
        })
        .collect();
    let bars = vec![
        BarSpec::new("top", vec![ZonePlacement::Start, ZonePlacement::End])
            .with_label(i18n("Top bar"))
            .with_decoration(BarDecoration {
                title: Some(i18n("Video Title")),
                seek_slider: false,
                window_controls: true,
            }),
        BarSpec::new("bottom", ZonePlacement::ALL.to_vec())
            .with_label(i18n("Bottom bar"))
            .with_decoration(BarDecoration {
                title: None,
                seek_slider: true,
                window_controls: false,
            }),
    ];
    ZoneLayoutSpec::new(controls, bars)
}

/// Read the current layout from settings (typed `get_string`/`get_bool`).
fn read_layout(settings: &Settings, spec: &ZoneLayoutSpec) -> ZoneLayout {
    let mut layout = ZoneLayout::from_csv_zones(spec, |bar, placement| {
        ZONE_KEYS
            .iter()
            .find(|(b, p, _)| *b == bar && *p == placement)
            .map_or_else(String::new, |(_, _, key)| settings.get_string(*key))
    });
    for control in CONTROLS {
        layout.set_visible(control.id, settings.get_bool(control.show));
    }
    layout
}

/// Persist a layout change to settings (`ui-zone-*` CSV + `ui-show-*` flags).
fn write_layout(settings: &Settings, layout: &ZoneLayout) {
    for (bar, placement, key) in ZONE_KEYS {
        let _ = settings.set_string(key.0, &layout.zone_csv(bar, *placement));
    }
    for control in CONTROLS {
        let _ = settings.set_boolean(control.show.0, layout.is_visible(control.id));
    }
}

/// Build the "Buttons" appearance page hosting the shared zone-layout editor.
pub(in crate::video) fn create_buttons_page(
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Buttons"))
        .icon_name("view-more-symbolic")
        .build();

    let group = BigPreferencesGroupSpec::new(i18n("Button Layout"))
        .description(i18n(
            "Drag buttons between zones, or move the focused one with Alt+Left and Alt+Right. Click a button to show or hide it.",
        ))
        .build();

    let spec = build_spec();
    let editor = BigZoneLayoutEditor::new(&spec, &read_layout(settings, &spec));
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        editor.connect_changed(move |layout| {
            write_layout(&settings, layout);
            on_changed();
        });
    }
    group.add(editor.root());
    page.add(&group);
    // The editor struct can drop here: its widgets live in the page and the
    // gesture closures keep its state alive (they hold the change sink), so
    // edits keep persisting after this function returns.
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_places_match_the_zone_defaults() {
        let settings = Settings::memory_only();
        for (bar, placement, key) in ZONE_KEYS {
            let expected: Vec<&str> = CONTROLS
                .iter()
                .filter(|control| control.bar == *bar && control.placement == *placement)
                .map(|control| control.id)
                .collect();
            let saved = settings.get_string(*key);
            assert_eq!(saved.split(',').collect::<Vec<_>>(), expected, "{}", key.0);
        }
    }
}
