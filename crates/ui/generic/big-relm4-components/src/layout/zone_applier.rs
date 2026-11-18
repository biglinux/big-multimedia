// SPDX-License-Identifier: MIT

//! Runtime applier for the display-free
//! [`ZoneLayout`](crate::input::zone_layout::ZoneLayout) model: reparent live
//! controls into their zone containers and apply per-control visibility.
//!
//! This is the leak-safe, generic version of the video-player controls-bar
//! `ButtonLayoutWidgets::apply_settings` code (the "button-zone live reparent"
//! residual). It pairs with the GTK-free
//! [`ZoneLayout`](big_app_kit_core::zone_layout::ZoneLayout) value and the visual
//! [`BigZoneLayoutEditor`](crate::input::zone_layout_editor::BigZoneLayoutEditor):
//! the editor produces a layout, this applier renders it onto live widgets.
//!
//! # Leak safety
//!
//! Reparenting a GTK widget that already has a parent panics unless it is
//! unparented first, and a naive `remove`/`append` on every apply churns the
//! widget graph. A widget that is **already** a child of the target container
//! stays there and is only reordered to follow the layout; otherwise it is
//! `unparent()`ed from its old parent before it is inserted. A control placed
//! by its fallback default keeps its place in the zone. No dangling
//! references, no redundant reparents.

use std::collections::{BTreeSet, HashMap};

use gtk::prelude::*;

use big_app_kit_core::zone_layout::ZoneLayout;

/// Apply `layout` onto live control widgets.
///
/// - `controls`: the control-id → widget registry. A control absent here is
///   skipped (it neither reparents nor blocks a slot), mirroring the video
///   player's `control_widget` returning `None`.
/// - `slots`: the ordered `(slot_key, container)` list. The order drives
///   cross-slot first-wins dedup: a control named in two slots lands in the
///   first slot (in this order) that names it.
/// - `fallback_defaults`: ordered `(control_id, slot_key)` defaults. Any
///   registered control the layout leaves unplaced is appended to the **end** of
///   its default slot, in this order, unless it is already in that slot, where
///   it keeps its place.
/// - `visibility`: per-control visibility overrides for controls whose
///   visibility depends on extra runtime conditions (e.g. a backend probe). A
///   control absent from this map falls back to [`ZoneLayout::is_visible`] (its
///   hidden-set membership).
pub fn apply_zone_layout(
    layout: &ZoneLayout,
    controls: &HashMap<String, gtk::Widget>,
    slots: &[(&str, &gtk::Box)],
    fallback_defaults: &[(&str, &str)],
    visibility: &HashMap<String, bool>,
) {
    let slot_order: Vec<&str> = slots.iter().map(|(key, _)| *key).collect();
    let placements = resolve_placements(layout, &slot_order, fallback_defaults, |id| {
        controls.contains_key(id)
    });

    // `placements` is built in `slot_order`, i.e. the same order as `slots`, so
    // a positional zip pairs each resolved slot with its container.
    for ((slot_key, ids), &(_, target)) in placements.iter().zip(slots.iter()) {
        let named = layout.slots.get(slot_key);
        let mut previous: Option<&gtk::Widget> = None;
        for id in ids {
            let Some(widget) = controls.get(id) else {
                continue;
            };
            if named.is_some_and(|named| named.contains(id)) {
                reparent_after(widget, target, previous);
                previous = Some(widget);
            } else if widget.parent().as_ref() != Some(target.upcast_ref()) {
                widget.unparent();
                target.append(widget);
            }
        }
    }

    for (id, widget) in controls {
        widget.set_visible(resolve_visibility(layout, id, visibility));
    }
}

/// Resolve the final control ordering per slot (pure, no GTK).
///
/// Walks `slot_order`, taking each slot's ids from `layout`; a control is placed
/// in the FIRST slot (in `slot_order`) that names it (`is_registered` and not
/// already placed). After the layout walk, `fallback_defaults` appends any
/// registered, still-unplaced control to the end of its slot. Returns
/// `(slot_key, ordered_ids)` in `slot_order`.
fn resolve_placements(
    layout: &ZoneLayout,
    slot_order: &[&str],
    fallback_defaults: &[(&str, &str)],
    is_registered: impl Fn(&str) -> bool,
) -> Vec<(String, Vec<String>)> {
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let mut result: Vec<(String, Vec<String>)> = slot_order
        .iter()
        .map(|slot_key| ((*slot_key).to_owned(), Vec::new()))
        .collect();

    for (index, &slot_key) in slot_order.iter().enumerate() {
        if let Some(named) = layout.slots.get(slot_key) {
            for id in named {
                if is_registered(id.as_str()) && placed.insert(id.clone()) {
                    result[index].1.push(id.clone());
                }
            }
        }
    }

    for &(id, slot_key) in fallback_defaults {
        if is_registered(id)
            && !placed.contains(id)
            && let Some(index) = slot_order.iter().position(|key| *key == slot_key)
        {
            placed.insert(id.to_owned());
            result[index].1.push(id.to_owned());
        }
    }

    result
}

