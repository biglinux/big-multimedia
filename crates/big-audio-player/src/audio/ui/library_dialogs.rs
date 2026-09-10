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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NewPlaylistDialogSpec {
    title: &'static str,
    content_width: i32,
    content_height: i32,
    content_spacing: i32,
    entry_title: &'static str,
    cancel_label: &'static str,
    create_label: &'static str,
    focus_select_start: i32,
    focus_select_end: i32,
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

fn new_playlist_dialog_spec() -> NewPlaylistDialogSpec {
    NewPlaylistDialogSpec {
        title: gettext_noop("New Playlist"),
        content_width: 420,
        content_height: 220,
        content_spacing: 12,
        entry_title: gettext_noop("Playlist name"),
        cancel_label: gettext_noop("Cancel"),
        create_label: gettext_noop("Create"),
        focus_select_start: 0,
        focus_select_end: -1,
    }
}

fn can_create_playlist_name(name: &str) -> bool {
    normalized_playlist_name(name).is_some()
}

fn normalized_playlist_name(name: &str) -> Option<String> {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        None
    } else {
        Some(trimmed_name.to_string())
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
    let dialog = adw::Dialog::builder()
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
fn present_new_playlist_dialog<W>(parent: &W, track_id: i64, sender: &ComponentSender<LibraryPanel>)
where
    W: IsA<gtk::Widget>,
{
    let dialog_spec = new_playlist_dialog_spec();
    let dialog = adw::Dialog::builder()
        .title(i18n(dialog_spec.title))
        .content_width(dialog_spec.content_width)
        .content_height(dialog_spec.content_height)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(dialog_spec.content_spacing);

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(i18n(dialog_spec.entry_title))
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(
        i18n(dialog_spec.cancel_label),
        i18n(dialog_spec.create_label),
    ));
    footer.accept_button().set_sensitive(false);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    entry.connect_changed(move |row| {
        accept_button.set_sensitive(can_create_playlist_name(&row.text()));
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
        let Some(name) = normalized_playlist_name(&entry.text()) else {
            return;
        };
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
            entry_for_idle
                .select_region(dialog_spec.focus_select_start, dialog_spec.focus_select_end);
        });
    });
    dialog.present(Some(parent));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_to_playlist_dialog_spec_keeps_picker_chrome_and_row_actions() {
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

    #[test]
    fn new_playlist_dialog_spec_keeps_entry_chrome_and_focus_selection() {
        assert_eq!(
            new_playlist_dialog_spec(),
            NewPlaylistDialogSpec {
                title: "New Playlist",
                content_width: 420,
                content_height: 220,
                content_spacing: 12,
                entry_title: "Playlist name",
                cancel_label: "Cancel",
                create_label: "Create",
                focus_select_start: 0,
                focus_select_end: -1,
            }
        );
    }

    #[test]
    fn can_create_playlist_name_requires_non_blank_trimmed_text() {
        assert!(!can_create_playlist_name(""));
        assert!(!can_create_playlist_name("   "));
        assert!(can_create_playlist_name("Road Trip"));
        assert!(can_create_playlist_name("  Road Trip  "));
        assert_eq!(
            normalized_playlist_name("  Road Trip  "),
            Some("Road Trip".to_string())
        );
        assert_eq!(normalized_playlist_name("   "), None);
    }
}
