//! Triage file operations for Files mode: the multi-select batch action bar and
//! the move / rename helpers. Destructive or launch-style actions (send to
//! converter, trash) are emitted to the shell (which owns messages + process
//! spawning); move and rename are filesystem ops done here, followed by a
//! folder re-scan. All act on the current `selection` (batch) or a single path.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_os_kit::storage::{StagedFile, rename_no_replace};
use big_relm4_components::layout::name_prompt::{BigNamePromptSpec, prompt_name_with_reply};

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
    done.set_tooltip_text(Some(&i18n("Done")));
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
    action_button.set_tooltip_text(Some(label));
    action_button.update_property(&[gtk::accessible::Property::Label(label)]);
    action_button.connect_clicked({
        let state = state.clone();
        move |_| {
            // In listing order: the selection set has none.
            let selected_paths: Vec<String> = {
                let selection = state.selection.borrow();
                state
                    .entries
                    .borrow()
                    .iter()
                    .filter(|path| selection.contains(*path))
                    .cloned()
                    .collect()
            };
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
        let drain = big_os_kit::background::track_operation();
        big_media_components::player_chrome::spawn_blocking_result(
            move || {
                let _drain = drain;
                let mut moved = Vec::new();
                let mut failed = 0_usize;
                for src in &paths {
                    match move_file(src, &dest) {
                        Ok(to) => moved.push((src.clone(), to.to_string_lossy().into_owned())),
                        Err(_) => failed += 1,
                    }
                }
                (moved, failed)
            },
            move |(moved, failed): (Vec<(String, String)>, usize)| {
                notify_op_result(&state, moved.len(), failed, &i18n("moved"));
                if !moved.is_empty() {
                    let _ = state
                        .sender
                        .output(VideoBrowserSidebarOutput::Renamed(moved));
                }
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

    let spec = BigNamePromptSpec {
        title: i18n("Rename"),
        description: Some(i18n("Choose a new file name.")),
        entry_title: i18n("New name"),
        prefill: current_name,
        accept_label: i18n("Rename"),
        content_width: 460,
        content_height: 240,
    };
    let state = Rc::downgrade(state);
    prompt_name_with_reply(
        Some(window.upcast_ref()),
        &spec,
        accepted_rename_name,
        move |new_name, reply| {
            let state = state.clone();
            let old = old.clone();
            let dest = parent.join(&new_name);
            // Where the filesystem cannot refuse a replacing rename, this copies.
            let drain = big_os_kit::background::track_operation();
            big_media_components::player_chrome::spawn_blocking_result(
                move || {
                    let _drain = drain;
                    let result = move_no_replace(&old, &dest);
                    (old, dest, result)
                },
                move |(old, dest, result)| {
                    let Some(state) = state.upgrade() else {
                        reply.close();
                        return;
                    };
                    match result {
                        Ok(()) => {
                            let renamed = (
                                old.to_string_lossy().into_owned(),
                                dest.to_string_lossy().into_owned(),
                            );
                            state
                                .sender
                                .output(VideoBrowserSidebarOutput::Renamed(vec![renamed]))
                                .ok();
                            state.refresh_current_folder();
                            reply.close();
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
                },
            );
        },
    );
}

fn accepted_rename_name(input: &str) -> Option<String> {
    let name = input.trim();
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return None;
    }
    Some(name.to_string())
}

/// Move `src` into `dest_dir`, keeping its name and never replacing a file
/// there; the path it has now.
fn move_file(src: &str, dest_dir: &Path) -> std::io::Result<PathBuf> {
    let src_path = Path::new(src);
    let name = src_path
        .file_name()
        .ok_or_else(|| std::io::Error::other("no file name"))?;
    let dest = dest_dir.join(name);
    if dest == src_path {
        return Ok(dest);
    }
    move_no_replace(src_path, &dest)?;
    Ok(dest)
}

/// Rename `src` to `dest`, failing with `AlreadyExists` instead of replacing
/// it. Copies across filesystems, and where the filesystem cannot refuse a
/// replacing rename (`EINVAL` on some FUSE and NFS mounts).
fn move_no_replace(src: &Path, dest: &Path) -> std::io::Result<()> {
    match rename_no_replace(src, dest) {
        Err(error) if matches!(error.raw_os_error(), Some(libc::EXDEV | libc::EINVAL)) => {
            move_by_copy(src, dest)
        }
        result => result,
    }
}

/// Copy `src` to the absent `dest` through staging that a killed copy leaves
/// out of sight, and remove `src` only once the copy and its directory entry
/// are on disk.
fn move_by_copy(src: &Path, dest: &Path) -> std::io::Result<()> {
    // Copying a large video takes minutes; refuse a taken name before it.
    if dest.symlink_metadata().is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    let stage = StagedFile::new(dest)?;
    let mut staged = std::fs::OpenOptions::new().write(true).open(stage.path())?;
    std::io::copy(&mut std::fs::File::open(src)?, &mut staged)?;
    drop(staged);
    stage.publish(dest)?.into_io_result()?;
    std::fs::remove_file(src)
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
    use std::io::Write;
    use std::path::{Path, PathBuf};

    use super::{accepted_rename_name, move_by_copy, move_file, move_no_replace};

    fn test_directory(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bvp-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry_names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A killed copy must leave nothing under the final name: the source is
    /// a FIFO, so the copy is held open while the destination is inspected.
    #[test]
    fn cross_device_move_hides_partial_copies() {
        let dir = test_directory("cross-device");
        let (src, dest) = (dir.join("in.mp4"), dir.join("out").join("in.mp4"));
        std::fs::create_dir(dir.join("out")).unwrap();
        let fifo = std::ffi::CString::new(src.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `fifo` is a NUL-terminated path bound for the whole call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let mover = std::thread::spawn({
            let (src, dest) = (src.clone(), dest.clone());
            move || move_by_copy(&src, &dest)
        });
        let mut writer = std::fs::OpenOptions::new().write(true).open(&src).unwrap();
        assert!(
            !dest.exists(),
            "a partial copy is visible under the final name"
        );
        writer.write_all(b"video").unwrap();
        drop(writer);
        mover.join().unwrap().unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"video");
        assert!(!src.exists());
        assert_eq!(entry_names(&dir.join("out")), ["in.mp4"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cross_device_move_keeps_an_existing_destination() {
        let dir = test_directory("cross-device-taken");
        let (src, dest) = (dir.join("in.mp4"), dir.join("out").join("in.mp4"));
        std::fs::create_dir(dir.join("out")).unwrap();
        std::fs::write(&src, b"new").unwrap();
        std::fs::write(&dest, b"kept").unwrap();
        let error = move_by_copy(&src, &dest).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&src).unwrap(), b"new");
        assert_eq!(std::fs::read(&dest).unwrap(), b"kept");
        assert_eq!(entry_names(&dir.join("out")), ["in.mp4"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rename_and_move_keep_existing_files() {
        let dir = std::env::temp_dir().join(format!("bvp-noreplace-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let (a, b) = (dir.join("a.mp4"), dir.join("b.mp4"));
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(&b, b"b").unwrap();
        std::fs::write(sub.join("a.mp4"), b"kept").unwrap();

        let err = move_no_replace(&a, &b).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(move_file(a.to_str().unwrap(), &sub).is_err());
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert_eq!(std::fs::read(sub.join("a.mp4")).unwrap(), b"kept");

        move_no_replace(&a, &dir.join("c.mp4")).unwrap();
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
    fn rename_rejects_empty_and_traversal() {
        for input in ["", "   ", ".", "..", "nested/name.mp4", "bad\0name.mp4"] {
            assert_eq!(accepted_rename_name(input), None);
        }
    }
}
