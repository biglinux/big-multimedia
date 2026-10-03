//! Queue interactions for the `PlaylistPanel`: Delete/Backspace removal and the
//! right-click track context menu. Each emits an `Output` intent; the shell
//! owns the queue mutation + undo.

use adw::prelude::*;
use gtk::{gdk, glib};
use relm4::ComponentSender;

use big_relm4_components::feedback::context_popover::BigContextPopover;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

use crate::db::models::Track;
use crate::i18n::i18n;
use crate::shared::converter_handoff;

use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput};

fn is_queue_delete_key(keyval: gdk::Key) -> bool {
    keyval == gdk::Key::Delete || keyval == gdk::Key::BackSpace
}

pub(super) fn valid_reorder_source_index(raw_index: i64) -> Option<usize> {
    if raw_index < 0 {
        None
    } else {
        Some(raw_index as usize)
    }
}

fn remove_from_queue_label(count: usize) -> String {
    if count > 1 {
        format!("{} ({count})", i18n("Remove from Queue"))
    } else {
        i18n("Remove from Queue")
    }
}

fn should_show_single_track_actions(count: usize) -> bool {
    count == 1
}

fn downloadable_episode_items(selected: &[(usize, Track)]) -> Vec<(String, String)> {
    selected
        .iter()
        .filter(|(_, track)| {
            track.file_path.starts_with("http://") || track.file_path.starts_with("https://")
        })
        .map(|(_, track)| (track.file_path.clone(), track.display_title().to_string()))
        .collect()
}

fn download_episode_label(episode_count: usize) -> Option<String> {
    match episode_count {
        0 => None,
        1 => Some(i18n("Download Episode…")),
        count => Some(format!("{} ({count})", i18n("Download Episodes…"))),
    }
}

/// Delete/Backspace on the selection → `RemoveTracks` (queue indices, desc).
pub(super) fn wire_delete_key(
    view: &big_item_view::ItemView,
    selection: &gtk::MultiSelection,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let key = gtk::EventControllerKey::new();
    let (sel, out) = (selection.downgrade(), sender.clone());
    key.connect_key_pressed(move |_, keyval, _, _| {
        if !is_queue_delete_key(keyval) {
            return glib::Propagation::Proceed;
        }
        let Some(sel) = sel.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let bitset = sel.selection();
        // Highest-first so the shell's removals keep lower indices valid.
        let mut indices: Vec<usize> = (0..bitset.size())
            .map(|nth| bitset.nth(nth as u32) as usize)
            .collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        if indices.is_empty() {
            return glib::Propagation::Proceed;
        }
        let _ = out.output(PlaylistPanelOutput::RemoveTracks { indices });
        glib::Propagation::Stop
    });
    view.add_controller(key);
}

