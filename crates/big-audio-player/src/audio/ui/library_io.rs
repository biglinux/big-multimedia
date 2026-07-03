//! Library-panel file IO + folder-management dialogs (push-model).
//!
//! Free functions the panel calls when the user triggers export/import/save-as
//! or opens the watch-folder manager. The panel NEVER touches the DB or the
//! scanner: the m3u8 export writes the file off-thread (it already holds the
//! detail tracks), but every DB-backed action emits an `Output` intent and the
//! shell runs the query/scan. `parent` is the toplevel window — it satisfies
//! both `FilePicker` (`IsA<gtk::Window>`) and `AlertDialog::present`.

use adw::prelude::*;
use gtk::glib;

use big_app_kit::file_dialogs::FilePicker;
use big_media_components::player_chrome::spawn_blocking_result;
use big_relm4_components::input::button_content::{build_button, BigButtonContentSpec};
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};
use relm4::ComponentSender;

use crate::db::models::{Track, WatchFolder};
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};

const MUSIC_FOLDERS_DIALOG_WIDTH: i32 = 600;
const MUSIC_FOLDERS_DIALOG_HEIGHT: i32 = 560;
const MUSIC_FOLDERS_CONTENT_WIDTH: i32 = 560;
const MUSIC_FOLDERS_TIGHTENING_WIDTH: i32 = 420;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NameEntryDialogSpec {
    content_width: i32,
    content_height: i32,
    content_spacing: i32,
    entry_title: &'static str,
    cancel_label: &'static str,
    save_label: &'static str,
    focus_select_start: i32,
    focus_select_end: i32,
}

fn name_entry_dialog_spec() -> NameEntryDialogSpec {
    NameEntryDialogSpec {
        content_width: 420,
        content_height: 240,
        content_spacing: 12,
        entry_title: "Playlist name",
        cancel_label: "Cancel",
        save_label: "Save",
        focus_select_start: 0,
        focus_select_end: -1,
    }
}

fn normalized_playlist_name(name: &str) -> Option<String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        None
    } else {
        Some(trimmed_name.to_string())
    }
}

/// Export the current detail tracks as an `.m3u8` playlist. The save picker
/// runs on the main loop; the `#EXTM3U`/`#EXTINF` body is built and written
/// off-thread via `spawn_blocking_result` (the panel already owns the tracks,
/// so no DB round-trip is needed). Success/failure surfaces through a `Notify`
/// intent — the shell owns the toast surface.
pub(super) fn export_detail_m3u8(
    parent: &gtk::Window,
    tracks: Vec<Track>,
    sender: &ComponentSender<LibraryPanel>,
) {
    if tracks.is_empty() {
        let _ = sender.output(LibraryPanelOutput::Notify(i18n("No tracks to export")));
        return;
    }
    let out = sender.clone();
    FilePicker::save_file(i18n("Export Playlist"))
        .accept_label(i18n("Save"))
        .initial_name("playlist.m3u8")
        .pattern_filter("M3U8 Playlist", &["*.m3u8", "*.m3u"])
        .save(parent, move |path| {
            spawn_blocking_result(
                move || std::fs::write(&path, render_m3u8(&tracks)).is_ok(),
                move |ok| {
                    let notification_message = if ok {
                        i18n("Playlist exported")
                    } else {
                        i18n("Export failed")
                    };
                    let _ = out.output(LibraryPanelOutput::Notify(notification_message));
                },
            );
        });
}

/// Render the `#EXTM3U` body for `tracks` (pure; off-thread caller).
pub(crate) fn render_m3u8(tracks: &[Track]) -> String {
    let mut content = String::from("#EXTM3U\n");
    for t in tracks {
        let secs = t.duration_secs as i64;
        let artist = t.display_artist();
        let title = t.display_title();
        content.push_str(&format!("#EXTINF:{secs},{artist} - {title}\n"));
        content.push_str(&t.file_path);
        content.push('\n');
    }
    content
}

/// Pick an `.m3u`/`.m3u8`/`.pls` file and hand the path to the shell, which
/// parses it (`big_media_runtime::media::playlist_scan`) + inserts the tracks.
pub(super) fn import_playlist(parent: &gtk::Window, sender: &ComponentSender<LibraryPanel>) {
    let out = sender.clone();
    FilePicker::new(i18n("Import Playlist"))
        .accept_label(i18n("Open"))
        .pattern_filter("Playlists (m3u, m3u8, pls)", &["*.m3u", "*.m3u8", "*.pls"])
        .open(parent, move |path| {
            let _ = out.output(LibraryPanelOutput::ImportPlaylistFile { path });
        });
}

