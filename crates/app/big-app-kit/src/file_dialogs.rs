// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GTK file dialog wrapper for BigLinux desktop apps.

use std::path::PathBuf;

use gtk::gio;
use gtk::prelude::*;

/// GTK file dialog builder. Each constructor names the intent at the call
/// site; the method called last (`open`, `save`, ...) picks the dialog kind.
#[must_use = "FilePicker is a builder; call open, save, or select_folder"]
pub struct FilePicker {
    title: String,
    accept_label: Option<String>,
    initial_name: Option<String>,
    filters: Vec<gtk::FileFilter>,
}

impl FilePicker {
    /// Picker for opening one file.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            accept_label: None,
            initial_name: None,
            filters: Vec::new(),
        }
    }

    /// Picker for opening several files; pair with [`Self::open_multiple`].
    pub fn open_files(title: impl Into<String>) -> Self {
        Self::new(title)
    }

    /// Picker for choosing a folder; pair with [`Self::select_folder`].
    pub fn open_folder(title: impl Into<String>) -> Self {
        Self::new(title)
    }

    /// Picker for a save destination; pair with [`Self::save`].
    pub fn save_file(title: impl Into<String>) -> Self {
        Self::new(title)
    }

    /// Label of the confirm button.
    pub fn accept_label(mut self, label: impl Into<String>) -> Self {
        self.accept_label = Some(label.into());
        self
    }

    /// File name proposed by a save dialog.
    pub fn initial_name(mut self, name: impl Into<String>) -> Self {
        self.initial_name = Some(name.into());
        self
    }

    /// Adds a filter named `name` matching file name suffixes such as `"mp3"`.
    pub fn filter(mut self, name: &str, suffixes: &[&str]) -> Self {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(name));
        for suffix in suffixes {
            filter.add_suffix(suffix);
        }
        self.filters.push(filter);
        self
    }

    /// Adds a filter named `name` matching glob patterns such as `"*.m3u"`.
    pub fn pattern_filter(mut self, name: &str, patterns: &[&str]) -> Self {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(name));
        for pattern in patterns {
            filter.add_pattern(pattern);
        }
        self.filters.push(filter);
        self
    }

    fn build_dialog(&self) -> gtk::FileDialog {
        let mut builder = gtk::FileDialog::builder().title(&self.title).modal(true);
        if let Some(ref label) = self.accept_label {
            builder = builder.accept_label(label);
        }
        if let Some(ref name) = self.initial_name {
            builder = builder.initial_name(name);
        }
        let dialog = builder.build();
        if !self.filters.is_empty() {
            let store = gio::ListStore::new::<gtk::FileFilter>();
            for filter in &self.filters {
                store.append(filter);
            }
            dialog.set_filters(Some(&store));
        }
        dialog
    }

    /// Open the picker as a modal child of `parent`; `callback` runs
    /// with the chosen path on success and is dropped on cancellation
    /// or error.
    pub fn open<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.open_optional(Some(parent), callback);
    }

    /// Same as [`Self::open`] but tolerates a missing parent window;
    /// in that case the portal picks the focus surface itself.
    pub fn open_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        dialog.open(parent, None::<&gio::Cancellable>, move |result| {
            if let Ok(file) = result
                && let Some(path) = file.path()
            {
                callback(path);
            }
        });
    }

    /// Multi-select variant of [`Self::open`]; `callback` runs with the
    /// non-empty selection or is dropped when the user cancels.
    pub fn open_multiple<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Vec<PathBuf>) + 'static,
    {
        self.open_multiple_optional(Some(parent), callback);
    }

    /// Parent-optional multi-select variant of [`Self::open`].
    pub fn open_multiple_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(Vec<PathBuf>) + 'static,
    {
        let dialog = self.build_dialog();
        dialog.open_multiple(parent, None::<&gio::Cancellable>, move |result| {
            if let Ok(files) = result {
                let paths = collect_paths(&files);
                if !paths.is_empty() {
                    callback(paths);
                }
            }
        });
    }

    /// Folder-select variant of [`Self::open`]; portal switches to its
    /// folder picker.
    pub fn select_folder<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.select_folder_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::select_folder`].
    pub fn select_folder_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        dialog.select_folder(parent, None::<&gio::Cancellable>, move |result| {
            if let Ok(folder) = result
                && let Some(path) = folder.path()
            {
                callback(path);
            }
        });
    }

    /// Save-as dialog; `callback` receives the chosen destination on
    /// success and is dropped when the user cancels.
    pub fn save<W, F>(self, parent: &W, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        self.save_optional(Some(parent), callback);
    }

    /// Parent-optional variant of [`Self::save`].
    pub fn save_optional<W, F>(self, parent: Option<&W>, callback: F)
    where
        W: IsA<gtk::Window>,
        F: FnOnce(PathBuf) + 'static,
    {
        let dialog = self.build_dialog();
        dialog.save(parent, None::<&gio::Cancellable>, move |result| {
            if let Ok(file) = result
                && let Some(path) = file.path()
            {
                callback(path);
            }
        });
    }
}

fn collect_paths(files: &gio::ListModel) -> Vec<PathBuf> {
    (0..files.n_items())
        .filter_map(|i| files.item(i))
        .filter_map(|obj| obj.downcast::<gio::File>().ok())
        .filter_map(|file| file.path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    #[cfg_attr(miri, ignore)]
    fn collect_paths_keeps_local_file_order() {
        let temporary_directory = tempdir().expect("temporary path collection directory");
        let first_path = temporary_directory.path().join("first.txt");
        let second_path = temporary_directory.path().join("second.txt");
        let file_store = gio::ListStore::new::<gio::File>();
        file_store.append(&gio::File::for_path(&first_path));
        file_store.append(&gio::File::for_path(&second_path));

        let collected_paths = collect_paths(file_store.upcast_ref());

        assert_eq!(collected_paths, vec![first_path, second_path]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn collect_paths_ignores_non_file_objects() {
        let menu_store = gio::ListStore::new::<gio::Menu>();
        menu_store.append(&gio::Menu::new());

        let collected_paths = collect_paths(menu_store.upcast_ref());

        assert!(collected_paths.is_empty());
    }
}
