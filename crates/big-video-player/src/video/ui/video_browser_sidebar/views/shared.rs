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
        let Some(idx) = row_index(&row.widget_name()) else {
            return;
        };
        let path = state.entries.borrow().get(idx).cloned();
        if let Some(path) = path {
            chrome::show_row_context_menu(&state, row.upcast_ref::<gtk::Widget>(), &path, x, y);
        }
    });
    row.add_controller(click);
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