/// Prompt for a name, then ask the shell to persist `track_ids` as a new
/// playlist (the shell owns `create_playlist` + `add_track_to_playlist`).
pub(super) fn save_tracks_as_playlist(
    parent: &gtk::Window,
    track_ids: Vec<i64>,
    sender: &ComponentSender<LibraryPanel>,
) {
    if track_ids.is_empty() {
        let _ = sender.output(LibraryPanelOutput::Notify(i18n("No tracks to save")));
        return;
    }
    let out = sender.clone();
    present_name_entry_dialog(
        parent,
        &i18n("Save as Playlist"),
        &i18n("Enter a name for the new playlist"),
        move |name| {
            let _ = out.output(LibraryPanelOutput::SaveTracksAsPlaylist {
                name,
                track_ids: track_ids.clone(),
            });
        },
    );
}

/// Present the "Manage Music Folders" dialog: lists the shell-supplied watch
/// folders, an add-folder picker, a remove button per row, and a scan-now
/// button. Add/remove/scan emit `Output` intents; the shell owns the DB +
/// scanner and pushes the refreshed `folders` back via a re-open.
pub(super) fn show_manage_folders(
    parent: &gtk::Window,
    folders: &[WatchFolder],
    sender: &ComponentSender<LibraryPanel>,
) {
    let page = BigDialogPage::scrolled_clamped(
        MUSIC_FOLDERS_CONTENT_WIDTH,
        MUSIC_FOLDERS_TIGHTENING_WIDTH,
    );
    page.root()
        .update_property(&[gtk::accessible::Property::Label(&i18n("Music Folders"))]);

    let content = page.content();
    content.set_spacing(14);
    content.append(&build_intro_label());
    content.append(&build_folders_card(parent, folders, sender));
    content.append(&build_tips_card());

    let dialog = adw::Dialog::builder()
        .title(i18n("Music Folders"))
        .content_width(MUSIC_FOLDERS_DIALOG_WIDTH)
        .content_height(MUSIC_FOLDERS_DIALOG_HEIGHT)
        .build();
    dialog.set_child(Some(&page.into_root()));
    dialog.present(Some(parent));
}

