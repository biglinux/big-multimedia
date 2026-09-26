use std::rc::Rc;

use adw::prelude::*;
use big_app_kit::file_dialogs::FilePicker;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::sidebar_header::{BigSidebarHeader, BigSidebarHeaderSpec};
use gtk::{gio, glib};

use super::super::{BrowserState, SidebarMode, VideoBrowserSidebarOutput};
use super::menu_actions::install_menu_actions;
use crate::i18n::i18n;
use crate::media::playlist;
use crate::settings::keys;

/// Mode-specific header: in Files, a "load folder" button + a files options menu
/// (subfolders / sort); in Playlist, an "add files" button + a playlist options
/// menu (shuffle / repeat / save / load / clear). Both keep the list/grid
/// toggle. The Files ⇄ Playlist switch itself lives in the main window header.
pub(crate) fn build_header(state: &Rc<BrowserState>) {
    let mode = state.mode.get();
    let title = if mode == SidebarMode::Files {
        i18n("Files")
    } else {
        i18n("Playlist")
    };
    let sidebar_header = BigSidebarHeader::new(BigSidebarHeaderSpec::new(title).css_class("flat"));
    // A card floating over the video: the window's own buttons are not
    // this header's to show (its close would quit the player).
    sidebar_header.root().set_show_start_title_buttons(false);
    sidebar_header.root().set_show_end_title_buttons(false);

    if mode == SidebarMode::Files {
        let folder_button =
            tooltip::icon_button("folder-open-symbolic", &i18n("Load folder…"), &[]);
        folder_button.connect_clicked({
            let state = state.clone();
            move |_| pick_folder(&state)
        });
        sidebar_header.pack_start(&folder_button);

        // Triage multi-select toggle (creator workflow).
        let select_mode_button = gtk::ToggleButton::builder()
            .icon_name("selection-mode-symbolic")
            .active(state.select_mode.get())
            .css_classes(["flat"])
            .build();
        tooltip::set(&select_mode_button, i18n("Select"));
        select_mode_button.update_property(&[gtk::accessible::Property::Label(&i18n("Select"))]);
        select_mode_button.connect_toggled({
            let state = state.clone();
            move |button| state.set_select_mode(button.is_active())
        });
        sidebar_header.pack_start(&select_mode_button);
    } else {
        let add_files_button = tooltip::icon_button("list-add-symbolic", &i18n("Add files…"), &[]);
        add_files_button.connect_clicked({
            let state = state.clone();
            move |_| add_files(&state)
        });
        sidebar_header.pack_start(&add_files_button);
    }

    // pack_end is right-to-left → menu rightmost, view toggle to its left.
    sidebar_header.pack_end(&build_options_menu(state, mode));
    // The list/grid toggle only makes sense for the Files browser; the Playlist
    // is always a compact ordered queue.
    if mode == SidebarMode::Files {
        sidebar_header.pack_end(&build_view_toggle(state));
    }

    if let Some(panel) = state.container() {
        panel.append(sidebar_header.root());
    }
}

/// List ⇄ grid view toggle (the one frequently-used, self-evident control kept
/// out of the overflow menu).
fn build_view_toggle(state: &Rc<BrowserState>) -> gtk::Button {
    let is_grid = state.settings.get_string(keys::PLAYLIST_VIEW) == "grid";
    let (icon, label) = if is_grid {
        ("view-list-symbolic", i18n("Show as list"))
    } else {
        ("view-grid-symbolic", i18n("Show as grid"))
    };
    let view_toggle_button = tooltip::icon_button(icon, &label, &["flat"]);
    view_toggle_button.connect_clicked({
        let state = state.clone();
        move |_| {
            let next = if state.settings.get_string(keys::PLAYLIST_VIEW) == "list" {
                "grid"
            } else {
                "list"
            };
            state.settings.put_string(keys::PLAYLIST_VIEW, next);
            state.rebuild();
        }
    });
    view_toggle_button
}

/// Open the folder picker, switch to Files mode, and navigate into the chosen
/// directory.
pub(crate) fn pick_folder(state: &Rc<BrowserState>) {
    let Some(window) = state.window() else {
        return;
    };
    let state = state.clone();
    FilePicker::new(i18n("Choose Video Folder")).select_folder(&window, move |folder| {
        state.mode.set(SidebarMode::Files);
        state.navigate_to_folder(&folder.to_string_lossy());
    });
}

