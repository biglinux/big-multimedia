//! The header buttons the Appearance > Buttons page places: which show, and on
//! which side in which order, from the `ui-zone-top-*` / `ui-show-*` settings.

use std::collections::HashMap;

use adw::prelude::*;
use big_relm4_components::input::zone_layout::{ZoneLayout, ZonePlacement, slot_key};
use big_relm4_components::layout::zone_applier::apply_zone_layout;

use crate::settings::{Settings, keys};
use crate::video::ui::video_controls_bar::read_zones;

/// `(id, its ui-show-* key, the zone it takes when no CSV names it)`.
/// `playlist` is the Files/Playlist switch.
const HEADER_CONTROLS: [(&str, keys::BoolKey, ZonePlacement); 4] = [
    ("pin", keys::UI_SHOW_PIN, ZonePlacement::Start),
    ("open", keys::UI_SHOW_OPEN, ZonePlacement::Start),
    ("playlist", keys::UI_SHOW_PLAYLIST, ZonePlacement::Start),
    ("menu", keys::UI_SHOW_MENU, ZonePlacement::End),
];

/// The header's start and end zones and the buttons placed in them. The
/// window controls, the title and the status pills stay outside the zones.
#[derive(Clone)]
pub(super) struct HeaderButtons {
    pub(super) start_zone: gtk::Box,
    pub(super) end_zone: gtk::Box,
    controls: HashMap<String, gtk::Widget>,
}

impl HeaderButtons {
    pub(super) fn new(
        pin: &impl IsA<gtk::Widget>,
        open: &impl IsA<gtk::Widget>,
        sidebar_switch: &impl IsA<gtk::Widget>,
        menu: &impl IsA<gtk::Widget>,
    ) -> Self {
        // The header bar's own spacing, so a zone reads as its siblings do.
        let zone = || gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let controls = [
            ("pin", pin.as_ref()),
            ("open", open.as_ref()),
            ("playlist", sidebar_switch.as_ref()),
            ("menu", menu.as_ref()),
        ]
        .into_iter()
        .map(|(id, widget)| (id.to_owned(), widget.clone()))
        .collect();
        Self {
            start_zone: zone(),
            end_zone: zone(),
            controls,
        }
    }

    /// Place and show the buttons as the settings say; a hidden button's
    /// action and accelerator stay.
    pub(super) fn apply_settings(&self, settings: &Settings) {
        let start = slot_key("top", ZonePlacement::Start);
        let end = slot_key("top", ZonePlacement::End);
        let fallback_slots =
            HEADER_CONTROLS.map(|(id, _, placement)| (id, slot_key("top", placement)));
        let fallback = fallback_slots
            .each_ref()
            .map(|(id, slot)| (*id, slot.as_str()));
        apply_zone_layout(
            &read_header_layout(settings),
            &self.controls,
            &[
                (start.as_str(), &self.start_zone),
                (end.as_str(), &self.end_zone),
            ],
            &fallback,
            &HashMap::new(),
        );
    }
}

/// The header's layout: its two zones and the buttons hidden.
fn read_header_layout(settings: &Settings) -> ZoneLayout {
    let mut layout = read_zones(
        settings,
        &[
            (
                keys::UI_ZONE_TOP_LEFT,
                &slot_key("top", ZonePlacement::Start),
            ),
            (
                keys::UI_ZONE_TOP_RIGHT,
                &slot_key("top", ZonePlacement::End),
            ),
        ],
        &HEADER_CONTROLS.map(|(id, _, _)| id),
    );
    for (id, show, _) in HEADER_CONTROLS {
        layout.set_visible(id, settings.get_bool(show));
    }
    layout
}
