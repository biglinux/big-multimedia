// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Standard BigLinux hamburger menu builder.
//!
//! The framework contract is intentionally narrow: a hamburger menu is a native
//! `GtkMenuButton` with explicit action rows. Apps provide labels and action
//! IDs; the framework owns tooltip labels, popover behavior, and accessible
//! action rows.

use relm4::gtk;
use relm4::gtk::gio;
use relm4::gtk::prelude::*;

const ACTION_POPOVER_QDATA_KEY: &str = "big-action-popover";

#[path = "hamburger_menu/action_row.rs"]
mod action_row;

use action_row::build_action_menu_row;

/// One visible menu item bound to one detailed action name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMenuActionItem {
    /// User-visible item label.
    pub label: String,
    /// Detailed action name, for example `win.preferences` or `app.about`.
    pub action: String,
    /// Whether the row can be activated.
    pub enabled: bool,
    /// Optional symbolic icon shown before the label.
    pub leading_icon_name: Option<String>,
    /// Optional keyboard shortcut or trailing hint shown after the label.
    pub shortcut_label: Option<String>,
}

impl BigMenuActionItem {
    /// Create a menu item from a localized label and detailed action name.
    #[must_use]
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
            enabled: true,
            leading_icon_name: None,
            shortcut_label: None,
        }
    }
}

/// Specification for a native hamburger menu button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigHamburgerMenuSpec {
    /// Symbolic icon name shown inside the button.
    pub icon_name: String,
    /// Tooltip and accessible label for the icon-only button.
    pub label: String,
    /// CSS classes applied to the menu button.
    pub css_classes: Vec<String>,
    /// Flat ordered menu items.
    pub items: Vec<BigMenuActionItem>,
}

/// Accessible button that opens a flat action popover.
#[derive(Debug, Clone)]
pub struct BigActionPopoverButton {
    root: gtk::Box,
    popover: gtk::Popover,
}

impl BigActionPopoverButton {
    /// The root container that must be inserted into the widget tree.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// The popover opened by the button.
    #[must_use]
    pub fn popover(&self) -> &gtk::Popover {
        &self.popover
    }
}

impl BigHamburgerMenuSpec {
    /// Create a standard hamburger spec with the `open-menu-symbolic` icon.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            icon_name: "open-menu-symbolic".to_owned(),
            label: label.into(),
            css_classes: Vec::new(),
            items: Vec::new(),
        }
    }

    /// Override the symbolic icon name.
    #[must_use]
    pub fn icon_name(mut self, icon_name: impl Into<String>) -> Self {
        self.icon_name = icon_name.into();
        self
    }

    /// Replace the CSS class list.
    #[must_use]
    pub fn css_classes<I, S>(mut self, classes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.css_classes = classes.into_iter().map(Into::into).collect();
        self
    }

    /// Replace the menu item list.
    #[must_use]
    pub fn items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = BigMenuActionItem>,
    {
        self.items = items.into_iter().collect();
        self
    }
}

/// Build a flat native `GMenuModel` from ordered action items.
#[must_use]
pub fn build_flat_menu_model(items: &[BigMenuActionItem]) -> gio::Menu {
    let menu = gio::Menu::new();
    for item in items {
        menu.append(Some(&item.label), Some(&item.action));
    }
    menu
}

/// Collect detailed action names from a menu model recursively.
#[must_use]
pub fn menu_action_names(menu: &impl IsA<gio::MenuModel>) -> Vec<String> {
    let model = menu.as_ref();
    let mut actions = Vec::new();
    for index in 0..model.n_items() {
        if let Some(action) = model
            .item_attribute_value(index, "action", Some(gtk::glib::VariantTy::STRING))
            .and_then(|value| value.get::<String>())
        {
            actions.push(action);
        }
        if let Some(section) = model.item_link(index, "section") {
            actions.extend(menu_action_names(&section));
        }
        if let Some(submenu) = model.item_link(index, "submenu") {
            actions.extend(menu_action_names(&submenu));
        }
    }
    actions
}

