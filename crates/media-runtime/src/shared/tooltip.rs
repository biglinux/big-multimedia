//! Adapter for the shared BigLinux tooltip helper.
//!
//! App code must use this module, not `set_tooltip_text` directly.
//! bigagents: app-local-adapter; wraps component with app empty-text policy.

use adw::prelude::*;

/// No-op when `text` is empty. Lazily creates the shared helper.
pub fn set(widget: &impl IsA<gtk::Widget>, text: &str) {
    if text.is_empty() {
        return;
    }
    big_relm4_components::feedback::tooltip::set(widget, text);
}

/// `y_offset` shifts the popover off slim widgets so it doesn't cover them.
pub fn set_with_offset(widget: &impl IsA<gtk::Widget>, text: &str, y_offset: i32) {
    if text.is_empty() {
        return;
    }
    big_relm4_components::feedback::tooltip::set_with_offset(widget, text, y_offset);
}

pub fn update(widget: &impl IsA<gtk::Widget>, text: &str) {
    big_relm4_components::feedback::tooltip::update(widget, text);
}

pub fn clear(widget: &impl IsA<gtk::Widget>) {
    big_relm4_components::feedback::tooltip::clear(widget);
}
