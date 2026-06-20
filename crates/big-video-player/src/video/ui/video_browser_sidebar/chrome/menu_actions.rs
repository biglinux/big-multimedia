use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::desktop::install_action;
use big_app_kit::file_dialogs::FilePicker;
use gtk::{gio, glib};

use super::super::{BrowserState, VideoBrowserSidebarOutput};
use crate::i18n::i18n;
use crate::media::playlist;
use crate::settings::keys;

/// Register the stateful + plain actions backing the overflow menu.
pub(crate) fn install_menu_actions(state: &Rc<BrowserState>, group: &gio::SimpleActionGroup) {
    use glib::prelude::ToVariant;

    // Shuffle — boolean toggle (persist only; does not reorder the listing).
    let shuffle = gio::SimpleAction::new_stateful(
        "shuffle",
        None,
        &state.settings.get_bool(keys::PLAYLIST_SHUFFLE).to_variant(),
    );
    shuffle.connect_activate({
        let state = state.clone();
        move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            state.settings.put_bool(keys::PLAYLIST_SHUFFLE, on);
        }
    });
    group.add_action(&shuffle);

    // Repeat — radio (no / file / playlist).
    let repeat = gio::SimpleAction::new_stateful(
        "loop",
        Some(glib::VariantTy::STRING),
        &state.settings.get_string(keys::LOOP_MODE).to_variant(),
    );
    repeat.connect_activate({
        let state = state.clone();
        move |action, target| {
            if let Some(mode) = target.and_then(glib::Variant::get::<String>) {
                action.set_state(&mode.to_variant());
                state.settings.put_string(keys::LOOP_MODE, &mode);
            }
        }
    });
    group.add_action(&repeat);

    // Include subfolders — boolean toggle; re-scans the current folder.
    let recursive = gio::SimpleAction::new_stateful(
        "recursive",
        None,
        &state
            .settings
            .get_bool(keys::PLAYLIST_FOLDER_RECURSIVE)
            .to_variant(),
    );
    recursive.connect_activate({
        let state = state.clone();
        move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            state.settings.put_bool(keys::PLAYLIST_FOLDER_RECURSIVE, on);
            let folder = state.folder.borrow().clone();
            if let Some(folder) = folder {
                state.navigate_to_folder(&folder);
            } else {
                state.rebuild();
            }
        }
    });
    group.add_action(&recursive);

    // Sort key — radio (name / date / size).
    let sort = gio::SimpleAction::new_stateful(
        "sort",
        Some(glib::VariantTy::STRING),
        &state.settings.get_string(keys::PLAYLIST_SORT).to_variant(),
    );
    sort.connect_activate({
        let state = state.clone();
        move |action, target| {
            if let Some(key) = target.and_then(glib::Variant::get::<String>) {
                action.set_state(&key.to_variant());
                state.settings.put_string(keys::PLAYLIST_SORT, &key);
                apply_sort(&state);
            }
        }
    });
    group.add_action(&sort);

    // Reverse order — boolean toggle; re-applies the sort descending.
    let reverse = gio::SimpleAction::new_stateful(
        "reverse",
        None,
        &state
            .settings
            .get_bool(keys::PLAYLIST_SORT_DESC)
            .to_variant(),
    );
    reverse.connect_activate({
        let state = state.clone();
        move |action, _| {
            let on = !action
                .state()
                .and_then(|s| s.get::<bool>())
                .unwrap_or(false);
            action.set_state(&on.to_variant());
            state.settings.put_bool(keys::PLAYLIST_SORT_DESC, on);
            apply_sort(&state);
        }
    });
    group.add_action(&reverse);

    install_action(group, "save", {
        let state = state.clone();
        move || save_playlist(&state)
    });
    install_action(group, "load", {
        let state = state.clone();
        move || load_playlist_file(&state)
    });
    install_action(group, "clear", {
        let state = state.clone();
        move || clear_playlist(&state)
    });
}

/// Re-sort the listing per the persisted key + direction, keeping the current
/// item lit.
fn apply_sort(state: &Rc<BrowserState>) {
    let key = state.settings.get_string(keys::PLAYLIST_SORT);
    let desc = state.settings.get_bool(keys::PLAYLIST_SORT_DESC);
    resort(state, |list| playlist::sort_paths(list, &key, desc));
}

/// Apply an in-place reordering to the entries, keeping the current item lit.
fn resort(state: &Rc<BrowserState>, reorder: impl Fn(&mut Vec<String>)) {
    let mut list = state.entries.borrow().clone();
    reorder(&mut list);
    state.entries.replace(list);
    state.recompute_pos();
    state.rebuild();
}

/// Clear the play queue + the shell's mpv playlist (overflow menu action).
fn clear_playlist(state: &Rc<BrowserState>) {
    state.queue.replace(Vec::new());
    state.recompute_pos();
    state.rebuild();
    state.sender.output(VideoBrowserSidebarOutput::Clear).ok();
}

/// Save the current play queue to an `.m3u8` (overflow menu action).
fn save_playlist(state: &Rc<BrowserState>) {
    let Some(window) = state.window() else {
        return;
    };
    let state = state.clone();
    FilePicker::new(i18n("Save Playlist"))
        .initial_name("playlist.m3u8")
        .save(&window, move |path| {
            playlist::write_m3u(&path, &state.queue.borrow());
        });
}

/// Load an `.m3u`/`.m3u8` as the play queue and start it (overflow menu action).
/// The shell loads it into mpv and pushes the queue back (revealing Playlist).
fn load_playlist_file(state: &Rc<BrowserState>) {
    let Some(window) = state.window() else {
        return;
    };
    let state = state.clone();
    FilePicker::new(i18n("Load Playlist"))
        .pattern_filter(&i18n("Playlists"), &["*.m3u", "*.m3u8"])
        .open(&window, move |path| {
            let paths = playlist::parse_m3u(&path);
            if paths.is_empty() {
                return;
            }
            state
                .sender
                .output(VideoBrowserSidebarOutput::PlayFromList { paths, start: 0 })
                .ok();
        });
}