/// Build + pop the per-track context menu; every item emits an `Output` intent.
pub(super) fn show_track_context_menu(
    widget: &big_item_view::ItemView,
    x: f64,
    y: f64,
    selected: Vec<(usize, Track)>,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let count = selected.len();
    let first_idx = selected[0].0;
    let first_track = selected[0].1.clone();

    let menu = BigContextPopover::new(false);

    {
        let out = sender.clone();
        menu.append_button(i18n("Play"), &[], move || {
            let _ = out.output(PlaylistPanelOutput::JumpTo { index: first_idx });
        });
    }
    {
        let label = remove_from_queue_label(count);
        let mut indices: Vec<usize> = selected.iter().map(|(i, _)| *i).collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        indices.dedup();
        let out = sender.clone();
        menu.append_button(label, &[], move || {
            let _ = out.output(PlaylistPanelOutput::RemoveTracks {
                indices: indices.clone(),
            });
        });
    }

    if should_show_single_track_actions(count) {
        let view = widget.downgrade();
        let out = sender.clone();
        let track_id = first_track.id;
        let stars: Vec<(&str, String)> = (1..=5)
            .map(|n| {
                (
                    "starred-symbolic",
                    i18n("{} of 5 stars").replace("{}", &n.to_string()),
                )
            })
            .collect();
        menu.append_icon_choices(&i18n("Rating"), &stars, move |chosen| {
            let rating = i32::try_from(chosen + 1).unwrap_or(5);
            if let Some(view) = view.upgrade()
                && let Some(store) = view
                    .model()
                    .and_downcast::<gtk::MultiSelection>()
                    .and_then(|selection| selection.model())
                    .and_downcast::<gtk::gio::ListStore>()
            {
                super::track_list::rate_in_place(&store, first_idx as u32, rating);
                view.refresh(first_idx as u32..first_idx as u32 + 1);
            }
            let _ = out.output(PlaylistPanelOutput::RateTrack {
                id: track_id,
                rating,
            });
        });
    }

    if should_show_single_track_actions(count) {
        menu.append_separator();
        {
            let out = sender.clone();
            let t = first_track.clone();
            menu.append_button(i18n("Track Info"), &[], move || {
                let _ = out.output(PlaylistPanelOutput::ShowTrackInfo(t.clone()));
            });
        }
        {
            let out = sender.clone();
            let t = first_track.clone();
            menu.append_button(i18n("Add to Playlist…"), &[], move || {
                let _ = out.output(PlaylistPanelOutput::ShowAddToPlaylistDialog(t.clone()));
            });
        }
        {
            let out = sender.clone();
            let fp = first_track.file_path.clone();
            menu.append_button(i18n("Show in File Manager"), &[], move || {
                let _ = out.output(PlaylistPanelOutput::RevealInFileManager(fp.clone()));
            });
        }
    }

    // Podcast episodes only (HTTP URLs).
    let http_episodes = downloadable_episode_items(&selected);
    if let Some(label) = download_episode_label(http_episodes.len()) {
        let out = sender.clone();
        menu.append_button(label, &[], move || {
            let _ = out.output(PlaylistPanelOutput::DownloadEpisodes(http_episodes.clone()));
        });
    }

    menu.append_separator();
    {
        let label = delete_storage_heading(count);
        let out = sender.clone();
        let items = selected.clone();
        let parent = widget.root().and_downcast::<gtk::Window>();
        menu.append_button(label, &["error"], move || {
            show_delete_confirm(parent.as_ref(), items.clone(), &out);
        });
    }
    if converter_handoff::is_installed(converter_handoff::AUDIO_CONVERTER) {
        let out = sender.clone();
        let paths: Vec<String> = selected.iter().map(|(_, t)| t.file_path.clone()).collect();
        menu.append_button(i18n("Edit / Convert…"), &[], move || {
            let _ = out.output(PlaylistPanelOutput::ConvertTracks(paths.clone()));
        });
    }

    // The view scrolls itself: its own coordinates are what is on screen.
    menu.popup_at(widget, x, y);
}

/// Trash-vs-permanent confirm; on confirm emits `DeleteTracksFromStorage`.
fn show_delete_confirm(
    parent: Option<&gtk::Window>,
    items: Vec<(usize, Track)>,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let heading = delete_storage_heading(items.len());
    let body = delete_storage_body(
        items.len(),
        items.first().map(|(_, track)| track.display_title()),
    );

    let dialog = adw::Dialog::builder()
        .title(&heading)
        .content_width(440)
        .content_height(280)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&body);

    let group = BigPreferencesGroupSpec::untitled().build();
    let perm_row = adw::SwitchRow::builder()
        .title(i18n("Delete permanently"))
        .subtitle(i18n("Skip trash — cannot be undone"))
        .active(false)
        .build();
    group.add(&perm_row);
    page.content().append(&group);

    let footer = BigDialogFooter::new(
        &BigDialogFooterSpec::new(i18n("Cancel"), i18n("Delete"))
            .with_accept_css_classes(&["destructive-action"]),
    );
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let out = sender.clone();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        let _ = out.output(PlaylistPanelOutput::DeleteTracksFromStorage {
            items: items.clone(),
            permanent: perm_row.is_active(),
        });
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
    dialog.present(parent);
}

fn delete_storage_heading(count: usize) -> String {
    if count == 1 {
        i18n("Delete from Storage")
    } else {
        format!("{} ({count})", i18n("Delete from Storage"))
    }
}

fn delete_storage_body(count: usize, single_title: Option<&str>) -> String {
    if count == 1 {
        format!(
            "{}\n\n{}",
            i18n("Move this file to the trash?"),
            single_title.unwrap_or_default()
        )
    } else {
        format!(
            "{}\n\n{} {}",
            i18n("Move these files to the trash?"),
            count,
            i18n("tracks")
        )
    }
}

#[cfg(test)]
mod tests;
