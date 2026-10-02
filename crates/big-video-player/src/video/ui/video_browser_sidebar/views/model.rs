use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib};

use super::super::{BrowserState, FilterKind};
use crate::i18n::i18n;
use crate::media::playlist::{self, EntryKind, PlaylistEntry};

/// Build the backing store: parent + sub-folder rows (flat mode only) followed
/// by the video rows. Returns the store and the leading folder-row count. The
/// `subfolders` come from `BrowserState` (enumerated off the main thread by the
/// folder scan) so this never does a blocking `read_dir` on the GTK loop.
pub(super) fn build_store(
    items: &[String],
    recursive_on: bool,
    folder: Option<&String>,
    subfolders: &[String],
) -> (gio::ListStore, u32) {
    let store = gio::ListStore::new::<glib::BoxedAnyObject>();

    // Recursive mode flattens the tree → no parent/sub folders to surface.
    if !recursive_on && let Some(folder) = folder {
        let folder_path = Path::new(folder);
        if let Some(parent) = folder_path.parent()
            && parent.exists()
        {
            store.append(&glib::BoxedAnyObject::new(PlaylistEntry {
                path: parent.to_string_lossy().into_owned(),
                kind: EntryKind::ParentFolder,
                index: 0,
                display_name: i18n("Previous folder"),
            }));
        }
        for sub in subfolders {
            let name = Path::new(sub)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            store.append(&glib::BoxedAnyObject::new(PlaylistEntry {
                path: sub.clone(),
                kind: EntryKind::Folder,
                index: 0,
                display_name: name,
            }));
        }
    }

    let folder_count = store.n_items();
    for (i, path) in items.iter().enumerate() {
        store.append(&glib::BoxedAnyObject::new(PlaylistEntry {
            display_name: playlist::display_name(path),
            path: path.clone(),
            kind: EntryKind::Video,
            index: i,
        }));
    }
    (store, folder_count)
}

/// Custom filter combining the kind chip and the fuzzy search query. The
/// state keeps this filter, so the filter holds the state weakly.
pub(super) fn build_filter(state: &Rc<BrowserState>) -> gtk::CustomFilter {
    let state = Rc::downgrade(state);
    gtk::CustomFilter::new(move |obj| {
        let (Some(state), Some(entry_object)) =
            (state.upgrade(), obj.downcast_ref::<glib::BoxedAnyObject>())
        else {
            return false;
        };
        let entry: std::cell::Ref<PlaylistEntry> = entry_object.borrow();
        let kind_chip = state.filter_kind.get();
        match entry.kind {
            EntryKind::ParentFolder | EntryKind::Folder => kind_chip != FilterKind::Videos,
            EntryKind::Video => {
                if kind_chip == FilterKind::Folders {
                    return false;
                }
                let query = state.search_text.borrow();
                if query.is_empty() {
                    return true;
                }
                let name = entry.display_name.to_lowercase();
                name.contains(query.as_str()) || playlist::fuzzy_match(&query, &name).is_some()
            }
        }
    })
}

/// Wire a selection model + scroll-to-current onto a freshly built view, and
/// record the store + folder offset on the shared state.
pub(super) fn finish_view<V: IsA<gtk::Widget>>(
    state: &Rc<BrowserState>,
    view: &V,
    selection: &gtk::SingleSelection,
    store: gio::ListStore,
    filter: gtk::CustomFilter,
    folder_count: u32,
    scroll_to: impl Fn(u32) + 'static,
) -> gtk::ScrolledWindow {
    state.store.replace(Some(store.clone()));
    state.filter.replace(Some(filter));
    state.folder_entry_count.set(folder_count);

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(view)
        .build();

    // The current entry, read when the view is shown: playback may have
    // moved on while the sidebar was closed.
    let n_items = store.n_items();
    let selection = selection.clone();
    let state_weak = Rc::downgrade(state);
    let show_current = move || {
        let Some(state) = state_weak.upgrade() else {
            return;
        };
        let Ok(current) = u32::try_from(state.pos.get()) else {
            return;
        };
        let target = folder_count + current;
        if target < n_items {
            selection.set_selected(target);
            glib::timeout_add_local_once(Duration::from_millis(100), move || scroll_to(target));
        }
    };
    // A list scrolled before it is ever shown keeps that estimated layout:
    // with the sidebar closed, the rows above the first drawn one stayed
    // blank until the user scrolled. Wait for the map.
    if view.is_mapped() {
        show_current();
    } else {
        let handler = Rc::new(Cell::new(None));
        let pending = RefCell::new(Some(show_current));
        handler.set(Some(view.connect_map({
            let handler = handler.clone();
            move |view| {
                if let Some(id) = handler.take() {
                    view.disconnect(id);
                }
                if let Some(show_current) = pending.take() {
                    show_current();
                }
            }
        })));
    }
    scroll
}
