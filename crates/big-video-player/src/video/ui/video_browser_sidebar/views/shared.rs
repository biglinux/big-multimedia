use std::rc::Rc;

use adw::prelude::*;

use super::super::{BrowserState, chrome};

/// Attach a right-click gesture that opens the per-row context menu for videos.
pub(super) fn attach_context_menu(state: &Rc<BrowserState>, row: &gtk::Box) {
    let click = gtk::GestureClick::new();
    click.set_button(3);
    let state = state.clone();
    let row_weak = row.downgrade();
    click.connect_released(move |gesture, _, x, y| {
        gesture.set_state(gtk::EventSequenceState::Claimed);
        let Some(row) = row_weak.upgrade() else {
            return;
        };
        let name = row.widget_name();
        let Some(idx) = name
            .strip_prefix("video:")
            .and_then(|s| s.parse::<usize>().ok())
        else {
            return;
        };
        let path = state.entries.borrow().get(idx).cloned();
        if let Some(path) = path {
            chrome::show_row_context_menu(&state, row.upcast_ref::<gtk::Widget>(), &path, x, y);
        }
    });
    row.add_controller(click);
}

pub(super) fn centered_icon(name: &str, size: i32) -> gtk::Image {
    let icon = gtk::Image::from_icon_name(name);
    icon.set_pixel_size(size);
    icon.set_halign(gtk::Align::Center);
    icon.set_valign(gtk::Align::Center);
    icon
}

pub(super) fn set_accent(widget: &impl IsA<gtk::Widget>, on: bool) {
    if on {
        widget.add_css_class("accent");
    } else {
        widget.remove_css_class("accent");
    }
}

pub(super) fn tooltip_set(widget: &impl IsA<gtk::Widget>, text: &str) {
    big_relm4_components::feedback::tooltip::set(widget, text);
}
