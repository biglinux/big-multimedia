//! The header buttons the Appearance > Buttons page places: which show, and on
//! which side in which order, from the `ui-zone-top-*` / `ui-show-*` settings.

use std::collections::HashMap;

use adw::prelude::*;
use big_relm4_components::input::zone_layout::{ZoneLayout, ZonePlacement, slot_key};
use big_relm4_components::layout::zone_applier::apply_zone_layout;

use crate::settings::{Settings, keys};
use crate::video::ui::appearance::layout::{CONTROLS, OsdControl};
use crate::video::ui::video_controls_bar::read_zones;

/// The Buttons page's top-bar controls; `playlist` is the Files/Playlist switch.
fn header_controls() -> impl Iterator<Item = &'static OsdControl> {
    CONTROLS.iter().filter(|control| control.bar == "top")
}

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
        let fallback_slots: Vec<(&str, String)> = header_controls()
            .map(|control| (control.id, slot_key(control.bar, control.placement)))
            .collect();
        let fallback: Vec<(&str, &str)> = fallback_slots
            .iter()
            .map(|(id, slot)| (*id, slot.as_str()))
            .collect();
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
        &header_controls()
            .map(|control| control.id)
            .collect::<Vec<_>>(),
    );
    for control in header_controls() {
        layout.set_visible(control.id, settings.get_bool(control.show));
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_zones_follow_the_top_keys() {
        let settings = Settings::memory_only();
        settings.set_string(keys::UI_ZONE_TOP_LEFT.0, "menu, pin,volume,ghost");
        settings.set_string(keys::UI_ZONE_TOP_RIGHT.0, "playlist,,open");

        let layout = read_header_layout(&settings);

        assert_eq!(layout.zone("top", ZonePlacement::Start), ["menu", "pin"]);
        assert_eq!(layout.zone("top", ZonePlacement::End), ["playlist", "open"]);
    }

    #[test]
    fn header_hides_the_buttons_turned_off() {
        let settings = Settings::memory_only();
        settings.set_boolean(keys::UI_SHOW_OPEN.0, false);
        settings.set_boolean(keys::UI_SHOW_MENU.0, false);

        let layout = read_header_layout(&settings);

        assert!(layout.is_visible("pin"));
        assert!(!layout.is_visible("open"));
        assert!(layout.is_visible("playlist"));
        assert!(!layout.is_visible("menu"));
    }

    /// The header before it read these keys: Keep Above, Open and the
    /// Files/Playlist switch on the left, Settings on the right.
    #[test]
    fn default_header_keeps_its_places() {
        let layout = read_header_layout(&Settings::memory_only());

        assert_eq!(
            layout.zone("top", ZonePlacement::Start),
            ["pin", "open", "playlist"]
        );
        assert_eq!(layout.zone("top", ZonePlacement::End), ["menu"]);
        assert!(layout.hidden.is_empty());
    }

    fn children(zone: &gtk::Box) -> Vec<gtk::Widget> {
        std::iter::successors(zone.first_child(), gtk::Widget::next_sibling).collect()
    }

    fn widgets(buttons: &[&gtk::Button]) -> Vec<gtk::Widget> {
        buttons
            .iter()
            .map(|button| button.upcast_ref::<gtk::Widget>().clone())
            .collect()
    }

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn apply_places_the_header_buttons_live() {
        gtk::init().unwrap();
        let [pin, open, switch, menu] = [(); 4].map(|()| gtk::Button::new());
        let header = HeaderButtons::new(&pin, &open, &switch, &menu);
        let settings = Settings::memory_only();
        settings.set_string(keys::UI_ZONE_TOP_LEFT.0, "menu,pin,menu,ghost");
        settings.set_string(keys::UI_ZONE_TOP_RIGHT.0, "playlist,pin");
        settings.set_boolean(keys::UI_SHOW_OPEN.0, false);

        header.apply_settings(&settings);

        // A repeat keeps its first place; `open`, named nowhere, goes to its
        // default side.
        assert_eq!(children(&header.start_zone), widgets(&[&menu, &pin, &open]));
        assert_eq!(children(&header.end_zone), widgets(&[&switch]));
        assert!(!open.is_visible());
        assert!(menu.is_visible());

        settings.reset_to_defaults();
        header.apply_settings(&settings);

        assert_eq!(
            children(&header.start_zone),
            widgets(&[&pin, &open, &switch])
        );
        assert_eq!(children(&header.end_zone), widgets(&[&menu]));
        assert!(open.is_visible());
    }
}
