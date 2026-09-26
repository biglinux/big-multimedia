// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared container mutation helpers.

use adw::prelude::*;
use relm4::gtk;

/// Remove every child from a `GtkBox`.
pub fn clear_box_children(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}
