//! Triage file operations for Files mode: the multi-select batch action bar and
//! the move / rename helpers. Destructive or launch-style actions (send to
//! converter, trash) are emitted to the shell (which owns messages + process
//! spawning); move and rename are filesystem ops done here, followed by a
//! folder re-scan. All act on the current `selection` (batch) or a single path.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

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
    big_relm4_components::feedback::tooltip::set(&done, i18n("Done"));
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
    let action_button = gtk::Button::builder()
        .icon_name(icon)
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&action_button, label);
    action_button.update_property(&[gtk::accessible::Property::Label(label)]);
    action_button.connect_clicked({
        let state = state.clone();
        move |_| {
            let selected_paths: Vec<String> = state.selection.borrow().iter().cloned().collect();
            if selected_paths.is_empty() {
                return;
            }
            action(&state, selected_paths);
            state.set_select_mode(false);
        }
    });
    action_button
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

    let dialog = adw::Dialog::builder()
        .title(i18n("Rename"))
        .content_width(460)
        .content_height(240)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&i18n("Choose a new file name."));

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(i18n("New name"))
        .text(&current_name)
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Rename")));
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    accept_button.set_sensitive(accepted_rename_name(&entry.text()).is_some());
    entry.connect_changed({
        let accept_button = accept_button.clone();
        move |row| {
            accept_button.set_sensitive(accepted_rename_name(&row.text()).is_some());
        }
    });
    entry.connect_apply(move |_| {
        if accept_button.is_sensitive() {
            accept_button.emit_clicked();
        }
    });

    let dialog_ref = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_ref.upgrade() {
            dialog.close();
        }
    });

    let state = state.clone();
    let dialog_ref = dialog.downgrade();
    let entry_for_accept = entry.clone();
    footer.accept_button().connect_clicked(move |_| {
        let Some(new_name) = accepted_rename_name(&entry_for_accept.text()) else {
            state
                .sender
                .output(VideoBrowserSidebarOutput::Notify(i18n("Invalid name")))
                .ok();
            return;
        };
        let dest = parent.join(&new_name);
        match rename_no_replace(&old, &dest) {
            Ok(()) => {
                state.refresh_current_folder();
                if let Some(dialog) = dialog_ref.upgrade() {
                    dialog.close();
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                state
                    .sender
                    .output(VideoBrowserSidebarOutput::Notify(i18n(
                        "A file with that name already exists",
                    )))
                    .ok();
            }
            Err(_) => {
                state
                    .sender
                    .output(VideoBrowserSidebarOutput::Notify(i18n("Rename failed")))
                    .ok();
            }
        }
    });
    let entry_for_idle = entry.clone();
    dialog.connect_map(move |_| {
        let entry = entry_for_idle.clone();
        gtk::glib::idle_add_local_once(move || {
            entry.grab_focus();
            entry.select_region(0, -1);
        });
    });
    dialog.present(Some(&window));
}

fn accepted_rename_name(input: &str) -> Option<String> {
    let name = input.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return None;
    }
    Some(name.to_string())
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
    match rename_no_replace(src_path, &dest) {
        Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {}
        result => return result,
    }
    // Cross-device → copy then remove the source. `create_new` refuses a file
    // that appeared meanwhile; a failed copy (e.g. disk full) must not leave a
    // truncated file behind.
    let mut target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dest)?;
    let copied = std::fs::File::open(src_path)
        .and_then(|mut source| std::io::copy(&mut source, &mut target))
        .and_then(|_| target.sync_all());
    if let Err(error) = copied {
        let _ = std::fs::remove_file(&dest);
        return Err(error);
    }
    std::fs::remove_file(src_path)
}

/// `rename` that fails with `AlreadyExists` instead of replacing `dest`, so a
/// file created between a check and the rename is never overwritten.
fn rename_no_replace(src: &Path, dest: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let src_c = std::ffi::CString::new(src.as_os_str().as_bytes())?;
    let dest_c = std::ffi::CString::new(dest.as_os_str().as_bytes())?;
    // SAFETY: both arguments are valid NUL-terminated paths that outlive the
    // call; renameat2 only reads them.
    let status = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            src_c.as_ptr(),
            libc::AT_FDCWD,
            dest_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if status == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // Some filesystems (NFS, some FUSE) lack RENAME_NOREPLACE: keep the old
    // check-then-rename there rather than failing every rename.
    if error.raw_os_error() == Some(libc::EINVAL) {
        if dest.exists() {
            return Err(std::io::ErrorKind::AlreadyExists.into());
        }
        return std::fs::rename(src, dest);
    }
    Err(error)
}

/// Say when some of a batch file op failed; the folder view already shows
/// what moved.
fn notify_op_result(state: &Rc<BrowserState>, ok: usize, failed: usize, verb: &str) {
    if failed == 0 {
        return;
    }
    let notification_message = i18n("{ok} {verb}, {failed} failed")
        .replace("{ok}", &ok.to_string())
        .replace("{verb}", verb)
        .replace("{failed}", &failed.to_string());
    state
        .sender
        .output(VideoBrowserSidebarOutput::Notify(notification_message))
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

#[cfg(test)]
mod tests {
    use super::{accepted_rename_name, move_file, rename_no_replace};

    #[test]
    fn rename_and_move_never_replace_an_existing_file() {
        let dir = std::env::temp_dir().join(format!("bvp-noreplace-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let (a, b) = (dir.join("a.mp4"), dir.join("b.mp4"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        std::fs::write(sub.join("a.mp4"), b"kept").unwrap();

        let err = rename_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(move_file(a.to_str().unwrap(), &sub).is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(std::fs::read(sub.join("a.mp4")).unwrap(), b"kept");

        rename_no_replace(&a, &dir.join("c.mp4")).unwrap();
        assert!(!a.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_name_accepts_trimmed_file_names() {
        assert_eq!(
            accepted_rename_name("  renamed video.mp4  ").as_deref(),
            Some("renamed video.mp4")
        );
    }

    #[test]
    fn rename_name_rejects_empty_paths_and_traversal_names() {
        for input in ["", "   ", ".", "..", "nested/name.mp4", "bad\0name.mp4"] {
            assert_eq!(accepted_rename_name(input), None);
        }
    }
}
