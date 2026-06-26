//! Window wiring helpers for [`ConverterShell`](super::ConverterShell):
//! keyboard shortcuts, file-drop target, recursive media collection, and the
//! queue-item adapter. Split out of `shell.rs` (recipe: file-size budget); pure
//! move, behavior identical. Cross-module helpers are `pub(super)`.

use std::path::{Path, PathBuf};

use adw::prelude::*;
use relm4::ComponentSender;

use big_media_components::converter::queue::BigConversionQueueItem;

use crate::converter::app::ConverterMode;

use super::{ConverterShell, ShellMsg};

pub(super) fn to_queue_item(item: &crate::converter::queue::QueueItem) -> BigConversionQueueItem {
    BigConversionQueueItem {
        id: item.id,
        input: item.input.clone(),
        duration_secs: item.duration_secs,
    }
}

pub(super) fn install_shortcuts(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<ConverterShell>,
    mode: ConverterMode,
) {
    use big_app_kit::desktop::install_action;
    let Some(app) = window.application() else {
        return;
    };

    let s = sender.clone();
    install_action(window, "add-files", move || {
        s.input(ShellMsg::OpenFileChooser);
    });
    app.set_accels_for_action("win.add-files", &["<Control>o"]);

    let s = sender.clone();
    install_action(window, "add-folder", move || {
        s.input(ShellMsg::OpenFolderChooser);
    });
    app.set_accels_for_action("win.add-folder", &["<Control><Shift>o"]);

    let s = sender.clone();
    install_action(window, "convert", move || {
        s.input(ShellMsg::StartConversion);
    });
    app.set_accels_for_action("win.convert", &["<Control>Return"]);

    let s = sender.clone();
    install_action(window, "clear-queue", move || {
        s.input(ShellMsg::ClearQueueRequested);
    });
    app.set_accels_for_action("win.clear-queue", &["<Control>Delete"]);

    if mode.is_audio() {
        let s = sender.clone();
        install_action(window, "play-pause", move || {
            s.input(ShellMsg::AudioTogglePlayPause);
        });
        app.set_accels_for_action("win.play-pause", &["space"]);
    }
}

pub(super) fn install_file_drop(
    window: &adw::ApplicationWindow,
    sender: &ComponentSender<ConverterShell>,
) {
    let s = sender.clone();
    big_app_kit::widgets::add_file_drop_target(window, move |raw_paths| {
        // Accept the drop immediately and run the recursive directory walk +
        // per-file validation off the main loop (a deep or network-mounted
        // folder would otherwise freeze the UI). Files are added when ready.
        let s = s.clone();
        big_relm4_components::task::spawn_blocking_result(
            move || {
                big_app_kit::widgets::expand_drop_paths(raw_paths, |path| {
                    let mut collected = Vec::new();
                    collect_media_files(path, &mut collected);
                    collected
                })
            },
            move |paths| {
                if !paths.is_empty() {
                    s.input(ShellMsg::AddFiles(paths));
                }
            },
        );
        true
    });
}

pub(super) fn collect_media_files(dir: &Path, out: &mut Vec<PathBuf>) {
    collect_media_files_depth(dir, out, 10);
}

fn collect_media_files_depth(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_media_files_depth(&path, out, depth - 1);
        } else if path.is_file()
            && matches!(
                crate::converter::file_validator::validate_file(&path),
                crate::converter::file_validator::FileCheck::Video
                    | crate::converter::file_validator::FileCheck::AudioOnly
            )
        {
            out.push(path);
        }
    }
}
