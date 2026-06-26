// SPDX-License-Identifier: GPL-3.0-or-later
//! Button-layout editor page (RESTRUCTURE D3-T3): adopts the shared
//! `big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor` instead
//! of the bespoke drag-and-drop miniature that used to live here (+ `miniature.rs`,
//! both deleted). The video player owns only the SPEC (its 13 controls + the
//! top/bottom bars) and the SETTINGS mapping; the editor widget is shared.
//!
//! The runtime applier ([`super::super::super::video_controls_bar::zones`]) reads
//! the same `ui-zone-*` / `ui-show-*` keys — unchanged.

use std::rc::Rc;

use adw::prelude::*;

use big_relm4_components::input::zone_layout::{
    BarDecoration, BarSpec, ZoneControl, ZoneLayout, ZoneLayoutSpec, ZonePlacement,
};
use big_relm4_components::input::zone_layout_editor::BigZoneLayoutEditor;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::i18n::i18n;
use crate::settings::{keys, Settings};

/// The 13 video controls: `(id, label-msgid, icon, default "<bar>:<placement>"
/// slot, the `ui-show-*` key)`. Defaults mirror `big_media_data::settings::defaults`.
const CONTROLS: &[(&str, &str, &str, &str, keys::BoolKey)] = &[
    (
        "pin",
        "Keep Above",
        "view-pin-symbolic",
        "top:start",
        keys::UI_SHOW_PIN,
    ),
    (
        "open",
        "Open File",
        "folder-open-symbolic",
        "top:start",
        keys::UI_SHOW_OPEN,
    ),
    (
        "playlist",
        "Playlist",
        "view-list-symbolic",
        "top:end",
        keys::UI_SHOW_PLAYLIST,
    ),
    (
        "menu",
        "Menu",
        "open-menu-symbolic",
        "top:end",
        keys::UI_SHOW_MENU,
    ),
    (
        "volume",
        "Volume",
        "audio-volume-high-symbolic",
        "bottom:start",
        keys::UI_SHOW_VOLUME,
    ),
    (
        "speed",
        "Speed",
        "speedometer-symbolic",
        "bottom:start",
        keys::UI_SHOW_SPEED,
    ),
    (
        "voice",
        "Voice Enhancement",
        "audio-input-microphone-symbolic",
        "bottom:start",
        keys::UI_SHOW_VOICE,
    ),
    (
        "prev",
        "Previous",
        "media-skip-backward-symbolic",
        "bottom:center",
        keys::UI_SHOW_PREV,
    ),
    (
        "play",
        "Play/Pause",
        "media-playback-start-symbolic",
        "bottom:center",
        keys::UI_SHOW_PLAY,
    ),
    (
        "next",
        "Next",
        "media-skip-forward-symbolic",
        "bottom:center",
        keys::UI_SHOW_NEXT,
    ),
    (
        "subtitle",
        "Subtitles",
        "media-view-subtitles-symbolic",
        "bottom:end",
        keys::UI_SHOW_SUBTITLE,
    ),
    (
        "audio-track",
        "Audio Track",
        "audio-x-generic-symbolic",
        "bottom:end",
        keys::UI_SHOW_AUDIO_TRACK,
    ),
    (
        "fullscreen",
        "Fullscreen",
        "view-fullscreen-symbolic",
        "bottom:end",
        keys::UI_SHOW_FULLSCREEN,
    ),
];

/// `(bar, placement, the `ui-zone-*` key)` — maps the editor's slots to the
/// settings keys the runtime controls bar reads.
const ZONE_KEYS: &[(&str, ZonePlacement, keys::StringKey)] = &[
    ("top", ZonePlacement::Start, keys::UI_ZONE_TOP_LEFT),
    ("top", ZonePlacement::End, keys::UI_ZONE_TOP_RIGHT),
    ("bottom", ZonePlacement::Start, keys::UI_ZONE_BOTTOM_LEFT),
    ("bottom", ZonePlacement::Center, keys::UI_ZONE_BOTTOM_CENTER),
    ("bottom", ZonePlacement::End, keys::UI_ZONE_BOTTOM_RIGHT),
];

/// Build the editor spec: the 13 controls + a top bar (pin/open · title · menu/
/// playlist) and a bottom bar (transport, with a seek slider above it).
fn build_spec() -> ZoneLayoutSpec {
    let controls = CONTROLS
        .iter()
        .map(|(id, label, icon, slot, _)| ZoneControl::new(*id, i18n(label), *icon, *slot))
        .collect();
    let bars = vec![
        BarSpec::new("top", vec![ZonePlacement::Start, ZonePlacement::End]).with_decoration(
            BarDecoration {
                title: Some(i18n("Video Title")),
                seek_slider: false,
            },
        ),
        BarSpec::new("bottom", ZonePlacement::ALL.to_vec()).with_decoration(BarDecoration {
            title: None,
            seek_slider: true,
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
    for (id, _, _, _, show) in CONTROLS {
        layout.set_visible(id, settings.get_bool(*show));
    }
    layout
}

/// Persist a layout change to settings (`ui-zone-*` CSV + `ui-show-*` flags).
fn write_layout(settings: &Settings, layout: &ZoneLayout) {
    for (bar, placement, key) in ZONE_KEYS {
        let _ = settings.set_string(key.0, &layout.zone_csv(bar, *placement));
    }
    for (id, _, _, _, show) in CONTROLS {
        let _ = settings.set_boolean(show.0, layout.is_visible(id));
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
            "Drag buttons between zones • Click to toggle visibility",
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
