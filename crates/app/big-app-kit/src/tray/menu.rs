// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! The `com.canonical.dbusmenu` layout of a fixed tray menu. Item `n` of the
//! spec has menu id `n + 1`; id 0 is the root that holds them all.

use glib::Variant;
use glib::variant::ToVariant;
use gtk::glib;
use std::collections::HashMap;

use super::TrayMenuItem;

/// The layout never changes, so every reply carries the same revision.
pub(super) const REVISION: u32 = 1;

type Properties = HashMap<String, Variant>;
/// `(ia{sv}av)`: id, properties, children.
type Node = (i32, Properties, Vec<Variant>);

/// The spec item behind a menu id, if any.
pub(super) fn item(items: &[TrayMenuItem], id: i32) -> Option<&TrayMenuItem> {
    usize::try_from(id)
        .ok()
        .and_then(|id| id.checked_sub(1))
        .and_then(|index| items.get(index))
}

/// Every property of a menu id; `None` for an id the menu does not have.
fn all_properties(items: &[TrayMenuItem], id: i32) -> Option<Properties> {
    let mut properties = Properties::new();
    if id == 0 {
        properties.insert("children-display".into(), "submenu".to_variant());
        return Some(properties);
    }
    match item(items, id)? {
        TrayMenuItem::Separator => {
            properties.insert("type".into(), "separator".to_variant());
        }
        TrayMenuItem::Action {
            label, icon_name, ..
        } => {
            properties.insert("label".into(), label.to_variant());
            if let Some(icon_name) = icon_name {
                properties.insert("icon-name".into(), icon_name.to_variant());
            }
        }
    }
    Some(properties)
}

/// The properties of `id` a host asked for; an empty list asks for all.
pub(super) fn properties(items: &[TrayMenuItem], id: i32, names: &[String]) -> Option<Properties> {
    let mut properties = all_properties(items, id)?;
    if !names.is_empty() {
        properties.retain(|name, _| names.contains(name));
    }
    Some(properties)
}

/// `GetLayout`: the subtree under `parent`, `depth` levels deep (-1 = all).
/// Items have no children, so only the root ever lists any.
pub(super) fn layout(
    items: &[TrayMenuItem],
    parent: i32,
    depth: i32,
    names: &[String],
) -> Option<Node> {
    let properties = properties(items, parent, names)?;
    let children = if parent == 0 && depth != 0 {
        (1..=i32::try_from(items.len()).unwrap_or(i32::MAX))
            .filter_map(|id| {
                let child = properties_node(items, id, names)?;
                Some(child.to_variant())
            })
            .collect()
    } else {
        Vec::new()
    };
    Some((parent, properties, children))
}

fn properties_node(items: &[TrayMenuItem], id: i32, names: &[String]) -> Option<Node> {
    Some((id, properties(items, id, names)?, Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<TrayMenuItem> {
        vec![
            TrayMenuItem::action("show", "Show"),
            TrayMenuItem::Separator,
            TrayMenuItem::action("quit", "Quit").with_icon("application-exit-symbolic"),
        ]
    }

    #[test]
    fn menu_ids_start_at_one() {
        let items = items();
        assert!(item(&items, 0).is_none());
        assert!(matches!(
            item(&items, 1),
            Some(TrayMenuItem::Action { action, .. }) if action == "show"
        ));
        assert!(matches!(item(&items, 2), Some(TrayMenuItem::Separator)));
        assert!(item(&items, 4).is_none());
        assert!(item(&items, -1).is_none());
    }

    #[test]
    fn root_layout_lists_every_item() {
        let (id, properties, children) = layout(&items(), 0, -1, &[]).unwrap();
        assert_eq!(id, 0);
        assert_eq!(
            properties.get("children-display").and_then(Variant::str),
            Some("submenu")
        );
        assert_eq!(children.len(), 3);
        assert!(
            children
                .iter()
                .all(|child| child.type_().as_str() == "(ia{sv}av)")
        );
        assert_eq!(layout(&items(), 0, 0, &[]).unwrap().2.len(), 0);
    }

    #[test]
    fn layout_serializes_as_dbusmenu_expects() {
        let reply = (REVISION, layout(&items(), 0, -1, &[]).unwrap()).to_variant();
        assert_eq!(reply.type_().as_str(), "(u(ia{sv}av))");
    }

    #[test]
    fn items_describe_themselves() {
        let items = items();
        let separator = properties(&items, 2, &[]).unwrap();
        assert_eq!(
            separator.get("type").and_then(Variant::str),
            Some("separator")
        );
        let quit = properties(&items, 3, &[]).unwrap();
        assert_eq!(quit.get("label").and_then(Variant::str), Some("Quit"));
        assert_eq!(
            quit.get("icon-name").and_then(Variant::str),
            Some("application-exit-symbolic")
        );
        let only_label = properties(&items, 3, &["label".into()]).unwrap();
        assert_eq!(only_label.len(), 1);
        assert!(properties(&items, 9, &[]).is_none());
        assert!(layout(&items, 9, -1, &[]).is_none());
    }
}
