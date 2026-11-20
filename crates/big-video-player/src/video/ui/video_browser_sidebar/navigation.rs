//! Folder navigation in the Files tab: the places page, the off-thread
//! folder scan and its sub-folders, and reopening the last folder.

use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::{BrowserState, chrome, clear_sidebar_container};
use crate::settings::keys;

impl BrowserState {
    /// Back to the library's places, leaving the open folder.
    pub(super) fn show_places(self: &Rc<Self>) {
        self.scan_generation
            .set(self.scan_generation.get().wrapping_add(1));
        self.folder.replace(None);
        self.settings.put_string(keys::PLAYLIST_LAST_FOLDER, "");
        self.entries.replace(Vec::new());
        self.subfolders.replace(Vec::new());
        self.selection.borrow_mut().clear();
        self.select_mode.set(false);
        self.recompute_pos();
        self.rebuild();
    }

    /// Open the folder browsed last time, unless it is gone or the person
    /// navigated while it was being checked.
    pub(super) fn restore_last_folder(self: &Rc<Self>) {
        let folder = self.settings.get_string(keys::PLAYLIST_LAST_FOLDER);
        if folder.is_empty() {
            return;
        }
        let generation = self.scan_generation.get();
        let state = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let probe = folder.clone();
            let exists = gio::spawn_blocking(move || std::path::Path::new(&probe).is_dir())
                .await
                .unwrap_or(false);
            if let Some(state) = state.upgrade()
                && exists
                && state.scan_generation.get() == generation
                && state.folder.borrow().is_none()
                && !state.customizing.get()
            {
                state.navigate_to_folder(&folder);
            }
        });
    }

    /// Scan `folder` (flat or recursive per settings) and rebuild, cancelling
    /// any scan still in flight. Browsing never touches playback; only a video
    /// activation does.
    pub(super) fn navigate_to_folder(self: &Rc<Self>, folder: &str) {
        let folder_owned = folder.to_string();
        let recursive = self.settings.get_bool(keys::PLAYLIST_FOLDER_RECURSIVE);

        self.folder.replace(Some(folder_owned.clone()));
        self.customizing.set(false);
        self.settings
            .put_string(keys::PLAYLIST_LAST_FOLDER, &folder_owned);

        let generation = self.scan_generation.get().wrapping_add(1);
        self.scan_generation.set(generation);

        // Spinner while the blocking scan runs.
        self.show_scanning_spinner();

        // The saved sort applies to every listing, not only after a pick.
        let sort_key = self.settings.get_string(keys::PLAYLIST_SORT);
        let sort_desc = self.settings.get_bool(keys::PLAYLIST_SORT_DESC);
        let state = self.clone();
        glib::spawn_future_local(async move {
            let folder_clone = folder_owned.clone();
            // Sub-folder rows (flat mode only) are enumerated in the same
            // off-thread pass as the file scan, so the view builder reads them
            // from the cache instead of doing a blocking `read_dir` on the loop.
            let (scan, subfolders) = gio::spawn_blocking(move || {
                use crate::media::playlist;
                let path = std::path::Path::new(&folder_clone);
                let mut scan = if recursive {
                    playlist::scan_folder_recursive(path)
                } else {
                    playlist::ScanResult {
                        files: playlist::scan_folder_flat(path),
                        ..Default::default()
                    }
                };
                playlist::sort_paths(&mut scan.files, &sort_key, sort_desc);
                let subfolders = if recursive {
                    Vec::new()
                } else {
                    playlist::list_subfolders(path)
                };
                (scan, subfolders)
            })
            .await
            .unwrap_or_default();

            // Folder switched again while scanning → drop this stale result.
            if state.scan_generation.get() != generation {
                return;
            }

            state
                .truncated_total
                .set(scan.truncated.then_some(scan.total_found));
            state.entries.replace(scan.files);
            state.subfolders.replace(subfolders);
            state.recompute_pos();
            state.rebuild();
        });
    }

    /// Enumerate the current `folder`'s sub-folders off the main thread, then
    /// re-run the listing so the browse context shows them. Used by the
    /// single-file-open path (which sets `folder` without a full folder scan).
    /// No-op in recursive mode (flat tree → no sub-folder rows) or with no
    /// folder. Guarded by `scan_generation`: a navigation that supersedes this
    /// drops the stale result.
    pub(super) fn refresh_subfolders(self: &Rc<Self>) {
        if self.settings.get_bool(keys::PLAYLIST_FOLDER_RECURSIVE) {
            return;
        }
        let Some(folder) = self.folder.borrow().clone() else {
            return;
        };
        let generation = self.scan_generation.get();
        let state = self.clone();
        glib::spawn_future_local(async move {
            let scanned_folder = folder.clone();
            let subfolders = gio::spawn_blocking(move || {
                crate::media::playlist::list_subfolders(std::path::Path::new(&folder))
            })
            .await
            .unwrap_or_default();
            // Folder navigated/changed while enumerating → drop this stale
            // result (generation bumps on navigation; identity guards a second
            // single-file open that swapped the folder at the same generation).
            if state.scan_generation.get() != generation
                || state.folder.borrow().as_deref() != Some(scanned_folder.as_str())
            {
                return;
            }
            if subfolders.is_empty() {
                return;
            }
            state.subfolders.replace(subfolders);
            state.rebuild();
        });
    }

    /// Replace the container with a centered scanning spinner (header kept).
    pub(super) fn show_scanning_spinner(self: &Rc<Self>) {
        let Some(container) = self.container() else {
            return;
        };
        clear_sidebar_container(&container);
        chrome::build_header(self);

        let spinner_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .valign(gtk::Align::Center)
            .vexpand(true)
            .build();
        spinner_box.append(
            &adw::Spinner::builder()
                .width_request(32)
                .height_request(32)
                .halign(gtk::Align::Center)
                .build(),
        );
        spinner_box.append(
            &gtk::Label::builder()
                .label(crate::i18n::i18n("Scanning…"))
                .css_classes(["dim-label"])
                .halign(gtk::Align::Center)
                .build(),
        );
        container.append(&spinner_box);
    }
}