/// Resolve a control's visibility: an explicit `overrides` entry wins, else the
/// layout's hidden-set decides (visible when not hidden). Pure.
fn resolve_visibility(layout: &ZoneLayout, id: &str, overrides: &HashMap<String, bool>) -> bool {
    overrides
        .get(id)
        .copied()
        .unwrap_or_else(|| layout.is_visible(id))
}

/// Leak-safe reparent: put `widget` in `target` right after `previous`, or
/// first when `None`. Already in `target`, it moves only to follow
/// `previous`, so the first control a zone names keeps its place.
fn reparent_after(widget: &gtk::Widget, target: &gtk::Box, previous: Option<&gtk::Widget>) {
    if widget.parent().as_ref() == Some(target.upcast_ref()) {
        if previous.is_some() {
            target.reorder_child_after(widget, previous);
        }
        return;
    }
    widget.unparent();
    target.insert_child_after(widget, previous);
}

#[cfg(test)]
mod tests {
    use super::*;
    use big_app_kit_core::zone_layout::{ZonePlacement, slot_key};

    fn slots() -> [String; 3] {
        [
            slot_key("bottom", ZonePlacement::Start),
            slot_key("bottom", ZonePlacement::Center),
            slot_key("bottom", ZonePlacement::End),
        ]
    }

    /// A three-slot layout resolves layout-named controls in slot order, then
    /// appends fallback defaults for unplaced controls to the end of their slot.
    #[test]
    fn resolve_places_named_then_fallback_defaults() {
        let [start, center, end] = slots();
        let mut layout = ZoneLayout::default();
        layout.slots.insert(start.clone(), vec!["volume".into()]);
        layout.slots.insert(center.clone(), vec!["play".into()]);
        // `end` left empty: its defaults come from the fallback list.
        let order = [start.as_str(), center.as_str(), end.as_str()];
        let fallback = [
            ("volume", start.as_str()),
            ("speed", start.as_str()),
            ("play", center.as_str()),
            ("fullscreen", end.as_str()),
        ];

        let placed = resolve_placements(&layout, &order, &fallback, |_| true);

        assert_eq!(placed[0].1, ["volume", "speed"]); // named first, fallback appended
        assert_eq!(placed[1].1, ["play"]);
        assert_eq!(placed[2].1, ["fullscreen"]);
    }

    /// A control named in two slots lands in the first slot (slot order).
    #[test]
    fn resolve_dedups_control_to_first_slot() {
        let [start, center, end] = slots();
        let mut layout = ZoneLayout::default();
        layout.slots.insert(start.clone(), vec!["play".into()]);
        layout.slots.insert(center.clone(), vec!["play".into()]);
        let order = [start.as_str(), center.as_str(), end.as_str()];

        let placed = resolve_placements(&layout, &order, &[], |_| true);

        assert_eq!(placed[0].1, ["play"]);
        assert!(placed[1].1.is_empty());
    }

    /// Unregistered ids are dropped from both the layout walk and the fallback.
    #[test]
    fn resolve_drops_unregistered_ids() {
        let [start, center, end] = slots();
        let mut layout = ZoneLayout::default();
        layout
            .slots
            .insert(start.clone(), vec!["play".into(), "ghost".into()]);
        let order = [start.as_str(), center.as_str(), end.as_str()];
        let fallback = [("phantom", center.as_str())];
        let known: BTreeSet<&str> = ["play"].into_iter().collect();

        let placed = resolve_placements(&layout, &order, &fallback, |id| known.contains(id));

        assert_eq!(placed[0].1, ["play"]);
        assert!(placed[1].1.is_empty());
    }

    /// A fallback default is skipped when its control is already placed by the
    /// layout, even in a different slot.
    #[test]
    fn resolve_fallback_skips_already_placed_control() {
        let [start, center, end] = slots();
        let mut layout = ZoneLayout::default();
        layout.slots.insert(start.clone(), vec!["play".into()]);
        let order = [start.as_str(), center.as_str(), end.as_str()];
        let fallback = [("play", center.as_str())];

        let placed = resolve_placements(&layout, &order, &fallback, |_| true);

        assert_eq!(placed[0].1, ["play"]);
        assert!(placed[1].1.is_empty());
    }

