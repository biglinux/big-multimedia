//! Shared "Add to Playlist" picker for the library panel (push-model).
//!
//! One dialog, reused by both the detail add-button column and the right-click
//! context menu. The shell resolves the
//! existing playlists and pushes them in as `playlists: &[(id, name)]`; the
//! panel never queries the DB. Picking a row emits `AddTrackToPlaylist`; the
//! "New Playlist" entry emits `CreatePlaylistWithTrack`.

use adw::prelude::*;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    name_prompt::{BigNamePromptSpec, prompt_name, trimmed_name},
};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use relm4::ComponentSender;

use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelOutput};
use big_media_runtime::i18n::gettext_noop;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AddToPlaylistDialogSpec {
    title: &'static str,
    content_width: i32,
    content_height: i32,
    page_width: i32,
    page_height: i32,
    content_spacing: i32,
    list_accessible_label: &'static str,
    empty_label: &'static str,
    close_label: &'static str,
    new_playlist_label: &'static str,
    row_prefix_icon_name: &'static str,
    row_action_label: &'static str,
}

fn add_to_playlist_dialog_spec() -> AddToPlaylistDialogSpec {
    AddToPlaylistDialogSpec {
        title: gettext_noop("Add to Playlist"),
        content_width: 420,
        content_height: 360,
        page_width: 420,
        page_height: 320,
        content_spacing: 12,
        list_accessible_label: gettext_noop("Playlists"),
        empty_label: gettext_noop("No playlists yet"),
        close_label: gettext_noop("Close"),
        new_playlist_label: gettext_noop("New Playlist"),
        row_prefix_icon_name: "playlist-symbolic",
        row_action_label: gettext_noop("Add to playlist"),
    }
}

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
    let dialog_spec = add_to_playlist_dialog_spec();
    let track_id = track.id;
    let dialog = big_gtk_kit::dialog::Dialog::builder()
        .title(i18n(dialog_spec.title))
        .content_width(dialog_spec.content_width)
        .content_height(dialog_spec.content_height)
        .build();
    let page = BigDialogPage::scrolled_clamped(dialog_spec.page_width, dialog_spec.page_height);
    page.content().set_spacing(dialog_spec.content_spacing);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list".to_string()])
        .build();
    list.update_property(&[gtk::accessible::Property::Label(&i18n(
        dialog_spec.list_accessible_label,
    ))]);

    for (playlist_id, name) in playlists {
        let row = BigNavigationRow::new(
            BigNavigationRowSpec::new(name.as_str())
                .prefix_icon_name(dialog_spec.row_prefix_icon_name)
                .action_label(i18n(dialog_spec.row_action_label)),
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
            .label(i18n(dialog_spec.empty_label))
            .halign(gtk::Align::Center)
            .margin_top(18)
            .margin_bottom(18)
            .build();
        empty_label.add_css_class("dim-label");
        list.append(&empty_label);
    }

    page.content().append(&list);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(
        i18n(dialog_spec.close_label),
        i18n(dialog_spec.new_playlist_label),
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
fn present_new_playlist_dialog(
    parent: &gtk::Widget,
    track_id: i64,
    sender: &ComponentSender<LibraryPanel>,
) {
    let spec = BigNamePromptSpec {
        title: i18n("New Playlist"),
        description: None,
        entry_title: i18n("Playlist name"),
        prefill: String::new(),
        accept_label: i18n("Create"),
        content_width: 420,
        content_height: 220,
    };
    let out = sender.clone();
    prompt_name(Some(parent), &spec, trimmed_name, move |name| {
        let _ = out.output(LibraryPanelOutput::CreatePlaylistWithTrack { name, track_id });
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_to_playlist_dialog_keeps_chrome() {
        assert_eq!(
            add_to_playlist_dialog_spec(),
            AddToPlaylistDialogSpec {
                title: "Add to Playlist",
                content_width: 420,
                content_height: 360,
                page_width: 420,
                page_height: 320,
                content_spacing: 12,
                list_accessible_label: "Playlists",
                empty_label: "No playlists yet",
                close_label: "Close",
                new_playlist_label: "New Playlist",
                row_prefix_icon_name: "playlist-symbolic",
                row_action_label: "Add to playlist",
            }
        );
    }
}