/// Add picked files to the play queue (the shell appends them to mpv and pushes
/// the updated queue back, which reveals Playlist mode).
pub(crate) fn add_files(state: &Rc<BrowserState>) {
    let Some(window) = state.window() else {
        return;
    };
    let state = state.clone();
    FilePicker::new(i18n("Add Video Files"))
        .filter(&i18n("Video files"), playlist::VIDEO_EXTENSIONS)
        .open_multiple(&window, move |paths| {
            let added: Vec<String> = paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            if added.is_empty() {
                return;
            }
            state
                .sender
                .output(VideoBrowserSidebarOutput::Enqueue(added))
                .ok();
        });
}

/// Single overflow menu (hamburger) consolidating every secondary option, each
/// a *labeled* item with a checkbox/radio that reflects its persisted state.
/// Mode-specific: Files shows folder options (subfolders, sort); Playlist shows
/// playback (shuffle, repeat) + the m3u file actions — each interface only what
/// belongs to it.
fn build_options_menu(state: &Rc<BrowserState>, mode: SidebarMode) -> gtk::MenuButton {
    let group = gio::SimpleActionGroup::new();
    install_menu_actions(state, &group);
    if let Some(panel) = state.container() {
        panel.insert_action_group("pl", Some(&group));
    }

    let menu = gio::Menu::new();
    match mode {
        SidebarMode::Files => {
            let folder = gio::Menu::new();
            folder.append(Some(&i18n("Include subfolders")), Some("pl.recursive"));
            let sort = gio::Menu::new();
            let sort_keys = gio::Menu::new();
            sort_keys.append(Some(&i18n("Name")), Some("pl.sort::name"));
            sort_keys.append(Some(&i18n("Date")), Some("pl.sort::date"));
            sort_keys.append(Some(&i18n("Size")), Some("pl.sort::size"));
            sort.append_section(None, &sort_keys);
            let sort_dir = gio::Menu::new();
            sort_dir.append(Some(&i18n("Reverse order")), Some("pl.reverse"));
            sort.append_section(None, &sort_dir);
            folder.append_submenu(Some(&i18n("Sort by")), &sort);
            menu.append_section(None, &folder);
        }
        SidebarMode::Playlist => {
            let playback = gio::Menu::new();
            playback.append(Some(&i18n("Shuffle")), Some("pl.shuffle"));
            let repeat = gio::Menu::new();
            repeat.append(Some(&i18n("Off")), Some("pl.loop::no"));
            repeat.append(Some(&i18n("Current file")), Some("pl.loop::file"));
            repeat.append(Some(&i18n("Whole playlist")), Some("pl.loop::playlist"));
            playback.append_submenu(Some(&i18n("Repeat")), &repeat);
            menu.append_section(None, &playback);

            let file = gio::Menu::new();
            file.append(Some(&i18n("Save playlist…")), Some("pl.save"));
            file.append(Some(&i18n("Load playlist…")), Some("pl.load"));
            file.append(Some(&i18n("Clear playlist")), Some("pl.clear"));
            menu.append_section(None, &file);
        }
    }

    let options_menu_button =
        tooltip::icon_menu_button("view-more-symbolic", &i18n("Options"), &["flat"]);
    options_menu_button.set_menu_model(Some(&menu));
    // Repeat and shuffle can also change from Preferences or MPRIS; show the
    // stored state each time the menu opens instead of the one at build time.
    let settings = state.settings.clone();
    options_menu_button.connect_active_notify(move |button| {
        if !button.is_active() {
            return;
        }
        let set = |name: &str, value: glib::Variant| {
            if let Some(action) = group
                .lookup_action(name)
                .and_downcast::<gio::SimpleAction>()
            {
                action.set_state(&value);
            }
        };
        set("loop", settings.get_string(keys::LOOP_MODE).to_variant());
        set(
            "shuffle",
            settings.get_bool(keys::PLAYLIST_SHUFFLE).to_variant(),
        );
    });
    options_menu_button
}