/// Attach a flat action popover to `button` with named action rows.
pub fn set_flat_action_popover(button: &gtk::MenuButton, items: &[BigMenuActionItem]) {
    let popover = build_flat_action_popover(button.upcast_ref::<gtk::Widget>(), items);
    button.set_popover(Some(&popover));
}

/// Replace a popover's child with flat named action rows.
pub fn populate_flat_action_popover(
    popover: &gtk::Popover,
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
) {
    popover.set_accessible_role(gtk::AccessibleRole::Group);
    popover.add_css_class("menu");

    let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
    rows.set_margin_top(6);
    rows.set_margin_bottom(6);
    rows.set_margin_start(6);
    rows.set_margin_end(6);

    for item in items {
        rows.append(&build_action_menu_row(
            action_context.as_ref(),
            popover,
            item,
        ));
    }

    popover.set_child(Some(&rows));
}

/// Build a flat action popover with named action rows.
#[must_use]
pub fn build_flat_action_popover(
    action_context: &impl IsA<gtk::Widget>,
    items: &[BigMenuActionItem],
) -> gtk::Popover {
    let popover = gtk::Popover::new();
    populate_flat_action_popover(&popover, action_context, items);
    popover
}

/// Build a tooltip-aware native hamburger button from a flat menu spec.
#[must_use]
pub fn build_hamburger_menu_button(spec: &BigHamburgerMenuSpec) -> gtk::MenuButton {
    let class_refs = spec
        .css_classes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let button =
        crate::feedback::tooltip::icon_menu_button(&spec.icon_name, &spec.label, &class_refs);
    set_flat_action_popover(&button, &spec.items);
    button
}

/// Build an AT-SPI-actionable button that opens a flat action popover.
#[must_use]
pub fn build_flat_action_popover_button(spec: &BigHamburgerMenuSpec) -> BigActionPopoverButton {
    let class_refs = spec
        .css_classes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let button = crate::feedback::tooltip::icon_button(&spec.icon_name, &spec.label, &class_refs);
    let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    root.set_valign(gtk::Align::Center);
    root.append(&button);
    let popover = build_flat_action_popover(&button, &spec.items);
    attach_popover_to_button_with_parent(&root, &button, &popover);
    BigActionPopoverButton { root, popover }
}

/// Parent `popover` to `button` and open it when the button is activated.
pub fn attach_popover_to_button(button: &gtk::Button, popover: &gtk::Popover) {
    attach_popover_to_button_with_parent(button, button, popover);
}

/// Parent `popover` to a layout-managed root and open it from `button`.
pub fn attach_popover_to_button_with_parent(
    popover_parent: &impl IsA<gtk::Widget>,
    button: &gtk::Button,
    popover: &gtk::Popover,
) {
    popover.set_parent(popover_parent.as_ref());
    unsafe {
        // SAFETY: this module is the only writer for ACTION_POPOVER_QDATA_KEY,
        // and the value is always a strong gtk::Popover reference tied to the
        // parent lifetime. GLib drops the stored value when the parent finalizes.
        popover_parent
            .as_ref()
            .set_data(ACTION_POPOVER_QDATA_KEY, popover.clone());
    }
    {
        let popover = popover.downgrade();
        popover_parent.as_ref().connect_destroy(move |_| {
            if let Some(popover) = popover.upgrade() {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            }
        });
    }
    {
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            if popover.is_mapped() {
                popover.popdown();
            } else {
                popover.popup();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn parse_detailed_action_preserves_string_target() {
        let (action_name, target_value) = action_row::parse_detailed_action("fmsort.by::size")
            .expect("detailed action with string target parses");

        assert_eq!(action_name, "fmsort.by");
        assert_eq!(
            target_value.and_then(|value| value.str().map(str::to_owned)),
            Some("size".to_owned())
        );
    }

    #[test]
    fn hamburger_spec_defaults_to_open_menu_icon() {
        let spec = BigHamburgerMenuSpec::new("Main Menu");

        assert_eq!(spec.icon_name, "open-menu-symbolic");
        assert_eq!(spec.label, "Main Menu");
        assert!(spec.css_classes.is_empty());
        assert!(spec.items.is_empty());
    }
}
