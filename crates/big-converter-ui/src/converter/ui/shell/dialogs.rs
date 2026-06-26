//! Confirmation dialogs, the output-dir / add-files / add-folder / network
//! choosers, and the window-close flow for
//! [`ConverterShell`](super::ConverterShell). Split out of `shell.rs` (recipe:
//! file-size budget); pure move, behavior identical. Methods + cross-module
//! choosers are `pub(super)`.

use adw::prelude::*;
use gtk::{gio, glib};
use log::info;
use relm4::{ComponentController, ComponentSender};

use big_app_kit::file_dialogs::FilePicker;

use crate::converter::app::ConverterMode;
use crate::converter::settings::{self as conv_settings, keys};
use crate::converter::ui::sidebar::ConverterSidebarInput;
use crate::i18n::i18n;

use super::setup::collect_media_files;
use super::{ConverterShell, ShellMsg};

impl ConverterShell {
    pub(super) fn show_output_dir_chooser(&self) {
        let window = self.window.clone();
        let settings = self.settings.clone();
        let sidebar = self.sidebar.sender().clone();
        FilePicker::destination_folder(i18n("Output Directory")).select_folder(
            &self.window,
            move |path| {
                let path_str = path.to_string_lossy().to_string();
                sidebar.emit(ConverterSidebarInput::SetOutputDir(path_str.clone()));
                settings.put_string(keys::OUTPUT_DIR, &path_str);
                settings.flush();
                let _ = &window;
                info!("Output directory: {path_str}");
            },
        );
    }

