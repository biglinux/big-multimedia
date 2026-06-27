//! Shared "Add to Playlist" picker for the library panel (push-model).
//!
//! One dialog, reused by both the detail add-button column and the right-click
//! context menu (the legacy window built this twice). The shell resolves the
//! existing playlists and pushes them in as `playlists: &[(id, name)]`; the
//! panel never queries the DB. Picking a row emits `AddTrackToPlaylist`; the
//! "New Playlist" entry emits `CreatePlaylistWithTrack`.

use adw::prelude::*;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use gtk::glib;
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
    let dialog = adw::Dialog::builder()
        .title(i18n("Add to Playlist"))
        .content_width(420)
        .content_height(360)
        .build();
    let page = BigDialogPage::scrolled_clamped(420, 320);
    page.content().set_spacing(12);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list".to_string()])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(&i18n("Playlists"))]);

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

    if playlists.is_empty() {
        let empty_label = gtk::Label::builder()
            .label(i18n("No playlists yet"))
            .halign(gtk::Align::Center)
            .margin_top(18)
            .margin_bottom(18)
            .build();
        empty_label.add_css_class("dim-label");
        list.append(&empty_label);
    }

    page.content().append(&list);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(
        i18n("Close"),
        i18n("New Playlist"),
    ));
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let out = sender.clone();
    let parent_weak = parent.as_ref().downgrade();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
        if let Some(parent) = parent_weak.upgrade() {
            present_new_playlist_dialog(&parent, track_id, &out);
        }
    });

    dialog.present(Some(parent));
}

/// Name-entry dialog for "New Playlist" → `CreatePlaylistWithTrack`. Ignores an
/// empty (trimmed) name.
fn present_new_playlist_dialog<W>(parent: &W, track_id: i64, sender: &ComponentSender<LibraryPanel>)
where
    W: IsA<gtk::Widget>,
{
    let dialog = adw::Dialog::builder()
        .title(i18n("New Playlist"))
        .content_width(420)
        .content_height(220)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(i18n("Playlist name"))
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Create")));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    entry.connect_changed(move |row| {
        accept_button.set_sensitive(!row.text().trim().is_empty());
    });

    let accept_button = footer.accept_button().clone();
    entry.connect_apply(move |_| {
        if accept_button.is_sensitive() {
            accept_button.emit_clicked();
        }
    });

    let out = sender.clone();
    let entry_for_accept = entry.clone();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        let name = entry.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        let _ = out.output(LibraryPanelOutput::CreatePlaylistWithTrack { name, track_id });
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

    dialog.connect_map(move |_| {
        let entry_for_idle = entry_for_accept.clone();
        glib::idle_add_local_once(move || {
            entry_for_idle.grab_focus();
            entry_for_idle.select_region(0, -1);
        });
    });
    dialog.present(Some(parent));
}
