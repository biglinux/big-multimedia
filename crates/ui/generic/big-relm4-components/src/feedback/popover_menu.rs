// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Accessible names for the items of a `GtkPopoverMenu`.

use relm4::gtk;
use relm4::gtk::gio;
use relm4::gtk::prelude::*;

/// A `GtkPopoverMenu` over `model` whose items carry their visible text as
/// their accessible name.
#[must_use]
pub fn from_model(model: &impl IsA<gio::MenuModel>) -> gtk::PopoverMenu {
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner of named popover menus"
    )]
    let menu = gtk::PopoverMenu::from_model(Some(model));
    name_menu_items(&menu);
    menu
}

/// Give `button` the menu `model`, with items named as in [`from_model`].
pub fn set_menu_model(button: &gtk::MenuButton, model: &impl IsA<gio::MenuModel>) {
    #[expect(
        clippy::disallowed_methods,
        reason = "the owner of named popover menus"
    )]
    button.set_menu_model(Some(model));
    if let Some(menu) = button.popover().and_downcast::<gtk::PopoverMenu>() {
        name_menu_items(&menu);
    }
}

/// GTK 4.24 labels a menu item by a child label whose role is presentation,
/// and its name computation skips such labels, so every item reaches a screen
/// reader unnamed; name them each time the menu opens.
fn name_menu_items(menu: &gtk::PopoverMenu) {
    menu.connect_map(|menu| name_items_below(menu.upcast_ref()));
}

fn name_items_below(widget: &gtk::Widget) {
    let mut child = widget.first_child();
    while let Some(item) = child {
        let is_menu_item = matches!(
            item.accessible_role(),
            gtk::AccessibleRole::MenuItem
                | gtk::AccessibleRole::MenuItemCheckbox
                | gtk::AccessibleRole::MenuItemRadio
        );
        let label = std::iter::successors(item.first_child(), gtk::Widget::next_sibling)
            .find_map(|part| part.downcast::<gtk::Label>().ok());
        if is_menu_item && let Some(label) = label {
            item.reset_relation(gtk::AccessibleRelation::LabelledBy);
            item.update_property(&[gtk::accessible::Property::Label(&label.text())]);
        }
        name_items_below(&item);
        child = item.next_sibling();
    }
}
