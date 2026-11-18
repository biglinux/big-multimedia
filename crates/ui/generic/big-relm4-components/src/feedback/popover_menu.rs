// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Accessible names for the items of a `GtkPopoverMenu`.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// Name each item of `menu` after its visible text whenever the menu opens.
///
/// GTK 4.24 labels a menu item by a child label whose role is presentation,
/// and its name computation skips such labels, so every item reaches a screen
/// reader unnamed.
pub fn name_menu_items(menu: &gtk::PopoverMenu) {
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
