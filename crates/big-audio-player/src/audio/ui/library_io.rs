//! Library-panel file IO + folder-management dialogs (push-model).
//!
//! Free functions the panel calls when the user imports a playlist or opens
//! the watch-folder manager. The panel never touches the DB or the scanner:
//! every action emits an `Output` intent and the shell runs the query/scan.
//! `parent` is the toplevel window; it satisfies both `FilePicker`
//! (`IsA<gtk::Window>`) and `AlertDialog::present`.

use std::path::Path;

use adw::prelude::*;

use big_app_kit::file_dialogs::FilePicker;
use big_media_components::media::files::PLAYLIST_EXTENSIONS;
use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};
use relm4::ComponentSender;

use crate::db::models::WatchFolder;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};

const MUSIC_FOLDERS_DIALOG_WIDTH: i32 = 600;
const MUSIC_FOLDERS_DIALOG_HEIGHT: i32 = 560;
const MUSIC_FOLDERS_CONTENT_WIDTH: i32 = 560;
const MUSIC_FOLDERS_TIGHTENING_WIDTH: i32 = 420;

/// Pick an `.m3u`/`.m3u8`/`.pls` file and hand the path to the shell, which
/// parses it (`big_media_runtime::media::playlist_scan`) + inserts the tracks.
pub(super) fn import_playlist(parent: &gtk::Window, sender: &ComponentSender<LibraryPanel>) {
    let out = sender.clone();
    FilePicker::new(i18n("Import Playlist"))
        .accept_label(i18n("Open"))
        .filter(&i18n("Playlists"), PLAYLIST_EXTENSIONS)
        .open(parent, move |path| {
            let _ = out.output(LibraryPanelOutput::ImportPlaylistFile { path });
        });
}

/// A scan opens its own progress dialog. Stacked over this one, Escape closed
/// the progress (cancelling the scan) and left this identical-looking dialog,
/// so it read as Escape doing nothing: leave before the scan starts.
fn close_enclosing_dialog(widget: &impl IsA<gtk::Widget>) {
    if let Some(dialog) = widget
        .ancestor(adw::Dialog::static_type())
        .and_downcast::<adw::Dialog>()
    {
        dialog.close();
    }
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
    let review = gtk::Button::with_label(&i18n("Review unavailable files"));
    let out = sender.clone();
    review.connect_clicked(move |_| {
        let _ = out.output(LibraryPanelOutput::ReviewUnavailable);
    });
    content.append(&review);

    let dialog = big_gtk_kit::dialog::Dialog::builder()
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
             Removing a folder stops monitoring it. Tracks and ratings are kept until you explicitly review unavailable files.",
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
        remove_folder_button.connect_clicked(move |button| {
            let _ = out.output(LibraryPanelOutput::RemoveWatchFolder { id: folder_id });
            button.set_sensitive(false);
            button.set_tooltip_text(Some(&i18n(
                "Removal requested. Reopen this list after the operation finishes.",
            )));
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
    add_folder_button.connect_clicked(move |button| {
        let Some(window) = parent_weak.upgrade() else {
            return;
        };
        let out = out_add.clone();
        let button = button.downgrade();
        FilePicker::open_folder(i18n("Select Music Folder")).select_folder(&window, move |path| {
            if let Some(button) = button.upgrade() {
                close_enclosing_dialog(&button);
            }
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
        close_enclosing_dialog(button);
        let _ = out_scan.output(LibraryPanelOutput::ScanNow);
    });
    buttons.append(&scan_now_button);
    buttons
}

/// Rewrite a path inside the home folder to a `~`-abbreviated display form.
fn abbreviate_home(path: &str) -> String {
    let Some(home) = big_os_kit::xdg_dirs::home_dir() else {
        return path.to_string();
    };
    match Path::new(path).strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_string(),
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
    remove_folder_button
        .set_tooltip_text(Some(&i18n("Stop monitoring this folder; keep its tracks")));
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

    #[cfg(not(miri))]
    #[test]
    fn abbreviate_home_rewrites_prefix() {
        let home = big_os_kit::xdg_dirs::home_dir().expect("an absolute home folder");
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
        let home = big_os_kit::xdg_dirs::home_dir().expect("an absolute home folder");
        let sibling = format!("{}2/Music", home.display());
        assert_eq!(abbreviate_home(&sibling), sibling);
    }
}