/// The framed "Monitored Folders" card: header, description, the folder list,
/// and the add/scan button strip.
fn build_folders_card(
    parent: &gtk::Window,
    folders: &[WatchFolder],
    sender: &ComponentSender<LibraryPanel>,
) -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    card.add_css_class("card");

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let icon = gtk::Image::from_icon_name("folder-music-symbolic");
    icon.set_pixel_size(24);
    icon.set_valign(gtk::Align::Center);
    header.append(&icon);
    let title = gtk::Label::builder()
        .label(i18n("Monitored Folders"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.add_css_class("heading");
    header.append(&title);
    inner.append(&header);

    let desc = gtk::Label::builder()
        .label(i18n(
            "These folders are scanned for audio files. \
             Removing a folder also removes its tracks from the library.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    desc.add_css_class("dim-label");
    inner.append(&desc);

    inner.append(&build_folders_list(folders, sender));
    inner.append(&build_folder_buttons(parent, sender));

    card.append(&inner);
    card
}

/// The boxed-list of watch folders (or an empty-state row); each row's remove
/// button emits `RemoveWatchFolder`.
fn build_folders_list(
    folders: &[WatchFolder],
    sender: &ComponentSender<LibraryPanel>,
) -> gtk::ListBox {
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list".to_string()])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(&i18n("Music folders"))]);

    if folders.is_empty() {
        let empty = BigDetailRow::new(
            BigDetailRowSpec::new(i18n("No folders added yet"))
                .subtitle(i18n(
                    "Click the button below to add your first music folder",
                ))
                .prefix_icon_name("folder-new-symbolic"),
        )
        .into_root();
        empty.add_css_class("dim-label");
        list.append(&empty);
        return list;
    }

    for folder in folders {
        let folder_label = abbreviate_home(&folder.path);
        let strip = BigButtonStripRow::new(
            BigButtonStripRowSpec::new(&folder_label).prefix_icon_name("folder-symbolic"),
        );
        let remove_folder_button = build_remove_button(&folder_label);
        let folder_id = folder.id;
        let out = sender.clone();
        let list_weak = list.downgrade();
        remove_folder_button.connect_clicked(move |button| {
            let _ = out.output(LibraryPanelOutput::RemoveWatchFolder { id: folder_id });
            remove_row_from_list(button, &list_weak);
        });
        strip.append(&remove_folder_button);
        list.append(&strip.into_root());
    }
    list
}

/// The add-folder + scan-now button row under the folder list.
fn build_folder_buttons(parent: &gtk::Window, sender: &ComponentSender<LibraryPanel>) -> gtk::Box {
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .margin_top(4)
        .build();

    let add_folder_button = build_button(
        &BigButtonContentSpec::new("folder-new-symbolic", i18n("Add Folder"))
            .accessible_label(i18n("Add music folder"))
            .css_classes(["suggested-action", "pill"]),
    );
    add_folder_button.set_halign(gtk::Align::Start);
    let out_add = sender.clone();
    let parent_weak = parent.downgrade();
    add_folder_button.connect_clicked(move |_| {
        let Some(window) = parent_weak.upgrade() else {
            return;
        };
        let out = out_add.clone();
        FilePicker::open_folder(i18n("Select Music Folder")).select_folder(&window, move |path| {
            let _ = out.output(LibraryPanelOutput::AddWatchFolder { path });
        });
    });
    buttons.append(&add_folder_button);

    let scan_now_button = build_button(
        &BigButtonContentSpec::new("view-refresh-symbolic", i18n("Scan Now"))
            .accessible_description(i18n("Scan all monitored folders now"))
            .css_class("pill"),
    );
    let out_scan = sender.clone();
    scan_now_button.connect_clicked(move |button| {
        button.set_sensitive(false);
        let _ = out_scan.output(LibraryPanelOutput::ScanNow);
        let scan_now_button_weak = button.downgrade();
        glib::timeout_add_seconds_local_once(3, move || {
            if let Some(button) = scan_now_button_weak.upgrade() {
                button.set_sensitive(true);
            }
        });
    });
    buttons.append(&scan_now_button);
    buttons
}

/// Optimistic removal of the clicked row's `ActionRow` from `list` so the
/// dialog updates immediately (the shell deletes the row from the DB).
fn remove_row_from_list(clicked_button: &gtk::Button, list_weak: &glib::WeakRef<gtk::ListBox>) {
    let Some(list) = list_weak.upgrade() else {
        return;
    };
    if let Some(row) = clicked_button.ancestor(adw::ActionRow::static_type()) {
        if let Some(row) = row.downcast_ref::<adw::ActionRow>() {
            list.remove(row);
        }
    }
}

/// A small shared-chrome dialog with a single name entry; calls `on_save` with
/// the trimmed name when the user confirms.
fn present_name_entry_dialog<F>(parent: &gtk::Window, heading: &str, body: &str, on_save: F)
where
    F: Fn(String) + 'static,
{
    let dialog_spec = name_entry_dialog_spec();
    let dialog = adw::Dialog::builder()
        .title(heading)
        .content_width(dialog_spec.content_width)
        .content_height(dialog_spec.content_height)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(dialog_spec.content_spacing);
    page.append_description(body);

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(i18n(dialog_spec.entry_title))
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(
        i18n(dialog_spec.cancel_label),
        i18n(dialog_spec.save_label),
    ));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    entry.connect_changed(move |row| {
        accept_button.set_sensitive(normalized_playlist_name(&row.text()).is_some());
    });

    let accept_button = footer.accept_button().clone();
    entry.connect_apply(move |_| {
        if accept_button.is_sensitive() {
            accept_button.emit_clicked();
        }
    });

    let entry_for_accept = entry.clone();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        let Some(name) = normalized_playlist_name(&entry_for_accept.text()) else {
            return;
        };
        on_save(name);
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let entry_for_focus = entry.clone();
    dialog.connect_map(move |_| {
        let entry_for_idle = entry_for_focus.clone();
        glib::idle_add_local_once(move || {
            entry_for_idle.grab_focus();
            entry_for_idle
                .select_region(dialog_spec.focus_select_start, dialog_spec.focus_select_end);
        });
    });
    dialog.present(Some(parent));
}

/// Rewrite a `$HOME`-prefixed path to a `~`-abbreviated display form.
fn abbreviate_home(path: &str) -> String {
    let home = glib::home_dir();
    let home_str = home.to_string_lossy();
    if let Some(rest) = path.strip_prefix(home_str.as_ref()) {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

/// Error-styled circular trash button used for each folder row.
fn build_remove_button(folder_label: &str) -> gtk::Button {
    let remove_folder_button = gtk::Button::builder()
        .icon_name("user-trash-symbolic")
        .valign(gtk::Align::Center)
        .css_classes([
            "flat".to_string(),
            "circular".to_string(),
            "error".to_string(),
        ])
        .build();
    big_relm4_components::feedback::tooltip::set(
        &remove_folder_button,
        &i18n("Remove folder and its tracks"),
    );
    remove_folder_button.update_property(&[gtk::accessible::Property::Label(&format!(
        "{}: {folder_label}",
        i18n("Remove folder")
    ))]);
    remove_folder_button
}

/// The dialog's lead-in paragraph.
fn build_intro_label() -> gtk::Label {
    let intro = gtk::Label::builder()
        .label(i18n(
            "Add folders where your music files are stored. \
             The player scans these folders to build your library, \
             organizing tracks by artist, album, and genre automatically.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    intro.add_css_class("dim-label");
    intro.set_margin_bottom(4);
    intro
}

/// The framed "Tips" card at the bottom of the dialog.
fn build_tips_card() -> gtk::Box {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    card.add_css_class("card");

    let inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let icon = gtk::Image::from_icon_name("dialog-information-symbolic");
    icon.set_pixel_size(24);
    icon.set_valign(gtk::Align::Center);
    header.append(&icon);
    let title = gtk::Label::builder()
        .label(i18n("Tips"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    title.add_css_class("heading");
    header.append(&title);
    inner.append(&header);

    for text in [
        i18n("• Supported formats: MP3, FLAC, OGG, Opus, M4A, WAV, WMA, AAC, APE, WavPack"),
        i18n("• Subfolders are included automatically — no need to add each one"),
        i18n(
            "• Use the Files tab to play files from any folder without adding them to the library",
        ),
    ] {
        let tip = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .build();
        tip.add_css_class("dim-label");
        inner.append(&tip);
    }

    card.append(&inner);
    card
}

#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    use super::abbreviate_home;
    use super::render_m3u8;
    use super::{name_entry_dialog_spec, normalized_playlist_name, NameEntryDialogSpec};
    use crate::db::models::Track;

    #[cfg(not(miri))]
    #[test]
    fn abbreviate_home_rewrites_prefix() {
        let home = gtk::glib::home_dir();
        let input = home.join("Music/Albums");
        assert_eq!(
            abbreviate_home(&input.to_string_lossy()),
            "~/Music/Albums".to_string()
        );
    }

    #[cfg(not(miri))]
    #[test]
    fn abbreviate_home_keeps_external_path() {
        assert_eq!(
            abbreviate_home("/mnt/media/lib"),
            "/mnt/media/lib".to_string()
        );
    }

    #[test]
    fn render_m3u8_writes_extinf_and_path() {
        let mut t = Track::new_external_path_with_title("/m/a.flac", Some("A".to_string()));
        t.artist = Some("Band".to_string());
        t.duration_secs = 12.7;
        let out = render_m3u8(&[t]);
        assert!(out.starts_with("#EXTM3U\n"));
        assert!(out.contains("#EXTINF:12,Band - A\n"));
        assert!(out.contains("/m/a.flac\n"));
    }

    #[test]
    fn name_entry_dialog_spec_keeps_playlist_save_chrome_and_focus_selection() {
        assert_eq!(
            name_entry_dialog_spec(),
            NameEntryDialogSpec {
                content_width: 420,
                content_height: 240,
                content_spacing: 12,
                entry_title: "Playlist name",
                cancel_label: "Cancel",
                save_label: "Save",
                focus_select_start: 0,
                focus_select_end: -1,
            }
        );
    }

    #[test]
    fn normalized_playlist_name_requires_non_blank_trimmed_text() {
        assert_eq!(normalized_playlist_name(""), None);
        assert_eq!(normalized_playlist_name("   "), None);
        assert_eq!(
            normalized_playlist_name("  Favorites  "),
            Some("Favorites".to_string())
        );
    }
}