    /// Explicit overrides win over the hidden-set; absent controls fall back to
    /// [`ZoneLayout::is_visible`].
    #[test]
    fn resolve_visibility_prefers_overrides_then_hidden_set() {
        let mut layout = ZoneLayout::default();
        layout.set_visible("volume", false); // hidden-set says invisible
        let overrides: HashMap<String, bool> =
            [("volume".to_owned(), true), ("voice".to_owned(), false)]
                .into_iter()
                .collect();

        assert!(resolve_visibility(&layout, "volume", &overrides)); // override wins
        assert!(!resolve_visibility(&layout, "voice", &overrides)); // override wins
        assert!(resolve_visibility(&layout, "play", &overrides)); // not hidden, no override
        assert!(!resolve_visibility(&layout, "volume", &HashMap::new())); // hidden-set decides
    }

    fn gtk_ready() -> bool {
        gtk::init().is_ok()
    }

    /// End-to-end reparent: registered controls land in their resolved slot
    /// container, in order; a leak-safe re-apply moving a control to a new slot
    /// unparents it from the old container.
    #[test]
    #[ignore = "requires a live GTK display; run under `cargo nextest run --run-ignored`"]
    fn apply_reparents_controls_into_slot_containers() {
        if !gtk_ready() {
            return;
        }
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let center = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let volume = gtk::Button::new();
        let play = gtk::Button::new();
        let fullscreen = gtk::Button::new();
        // Controls start life in `start` (their build-order home).
        start.append(&volume);
        start.append(&play);
        start.append(&fullscreen);

        let controls: HashMap<String, gtk::Widget> = [
            ("volume".to_owned(), volume.clone().upcast()),
            ("play".to_owned(), play.clone().upcast()),
            ("fullscreen".to_owned(), fullscreen.clone().upcast()),
        ]
        .into_iter()
        .collect();
        let [start_key, center_key, end_key] = slots();
        let slot_refs = [
            (start_key.as_str(), &start),
            (center_key.as_str(), &center),
            (end_key.as_str(), &end),
        ];
        let fallback = [
            ("volume", start_key.as_str()),
            ("play", center_key.as_str()),
            ("fullscreen", end_key.as_str()),
        ];

        let mut layout = ZoneLayout::default();
        layout
            .slots
            .insert(start_key.clone(), vec!["volume".into()]);
        layout.slots.insert(center_key.clone(), vec!["play".into()]);

        apply_zone_layout(&layout, &controls, &slot_refs, &fallback, &HashMap::new());

        assert_eq!(volume.parent().as_ref(), Some(start.upcast_ref()));
        assert_eq!(play.parent().as_ref(), Some(center.upcast_ref()));
        assert_eq!(fullscreen.parent().as_ref(), Some(end.upcast_ref())); // via fallback

        // Re-apply with `play` moved to `end`: leak-safe unparent from `center`.
        let mut moved = ZoneLayout::default();
        moved.slots.insert(start_key.clone(), vec!["volume".into()]);
        moved
            .slots
            .insert(end_key.clone(), vec!["play".into(), "fullscreen".into()]);
        apply_zone_layout(&moved, &controls, &slot_refs, &fallback, &HashMap::new());

        assert_eq!(play.parent().as_ref(), Some(end.upcast_ref()));
        assert!(center.first_child().is_none());

        // Re-apply with the order inside `end` swapped: the zone follows it.
        let mut swapped = moved.clone();
        swapped
            .slots
            .insert(end_key.clone(), vec!["fullscreen".into(), "play".into()]);
        apply_zone_layout(&swapped, &controls, &slot_refs, &fallback, &HashMap::new());

        assert_eq!(end.first_child().as_ref(), Some(fullscreen.upcast_ref()));
        assert_eq!(end.last_child().as_ref(), Some(play.upcast_ref()));

        // `fullscreen` left to its fallback, already in `end`, keeps its place.
        let mut fallback_only = moved.clone();
        fallback_only
            .slots
            .insert(end_key.clone(), vec!["play".into()]);
        apply_zone_layout(
            &fallback_only,
            &controls,
            &slot_refs,
            &fallback,
            &HashMap::new(),
        );

        assert_eq!(end.first_child().as_ref(), Some(fullscreen.upcast_ref()));
        assert_eq!(end.last_child().as_ref(), Some(play.upcast_ref()));
    }
}
