use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::super::library::is_place;
use super::super::{BrowserState, SidebarMode};
use super::context_menu::remove_from_queue;

/// Keyboard controller on the sidebar: Ctrl+F focus, Esc clear, Backspace up,
/// Delete remove the highlighted video.
pub(crate) fn add_key_controller(state: &Rc<BrowserState>, search_entry: &gtk::SearchEntry) {
    let key = gtk::EventControllerKey::new();
    let entry_weak = search_entry.downgrade();
    let state_cl = state.clone();
    key.connect_key_pressed(move |_, keyval, _code, modifiers| {
        let state = &state_cl;
        let ctrl = modifiers.contains(gtk::gdk::ModifierType::CONTROL_MASK);
        match keyval {
            gtk::gdk::Key::f if ctrl => {
                if let Some(entry) = entry_weak.upgrade() {
                    entry.grab_focus();
                }
                glib::Propagation::Stop
            }
            gtk::gdk::Key::Escape => {
                if let Some(entry) = entry_weak.upgrade()
                    && (entry.has_focus() || !entry.text().is_empty())
                {
                    entry.set_text("");
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
            gtk::gdk::Key::BackSpace => {
                // Up to the parent of the browsed folder; from one of the
                // library's own folders, back to the places.
                let Some(folder) = state.folder.borrow().clone() else {
                    return glib::Propagation::Proceed;
                };
                if is_place(&state.settings, &folder) {
                    state.show_places();
                    return glib::Propagation::Stop;
                }
                let parent = std::path::Path::new(&folder)
                    .parent()
                    .filter(|p| p.exists())
                    .map(|p| p.to_string_lossy().into_owned());
                if let Some(parent) = parent {
                    state.navigate_to_folder(&parent);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            gtk::gdk::Key::Delete if state.mode.get() == SidebarMode::Playlist => {
                let pos = state.pos.get();
                let path = usize::try_from(pos)
                    .ok()
                    .and_then(|i| state.queue.borrow().get(i).cloned());
                if let Some(path) = path {
                    remove_from_queue(state, &path);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
            _ => glib::Propagation::Proceed,
        }
    });
    if let Some(panel) = state.container() {
        panel.add_controller(key);
    }
}
