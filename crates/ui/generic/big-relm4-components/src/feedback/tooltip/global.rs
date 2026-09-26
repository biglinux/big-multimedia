// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Process-local tooltip facade functions.

use adw::prelude::*;
use relm4::gtk;

use super::{BigTooltipHandle, GLOBAL};

/// Attach a tooltip to a widget using the process-local helper.
pub fn set(widget: &impl IsA<gtk::Widget>, text: impl AsRef<str>) {
    let text = text.as_ref();
    if text.is_empty() {
        return;
    }
    with_global(|handle| handle.add(widget, text));
}

/// Attach a tooltip with vertical anchor offset.
pub fn set_with_offset(widget: &impl IsA<gtk::Widget>, text: &str, y_offset: i32) {
    if text.is_empty() {
        return;
    }
    with_global(|handle| handle.add_with_offset(widget, text, y_offset));
}

/// Update a previously attached tooltip, falling back to attach when needed.
pub fn update(widget: &impl IsA<gtk::Widget>, text: impl AsRef<str>) {
    let text = text.as_ref();
    with_global(|handle| handle.update_text(widget, text));
}

/// Remove a tooltip attached through this module.
pub fn clear(widget: &impl IsA<gtk::Widget>) {
    with_global(|handle| handle.clear(widget));
}

/// Enable or disable tooltip display globally.
pub fn set_enabled(enabled: bool) {
    with_global(|handle| handle.set_enabled(enabled));
}

fn with_global<F: FnOnce(&BigTooltipHandle)>(f: F) {
    GLOBAL.with(|cell| {
        let mut handle = cell.borrow_mut();
        let handle = handle.get_or_insert_with(BigTooltipHandle::default);
        f(handle);
    });
}
