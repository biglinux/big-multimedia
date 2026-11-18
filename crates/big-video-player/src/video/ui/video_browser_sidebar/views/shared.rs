use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::feedback::context_popover::open_context_menu_on_keys;

use super::super::{BrowserState, chrome};

/// Open the per-row context menu on a right-click or a touch long-press.
pub(super) fn attach_context_menu(state: &Rc<BrowserState>, row: &gtk::Box) {
    let click = gtk::GestureClick::new();
    click.set_button(gtk::gdk::BUTTON_SECONDARY);
    click.connect_released({
        let state = state.clone();
        move |gesture, _, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(row) = gesture.widget() {
                open_row_menu(&state, &row, x, y);
            }
        }
    });
    row.add_controller(click);

    let long_press = gtk::GestureLongPress::new();
    long_press.set_touch_only(true);
    long_press.connect_pressed({
        let state = state.clone();
        move |gesture, x, y| {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            if let Some(row) = gesture.widget() {
                open_row_menu(&state, &row, x, y);
            }
        }
    });
    row.add_controller(long_press);
}

/// Open the focused row's context menu on Menu or Shift+F10.
pub(super) fn attach_context_menu_keys(state: &Rc<BrowserState>, list: &impl IsA<gtk::Widget>) {
    let state = state.clone();
    open_context_menu_on_keys(list, move |row| {
        let (x, y) = (f64::from(row.width()) / 2.0, f64::from(row.height()) / 2.0);
        open_row_menu(&state, row, x, y)
    });
}

/// Show the menu of the video `row` shows; folder rows have none.
fn open_row_menu(state: &Rc<BrowserState>, row: &gtk::Widget, x: f64, y: f64) -> bool {
    let Some(path) = row_index(&row.widget_name()).and_then(|index| state.item_at(index)) else {
        return false;
    };
    chrome::show_row_context_menu(state, row, &path, x, y);
    true
}

/// The list index in a bound row's widget name: `video:{index}` in Files,
/// `q:{index}:{path hash}` in the playlist.
pub(super) fn row_index(name: &str) -> Option<usize> {
    let rest = name
        .strip_prefix("video:")
        .or_else(|| name.strip_prefix("q:"))?;
    rest.split(':').next()?.parse().ok()
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

#[cfg(test)]
mod tests {
    use super::row_index;

    #[test]
    fn row_index_reads_files_and_playlist_row_names() {
        assert_eq!(row_index("video:4"), Some(4));
        assert_eq!(row_index("q:2:9f3a0c"), Some(2));
        assert_eq!(row_index("folder:/home/videos"), None);
        assert_eq!(row_index("unbound"), None);
    }
}