    pub(super) fn confirm_clear_queue(&self, sender: &ComponentSender<Self>) {
        let dialog = crate::shared::ui_helpers::new_confirm_dialog(
            &i18n("Clear queue?"),
            &i18n("All files in the queue will be removed."),
            "clear",
            &i18n("Clear"),
            true,
        );
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "clear" {
                s.input(ShellMsg::ClearQueueConfirmed);
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn confirm_restore_defaults(&self, _sender: &ComponentSender<Self>) {
        let sidebar = self.sidebar.sender().clone();
        let dialog = crate::shared::ui_helpers::new_confirm_dialog(
            &i18n("Restore Defaults?"),
            &i18n("All settings will be reset to their default values."),
            "restore",
            &i18n("Restore"),
            true,
        );
        dialog.connect_response(None, move |_, resp| {
            if resp == "restore" {
                // The sidebar owns the widgets: it resets GSettings + reloads
                // every effect control + re-emits ParamsChanged.
                sidebar.emit(ConverterSidebarInput::RestoreDefaults);
                info!("Settings restored to defaults");
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn confirm_remove_item(&self, item_id: u64, sender: &ComponentSender<Self>) {
        let dialog = crate::shared::ui_helpers::new_confirm_dialog(
            &i18n("Remove this item from the queue?"),
            &i18n("Edits applied to this file (segments, filters, output settings) will be lost."),
            "remove",
            &i18n("Remove"),
            true,
        );
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "remove" {
                s.input(ShellMsg::QueueRemoveConfirmed(item_id));
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn confirm_delete_disk(&self, item_id: u64, sender: &ComponentSender<Self>) {
        let Some(path) = self
            .queue
            .items()
            .iter()
            .find(|i| i.id == item_id)
            .map(|i| i.input.clone())
        else {
            return;
        };
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = format!(
            "{filename}\n\n{}",
            i18n("This will permanently delete the file from disk.")
        );
        let dialog = crate::shared::ui_helpers::new_confirm_dialog(
            &i18n("Delete File?"),
            &body,
            "delete",
            &i18n("Delete"),
            true,
        );
        let s = sender.clone();
        dialog.connect_response(None, move |_, resp| {
            if resp == "delete" {
                if let Err(e) = std::fs::remove_file(&path) {
                    log::error!("Failed to delete {}: {e}", path.display());
                } else {
                    log::info!("Deleted {}", path.display());
                    s.input(ShellMsg::QueueRemoveConfirmed(item_id));
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    pub(super) fn perform_window_close(&mut self, sender: &ComponentSender<Self>) {
        if self.is_closing.get() {
            return;
        }
        if self.converting {
            // Ask first. Do NOT latch is_closing here: if the user keeps
            // converting they must be able to request close again later.
            let win = self.window.clone();
            let s = sender.clone();
            glib::spawn_future_local(async move {
                let dialog = big_app_kit::dialogs::confirm_dialog(
                    &i18n("Conversion in progress"),
                    &i18n("Closing will cancel the current conversion. Are you sure?"),
                    &i18n("Continue converting"),
                    "close",
                    &i18n("Cancel and close"),
                    true,
                );
                if dialog.choose_future(Some(&win)).await == "close" {
                    s.input(ShellMsg::ConfirmedClose);
                }
            });
            return;
        }
        self.save_and_close();
    }

    pub(super) fn save_and_close(&mut self) {
        if !self.window.is_maximized() {
            let _ = self
                .settings
                .set_int(keys::WINDOW_WIDTH.0, self.window.width());
            let _ = self
                .settings
                .set_int(keys::WINDOW_HEIGHT.0, self.window.height());
        }
        let _ = self
            .settings
            .set_boolean(keys::WINDOW_MAXIMIZED.0, self.window.is_maximized());
        // Audio-effect settings are persisted continuously by the sidebar on
        // every change; flush makes the latest values durable on close.
        self.queue.cancel();
        self.settings.flush();
        self.is_closing.set(true);
        self.window.close();
    }
}

pub(super) fn show_file_chooser(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<ConverterShell>,
    mode: ConverterMode,
) {
    let last_dir = conv_settings::converter_settings(mode).get_string(keys::LAST_DIR);
    let s = sender.clone();
    FilePicker::open_files(i18n("Add Files"))
        .initial_folder(&last_dir)
        .mime_filter(&i18n("Video & Audio Files"), &["video/*", "audio/*"])
        .open_multiple(window, move |paths| {
            if let Some(first) = paths.first().and_then(|p| p.parent()) {
                let st = conv_settings::converter_settings(mode);
                st.put_string(keys::LAST_DIR, &first.to_string_lossy());
                st.flush();
            }
            if !paths.is_empty() {
                s.input(ShellMsg::AddFiles(paths));
            }
        });
}

pub(super) fn show_folder_chooser(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<ConverterShell>,
    mode: ConverterMode,
) {
    let last_dir = conv_settings::converter_settings(mode).get_string(keys::LAST_DIR);
    let s = sender.clone();
    FilePicker::open_folder(i18n("Add Folder"))
        .initial_folder(&last_dir)
        .select_folder(window, move |dir| {
            // Persist the chosen folder (cheap), then run the recursive walk +
            // per-file validation off the main loop — a deep or network-mounted
            // folder would otherwise freeze the UI (mirrors the drop handler in
            // `setup.rs`).
            let st = conv_settings::converter_settings(mode);
            st.put_string(keys::LAST_DIR, &dir.to_string_lossy());
            st.flush();
            let s = s.clone();
            big_relm4_components::task::spawn_blocking_result(
                move || {
                    let mut paths = Vec::new();
                    collect_media_files(&dir, &mut paths);
                    paths.sort();
                    paths
                },
                move |paths| {
                    if !paths.is_empty() {
                        s.input(ShellMsg::AddFiles(paths));
                    }
                },
            );
        });
}

pub(super) fn show_network_url_dialog(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<ConverterShell>,
) {
    let (dialog, entry) = big_app_kit::dialogs::entry_dialog(
        &i18n("Add Network File"),
        &i18n("Enter the file URL (sftp://, smb://, ftp://, http://):"),
        &i18n("Cancel"),
        "add",
        &i18n("Add"),
        "sftp://server/path/video.mp4",
        &i18n("Network file URL"),
    );

    let s = sender.clone();
    dialog.connect_response(None, move |_dlg, resp| {
        if resp != "add" {
            return;
        }
        let url = entry.text().trim().to_string();
        if url.is_empty() {
            return;
        }
        let file = gio::File::for_uri(&url);
        let cancellable = gio::Cancellable::new();
        let cancellable_for_timeout = cancellable.clone();
        glib::timeout_add_seconds_local_once(30, move || {
            cancellable_for_timeout.cancel();
        });
        let s2 = s.clone();
        file.mount_enclosing_volume(
            gio::MountMountFlags::NONE,
            None::<&gio::MountOperation>,
            Some(&cancellable),
            move |result| {
                let _ = result;
                let file = gio::File::for_uri(&url);
                if let Some(path) = file.path() {
                    s2.input(ShellMsg::AddFiles(vec![path]));
                }
            },
        );
    });

    dialog.present(Some(window.upcast_ref::<gtk::Window>()));
}
