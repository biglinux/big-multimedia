// SPDX-License-Identifier: MIT

//! `BigZoneLayoutEditor`: the visual "drag controls between zones" editor.
//!
//! A miniature of the target chrome (one `CenterBox` row per
//! [`BarSpec`](super::zone_layout::BarSpec), with
//! start/center/end drop zones) where the user **drags** controls between zones,
//! or moves the focused one with Alt+Left/Right, and **clicks** to toggle a
//! control's visibility. It is persistence-agnostic:
//! it edits an in-memory [`ZoneLayout`] and emits it through
//! [`connect_changed`](BigZoneLayoutEditor::connect_changed) on every change;
//! the app maps that to its own store (video-player → `ui-zone-*`/`ui-show-*`
//! gsettings, big-shell → its panel config).
//!
//! Leak contract: the editor's widgets are owned solely by its `root`
//! frame; the drag/drop/click closures capture an `Rc<EditorState>` that holds
//! only weak widget refs (no widget⇄closure strong cycle), so dropping the
//! editor finalizes the whole subtree.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};

use super::zone_layout::{ZoneLayout, ZoneLayoutSpec, ZonePlacement, slot_key};
use crate::i18n::t;

/// Change sink: invoked with the new [`ZoneLayout`] on every edit.
type ChangeCallback = Box<dyn Fn(&ZoneLayout)>;

