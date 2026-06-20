//! Shared "Add to Playlist" picker for the library panel (push-model).
//!
//! One dialog, reused by both the detail add-button column and the right-click
//! context menu (the legacy window built this twice). The shell resolves the
//! existing playlists and pushes them in as `playlists: &[(id, name)]`; the
//! panel never queries the DB. Picking a row emits `AddTrackToPlaylist`; the
//! "New Playlist" entry emits `CreatePlaylistWithTrack`.

use adw::prelude::*;
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use relm4::ComponentSender;

use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};

/// Present the add-to-playlist picker for `track`. Lists each `(id, name)` in
/// `playlists` as an activatable row, plus a "New Playlist…" row that opens a
/// name entry. `parent` is the toplevel window. The dialog closes itself on the
/// "close" response; row activation emits an intent then dismisses the dialog.
pub(super) fn show_add_to_playlist<W>(
    parent: &W,
    track: &Track,
    playlists: &[(i64, String)],
    sender: &ComponentSender<LibraryPanel>,
) where
    W: IsA<gtk::Widget>,
{
    let track_id = track.id;
    let (dialog, content) =
        big_app_kit::dialogs::content_dialog(&i18n("Add to Playlist"), &i18n("Cancel"));

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list".to_string()])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(&i18n("Playlists"))]);

    // "New Playlist" — always first, opens a name entry.
    {
        let new_row = BigNavigationRow::new(
            BigNavigationRowSpec::new(i18n("New Playlist…"))
                .prefix_icon_name("list-add-symbolic")
                .action_label(i18n("Create playlist")),
        )
        .into_root();
        let out = sender.clone();
        let dialog_weak = dialog.downgrade();
        new_row.connect_activated(move |row| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
            let Some(window) = row.root().and_downcast::<gtk::Window>() else {
                return;
            };
            present_new_playlist_dialog(&window, track_id, &out);
        });
        list.append(&new_row);
    }

    for (playlist_id, name) in playlists {
        let row = BigNavigationRow::new(
            BigNavigationRowSpec::new(name.as_str())
                .prefix_icon_name("playlist-symbolic")
                .action_label(i18n("Add to playlist")),
        )
        .into_root();
        let playlist_id = *playlist_id;
        let out = sender.clone();
        let dialog_weak = dialog.downgrade();
        row.connect_activated(move |_| {
            let _ = out.output(LibraryPanelOutput::AddTrackToPlaylist {
                track_id,
                playlist_id,
            });
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });
        list.append(&row);
    }

    content.append(&list);
    dialog.present(Some(parent));
}

/// Name-entry dialog for "New Playlist" → `CreatePlaylistWithTrack`. Ignores an
/// empty (trimmed) name.
fn present_new_playlist_dialog<W>(parent: &W, track_id: i64, sender: &ComponentSender<LibraryPanel>)
where
    W: IsA<gtk::Widget>,
{
    let (dialog, content) = big_app_kit::dialogs::content_action_dialog(
        &i18n("New Playlist"),
        &i18n("Cancel"),
        "create",
        &i18n("Create"),
        false,
    );
    let entry = gtk::Entry::builder()
        .placeholder_text(i18n("Playlist name"))
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&i18n("Playlist name"))]);
    content.append(&entry);

    let out = sender.clone();
    dialog.connect_response(None, move |_, response| {
        if response != "create" {
            return;
        }
        let name = entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let _ = out.output(LibraryPanelOutput::CreatePlaylistWithTrack { name, track_id });
    });
    dialog.present(Some(parent));
}
