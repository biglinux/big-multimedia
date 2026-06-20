//! Triage file operations for Files mode: the multi-select batch action bar and
//! the move / rename helpers. Destructive or launch-style actions (send to
//! converter, trash) are emitted to the shell (which owns toasts + process
//! spawning); move and rename are filesystem ops done here, followed by a
//! folder re-scan. All act on the current `selection` (batch) or a single path.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;

use super::{BrowserState, VideoBrowserSidebarOutput};
use crate::i18n::{i18n, ni18n};

/// "{n} selected" label text for the batch bar.
pub(super) fn selection_count_text(n: usize) -> String {
    ni18n("{} selected", "{} selected", n as u64).replace("{}", &n.to_string())
}

/// Bottom batch-action bar shown while in triage (select) mode: a live count and
/// the actions that operate on the whole selection.
pub(super) fn build_batch_bar(state: &Rc<BrowserState>) {
    let count = state.selection.borrow().len();
    let bar = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(8)
        .margin_end(8)
        .margin_top(4)
        .margin_bottom(8)
        .css_classes(["toolbar"])
        .build();

    let count_label = gtk::Label::builder()
        .label(selection_count_text(count))
        .halign(gtk::Align::Start)
        .css_classes(["dim-label", "caption"])
        .build();
    bar.append(&count_label);
    state.select_count_label.replace(Some(count_label));

    let actions = gtk::Box::builder()
        .spacing(4)
        .halign(gtk::Align::Center)
        .build();
    actions.append(&batch_button(
        state,
        "camera-video-symbolic",
        &i18n("Send to converter"),
        |state, sel| {
            state
                .sender
                .output(VideoBrowserSidebarOutput::SendToConverter(sel))
                .ok();
        },
    ));
    actions.append(&batch_button(
        state,
        "folder-move-symbolic",
        &i18n("Move to folder…"),
        move_to_folder,
    ));
    actions.append(&batch_button(
        state,
        "list-add-symbolic",
        &i18n("Add to playlist"),
        |state, sel| {
            state
                .sender
                .output(VideoBrowserSidebarOutput::Enqueue(sel))
                .ok();
        },
    ));
    actions.append(&batch_button(
        state,
        "user-trash-symbolic",
        &i18n("Move to trash"),
        |state, sel| {
            state
                .sender
                .output(VideoBrowserSidebarOutput::TrashFiles(sel))
                .ok();
        },
    ));

    let done = gtk::Button::builder()
        .icon_name("window-close-symbolic")
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&done, &i18n("Done"));
    done.update_property(&[gtk::accessible::Property::Label(&i18n("Done"))]);
    done.connect_clicked({
        let state = state.clone();
        move |_| state.set_select_mode(false)
    });
    actions.append(&done);

    bar.append(&actions);
    if let Some(panel) = state.container() {
        panel.append(&bar);
    }
}

/// A flat batch-action button: snapshots the selection and, if non-empty, runs
/// `action` then leaves select mode.
fn batch_button(
    state: &Rc<BrowserState>,
    icon: &str,
    label: &str,
    action: impl Fn(&Rc<BrowserState>, Vec<String>) + 'static,
) -> gtk::Button {
    let btn = gtk::Button::builder()
        .icon_name(icon)
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&btn, label);
    btn.update_property(&[gtk::accessible::Property::Label(label)]);
    btn.connect_clicked({
        let state = state.clone();
        move |_| {
            let sel: Vec<String> = state.selection.borrow().iter().cloned().collect();
            if sel.is_empty() {
                return;
            }
            action(&state, sel);
            state.set_select_mode(false);
        }
    });
    btn
}

/// Pick a destination folder, move `paths` into it, re-scan, and report.
pub(super) fn move_to_folder(state: &Rc<BrowserState>, paths: Vec<String>) {
    let Some(window) = state.window() else {
        return;
    };
    let state = state.clone();
    FilePicker::new(i18n("Move to Folder")).select_folder(&window, move |dest| {
        // File moves (incl. cross-device copy of large videos) run off-thread.
        let state = state.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || {
                let mut moved = 0_usize;
                let mut failed = 0_usize;
                for src in &paths {
                    if move_file(src, &dest).is_ok() {
                        moved += 1;
                    } else {
                        failed += 1;
                    }
                }
                (moved, failed)
            },
            move |(moved, failed)| {
                notify_op_result(&state, moved, failed, &i18n("moved"));
                state.refresh_current_folder();
            },
        );
    });
}

/// Rename a single file via a small dialog, then re-scan.
pub(super) fn rename_file(state: &Rc<BrowserState>, path: &str) {
    let Some(window) = state.window() else {
        return;
    };
    let old = PathBuf::from(path);
    let Some(parent) = old.parent().map(Path::to_path_buf) else {
        return;
    };
    let current_name = old
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (dialog, entry) = big_app_kit::dialogs::entry_dialog_with_initial(
        &i18n("Rename"),
        "",
        &i18n("Cancel"),
        "rename",
        &i18n("Rename"),
        &i18n("New name"),
        &current_name,
        &i18n("New name"),
    );

    let state = state.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "rename" {
            return;
        }
        let new_name = entry.text().trim().to_string();
        if new_name.is_empty() || new_name.contains('/') {
            state
                .sender
                .output(VideoBrowserSidebarOutput::Notify(i18n("Invalid name")))
                .ok();
            return;
        }
        let dest = parent.join(&new_name);
        if dest.exists() {
            state
                .sender
                .output(VideoBrowserSidebarOutput::Notify(i18n(
                    "A file with that name already exists",
                )))
                .ok();
            return;
        }
        match std::fs::rename(&old, &dest) {
            Ok(()) => state.refresh_current_folder(),
            Err(_) => {
                state
                    .sender
                    .output(VideoBrowserSidebarOutput::Notify(i18n("Rename failed")))
                    .ok();
            }
        }
    });
    dialog.present(Some(&window));
}

/// Move one file into `dest_dir`, keeping its basename. Refuses to overwrite an
/// existing destination; falls back to copy+remove across filesystems.
fn move_file(src: &str, dest_dir: &Path) -> std::io::Result<()> {
    let src_path = Path::new(src);
    let name = src_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let dest = dest_dir.join(name);
    if dest == src_path {
        return Ok(());
    }
    if dest.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    if std::fs::rename(src_path, &dest).is_ok() {
        return Ok(());
    }
    // Cross-device → copy then remove the source.
    std::fs::copy(src_path, &dest)?;
    std::fs::remove_file(src_path)
}

/// Emit a result toast for a batch file op.
fn notify_op_result(state: &Rc<BrowserState>, ok: usize, failed: usize, verb: &str) {
    let msg = if failed == 0 {
        ni18n("{} file {verb}", "{} files {verb}", ok as u64)
            .replace("{}", &ok.to_string())
            .replace("{verb}", verb)
    } else {
        i18n("{ok} {verb}, {failed} failed")
            .replace("{ok}", &ok.to_string())
            .replace("{verb}", verb)
            .replace("{failed}", &failed.to_string())
    };
    state
        .sender
        .output(VideoBrowserSidebarOutput::Notify(msg))
        .ok();
}

impl BrowserState {
    /// Re-scan the browsed folder after a filesystem change (move / rename).
    pub(super) fn refresh_current_folder(self: &Rc<Self>) {
        let folder = self.folder.borrow().clone();
        if let Some(folder) = folder {
            self.navigate_to_folder(&folder);
        } else {
            self.rebuild();
        }
    }
}