/// Self-contained CSS for the miniature (one provider per display, replaced on
/// re-injection). Dark and theme-independent: the miniature is a realistic preview
/// of chrome that is itself usually dark, regardless of the app's GTK theme.
const EDITOR_CSS: &str = "
.zle-frame { border-radius: 10px; border: 1px solid alpha(@borders, 0.5); }
.zle-layout { background: #1a1a1e; min-height: 120px; }
.zle-bar { background: rgba(42,42,48,0.9); padding: 4px 8px; min-height: 34px; }
.zle-seek { margin: 2px 8px 0; opacity: 0.4; }
.zle-zone {
    min-height: 28px; min-width: 36px; padding: 2px 4px; border-radius: 6px;
    border: 1.5px dashed alpha(white, 0.18); transition: all 200ms ease;
}
.zle-zone-active { border-color: @accent_color; background: alpha(@accent_color, 0.18); border-style: solid; }
.zle-button {
    min-width: 28px; min-height: 28px; padding: 2px; border-radius: 6px;
    background: rgba(255,255,255,0.08); color: white; transition: opacity 150ms ease;
}
.zle-button:hover { background: rgba(255,255,255,0.20); }
.zle-title { color: alpha(white, 0.45); font-size: 11px; font-style: italic; }
";

/// Shared editor state captured by the gesture closures. Holds only WEAK widget
/// refs + plain data, so capturing `Rc<EditorState>` in a closure-on-a-widget
/// never closes a strong widget cycle.
struct EditorState {
    /// Slot keys in spec render order (drives the rebuild walk).
    slot_keys: Vec<String>,
    /// `slot_key` → the zone box (weak; owned by the widget tree).
    slot_boxes: HashMap<String, glib::WeakRef<gtk::Box>>,
    /// `slot_key` → the spoken name of the zone.
    zone_labels: HashMap<String, String>,
    /// `control id` → its draggable button (weak).
    buttons: HashMap<String, glib::WeakRef<gtk::ToggleButton>>,
    /// The live edited value (placements rebuilt from the tree; `hidden` from clicks).
    layout: RefCell<ZoneLayout>,
    /// The change sink set via [`BigZoneLayoutEditor::connect_changed`].
    on_changed: RefCell<Option<ChangeCallback>>,
}

impl EditorState {
    /// Rebuild `layout.slots` from the live widget tree (each zone box's children
    /// in order), keep `hidden` as-is, and notify the change sink.
    fn rebuild_and_notify(&self) {
        self.describe_positions();
        let mut slots: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for key in &self.slot_keys {
            let mut ids = Vec::new();
            if let Some(zone) = self.slot_boxes.get(key).and_then(glib::WeakRef::upgrade) {
                let mut child = zone.first_child();
                while let Some(widget) = child {
                    ids.push(widget.widget_name().to_string());
                    child = widget.next_sibling();
                }
            }
            slots.insert(key.clone(), ids);
        }
        let snapshot = {
            let mut layout = self.layout.borrow_mut();
            layout.slots = slots;
            layout.clone()
        };
        if let Some(cb) = self.on_changed.borrow().as_ref() {
            cb(&snapshot);
        }
    }

    /// Describe each control by its zone and place in it, which a screen
    /// reader says after the name.
    fn describe_positions(&self) {
        for key in &self.slot_keys {
            let Some(zone) = self.slot_boxes.get(key).and_then(glib::WeakRef::upgrade) else {
                continue;
            };
            let zone_label = self.zone_labels.get(key).map_or("", String::as_str);
            let controls: Vec<gtk::Widget> =
                std::iter::successors(zone.first_child(), gtk::Widget::next_sibling).collect();
            for (index, control) in controls.iter().enumerate() {
                let description = position_description(zone_label, index, controls.len());
                control.update_property(&[gtk::accessible::Property::Description(&description)]);
            }
        }
    }

    /// Show or hide a control (opacity + `hidden`) and notify.
    fn set_visibility(&self, id: &str, button: &gtk::ToggleButton) {
        let visible = button.is_active();
        self.layout.borrow_mut().set_visible(id, visible);
        button.set_opacity(if visible { 1.0 } else { 0.3 });
        self.rebuild_and_notify();
    }

    /// Move a control one place toward the end (or start) of the bars and
    /// announce where it landed.
    fn step_control(&self, id: &str, toward_end: bool) {
        let Some(button) = self.buttons.get(id).and_then(glib::WeakRef::upgrade) else {
            return;
        };
        let target = step_target(&self.slot_keys, &self.layout.borrow().slots, id, toward_end);
        let Some((key, index)) = target else {
            button.error_bell();
            return;
        };
        let Some(zone) = self.slot_boxes.get(&key).and_then(glib::WeakRef::upgrade) else {
            return;
        };
        if let Some(parent) = button.parent().and_downcast::<gtk::Box>() {
            parent.remove(&button);
        }
        let after = index.checked_sub(1).and_then(|before| {
            std::iter::successors(zone.first_child(), gtk::Widget::next_sibling).nth(before)
        });
        zone.insert_child_after(&button, after.as_ref());
        button.grab_focus();
        self.rebuild_and_notify();
        let count = self.layout.borrow().slots.get(&key).map_or(0, Vec::len);
        let zone_label = self.zone_labels.get(&key).map_or("", String::as_str);
        button.announce(
            &position_description(zone_label, index, count),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }
}

/// "Bottom bar, left, 2 of 3" for the control at `index` among `count`.
fn position_description(zone_label: &str, index: usize, count: usize) -> String {
    t("{zone}, {position} of {count}")
        .replace("{zone}", zone_label)
        .replace("{position}", &(index + 1).to_string())
        .replace("{count}", &count.to_string())
}

/// The slot and index `id` lands on one step toward the end (or start) of
/// `slot_keys`: within its zone, else at the near edge of the neighboring zone.
/// `None` at either end of the bars.
fn step_target(
    slot_keys: &[String],
    slots: &BTreeMap<String, Vec<String>>,
    id: &str,
    toward_end: bool,
) -> Option<(String, usize)> {
    let zone = slot_keys.iter().position(|key| {
        slots
            .get(key)
            .is_some_and(|ids| ids.iter().any(|x| x == id))
    })?;
    let key = slot_keys.get(zone)?;
    let ids = slots.get(key)?;
    let index = ids.iter().position(|x| x == id)?;
    if toward_end {
        if index + 1 < ids.len() {
            return Some((key.clone(), index + 1));
        }
        Some((slot_keys.get(zone + 1)?.clone(), 0))
    } else {
        if let Some(before) = index.checked_sub(1) {
            return Some((key.clone(), before));
        }
        let previous = slot_keys.get(zone.checked_sub(1)?)?;
        Some((previous.clone(), slots.get(previous).map_or(0, Vec::len)))
    }
}

/// The spoken name of a zone: its bar's label and the side it sits on.
fn zone_label(bar_label: &str, placement: ZonePlacement) -> String {
    let start_is_left = gtk::Widget::default_direction() != gtk::TextDirection::Rtl;
    let template = match placement {
        ZonePlacement::Center => t("{bar}, center"),
        ZonePlacement::Start if start_is_left => t("{bar}, left"),
        ZonePlacement::End if !start_is_left => t("{bar}, left"),
        ZonePlacement::Start | ZonePlacement::End => t("{bar}, right"),
    };
    template.replace("{bar}", bar_label)
}

/// Visual zone-layout editor. Build with [`new`](Self::new), embed
/// [`root`](Self::root), read the current value with [`layout`](Self::layout),
/// and subscribe with [`connect_changed`](Self::connect_changed).
pub struct BigZoneLayoutEditor {
    root: gtk::Frame,
    state: Rc<EditorState>,
}

impl BigZoneLayoutEditor {
    /// Build the editor for `spec`, populated from `initial` (which is
    /// [`normalized`](ZoneLayout::normalized) against the spec first).
    #[must_use]
    pub fn new(spec: &ZoneLayoutSpec, initial: &ZoneLayout) -> Self {
        let layout = initial.normalized(spec);

        // Zone boxes, keyed by slot, owned by the bars built below.
        let mut slot_boxes_strong: HashMap<String, gtk::Box> = HashMap::new();
        for key in spec.slot_keys() {
            let zone = gtk::Box::new(gtk::Orientation::Horizontal, 2);
            zone.add_css_class("zle-zone");
            zone.set_widget_name(&key);
            zone.set_hexpand(true);
            slot_boxes_strong.insert(key, zone);
        }

        // Toggles, keyed by control id: pressed while the control is shown.
        let mut buttons_strong: HashMap<String, gtk::ToggleButton> = HashMap::new();
        for control in &spec.controls {
            let button = gtk::ToggleButton::builder()
                .icon_name(&control.icon)
                .active(layout.is_visible(&control.id))
                .build();
            button.set_widget_name(&control.id);
            button.add_css_class("zle-button");
            button.set_tooltip_text(Some(&control.label));
            button.update_property(&[
                gtk::accessible::Property::Label(&control.label),
                gtk::accessible::Property::KeyShortcuts("Alt+ArrowLeft Alt+ArrowRight"),
            ]);
            button.set_cursor_from_name(Some("grab"));
            button.set_opacity(if layout.is_visible(&control.id) {
                1.0
            } else {
                0.3
            });
            buttons_strong.insert(control.id.clone(), button);
        }

        // Place each control into its zone, in the layout's order.
        for key in spec.slot_keys() {
            if let Some(zone) = slot_boxes_strong.get(&key)
                && let Some(ids) = layout.slots.get(&key)
            {
                for id in ids {
                    if let Some(button) = buttons_strong.get(id) {
                        zone.append(button);
                    }
                }
            }
        }

        // Shared state (weak widget refs only).
        let zone_labels = spec
            .bars
            .iter()
            .flat_map(|bar| {
                bar.zones.iter().map(|&placement| {
                    (
                        slot_key(&bar.id, placement),
                        zone_label(&bar.label, placement),
                    )
                })
            })
            .collect();
        let state = Rc::new(EditorState {
            slot_keys: spec.slot_keys(),
            zone_labels,
            slot_boxes: slot_boxes_strong
                .iter()
                .map(|(k, b)| (k.clone(), b.downgrade()))
                .collect(),
            buttons: buttons_strong
                .iter()
                .map(|(k, b)| (k.clone(), b.downgrade()))
                .collect(),
            layout: RefCell::new(layout),
            on_changed: RefCell::new(None),
        });
        state.describe_positions();

        // Per-control: drag source, click-to-toggle and Alt+Left/Right to move.
        for (id, button) in &buttons_strong {
            let drag = gtk::DragSource::new();
            drag.set_actions(gdk::DragAction::MOVE);
            let drag_id = id.clone();
            drag.connect_prepare(move |_, _, _| {
                Some(gdk::ContentProvider::for_value(&drag_id.to_value()))
            });
            button.add_controller(drag);

            {
                let state = state.clone();
                let id = id.clone();
                button.connect_toggled(move |button| state.set_visibility(&id, button));
            }

            let keys = gtk::EventControllerKey::new();
            let state = state.clone();
            let key_id = id.clone();
            keys.connect_key_pressed(move |_, key, _, modifiers| {
                if !modifiers.contains(gdk::ModifierType::ALT_MASK) {
                    return glib::Propagation::Proceed;
                }
                let start_is_left = gtk::Widget::default_direction() != gtk::TextDirection::Rtl;
                let toward_end = match key {
                    gdk::Key::Left => !start_is_left,
                    gdk::Key::Right => start_is_left,
                    _ => return glib::Propagation::Proceed,
                };
                state.step_control(&key_id, toward_end);
                glib::Propagation::Stop
            });
            button.add_controller(keys);
        }

        // Per-zone: drop target (reorder by pointer-x + reparent).
        for (key, zone) in &slot_boxes_strong {
            // moves internal control ids between zones.
            let drop = gtk::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            {
                let zone = zone.downgrade();
                drop.connect_enter(move |_, _, _| {
                    if let Some(zone) = zone.upgrade() {
                        zone.add_css_class("zle-zone-active");
                    }
                    gdk::DragAction::MOVE
                });
            }
            {
                let zone = zone.downgrade();
                drop.connect_leave(move |_| {
                    if let Some(zone) = zone.upgrade() {
                        zone.remove_css_class("zle-zone-active");
                    }
                });
            }
            {
                let state = state.clone();
                let target_key = key.clone();
                drop.connect_drop(move |_, value, x, _| {
                    let Ok(id) = value.get::<String>() else {
                        return false;
                    };
                    let Some(button) = state.buttons.get(&id).and_then(glib::WeakRef::upgrade)
                    else {
                        return false;
                    };
                    let Some(target) = state
                        .slot_boxes
                        .get(&target_key)
                        .and_then(glib::WeakRef::upgrade)
                    else {
                        return false;
                    };
                    if let Some(parent) = button.parent().and_downcast::<gtk::Box>() {
                        parent.remove(&button);
                    }
                    // Insert after the last child whose center is left of the drop x.
                    let mut after: Option<gtk::Widget> = None;
                    let mut child = target.first_child();
                    while let Some(widget) = child {
                        if let Some(bounds) = widget.compute_bounds(&target) {
                            let mid = f64::from(bounds.x()) + f64::from(bounds.width()) / 2.0;
                            if x > mid {
                                after = Some(widget.clone());
                            }
                        }
                        child = widget.next_sibling();
                    }
                    target.insert_child_after(&button, after.as_ref());
                    target.remove_css_class("zle-zone-active");
                    state.rebuild_and_notify();
                    true
                });
            }
            zone.add_controller(drop);
        }

        // Assemble the miniature: one CenterBox per bar, stacked vertically.
        let layout_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        layout_box.add_css_class("zle-layout");
        for bar in &spec.bars {
            if bar.decoration.seek_slider {
                // A picture of the seek bar, not a control.
                let seek = gtk::Scale::builder()
                    .orientation(gtk::Orientation::Horizontal)
                    .adjustment(&gtk::Adjustment::new(35.0, 0.0, 100.0, 1.0, 0.0, 0.0))
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build();
                seek.set_sensitive(false);
                seek.add_css_class("zle-seek");
                layout_box.append(&seek);
            }
            let center_box = gtk::CenterBox::new();
            center_box.add_css_class("zle-bar");
            for &placement in &bar.zones {
                let Some(zone) = slot_boxes_strong.get(&slot_key(&bar.id, placement)) else {
                    continue;
                };
                match placement {
                    ZonePlacement::Start => center_box.set_start_widget(Some(zone)),
                    ZonePlacement::Center => center_box.set_center_widget(Some(zone)),
                    ZonePlacement::End => center_box.set_end_widget(Some(zone)),
                }
            }
            // A title placeholder fills the center when the bar has no center zone.
            if let Some(title) = &bar.decoration.title
                && !bar.zones.contains(&ZonePlacement::Center)
            {
                let label = gtk::Label::new(Some(title));
                label.add_css_class("zle-title");
                label.set_hexpand(true);
                center_box.set_center_widget(Some(&label));
            }
            layout_box.append(&center_box);
        }

        let root = gtk::Frame::new(None);
        root.add_css_class("zle-frame");
        crate::theme::CssSlot::new(&root, gtk::STYLE_PROVIDER_PRIORITY_USER).set(EDITOR_CSS);
        root.set_child(Some(&layout_box));

        Self { root, state }
    }

    /// The editor widget; embed this in a preferences page.
    #[must_use]
    pub fn root(&self) -> &gtk::Frame {
        &self.root
    }

    /// A clone of the current edited value.
    #[must_use]
    pub fn layout(&self) -> ZoneLayout {
        self.state.layout.borrow().clone()
    }

    /// Subscribe to layout changes. Called on every move and visibility
    /// toggle with the new [`ZoneLayout`]. Replaces any previous subscriber.
    pub fn connect_changed(&self, on_change: impl Fn(&ZoneLayout) + 'static) {
        *self.state.on_changed.borrow_mut() = Some(Box::new(on_change));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> Vec<String> {
        ["top:start", "top:end", "bottom:start"]
            .map(String::from)
            .to_vec()
    }

    fn slots() -> BTreeMap<String, Vec<String>> {
        BTreeMap::from([
            ("top:start".into(), vec!["pin".into(), "open".into()]),
            ("top:end".into(), Vec::new()),
            ("bottom:start".into(), vec!["volume".into(), "speed".into()]),
        ])
    }

    fn step(id: &str, toward_end: bool) -> Option<(String, usize)> {
        step_target(&keys(), &slots(), id, toward_end)
    }

    #[test]
    fn step_moves_within_a_zone() {
        assert_eq!(step("pin", true), Some(("top:start".into(), 1)));
        assert_eq!(step("speed", false), Some(("bottom:start".into(), 0)));
    }

    #[test]
    fn step_crosses_into_neighbor_zone() {
        assert_eq!(step("open", true), Some(("top:end".into(), 0)));
        assert_eq!(step("volume", false), Some(("top:end".into(), 0)));
        let mut full = slots();
        full.insert("top:end".into(), vec!["menu".into(), "playlist".into()]);
        assert_eq!(
            step_target(&keys(), &full, "volume", false),
            Some(("top:end".into(), 2))
        );
    }

    #[test]
    fn step_stops_at_bar_ends() {
        assert_eq!(step("pin", false), None);
        assert_eq!(step("speed", true), None);
        assert_eq!(step("unknown", true), None);
    }
}
