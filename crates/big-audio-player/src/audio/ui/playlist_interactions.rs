//! Queue `ColumnView` interactions for the `PlaylistPanel`.
//!
//! External-path drop, single-source row reorder, Delete/Backspace removal,
//! column-header sort commit, and the right-click track context menu. Each
//! emits an `Output` intent; the shell owns the queue mutation + undo.

use adw::prelude::*;
use gtk::{gdk, glib};
use relm4::ComponentSender;
use std::cell::Cell;
use std::rc::Rc;

use big_relm4_components::feedback::context_popover::BigContextPopover;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
};

use crate::audio::converter_launcher;
use crate::audio::ui::playlist_geom::{
    find_clicked_position, find_drop_position, find_reorder_drop_position,
};
use crate::db::models::Track;
use crate::i18n::i18n;

use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput};

fn is_queue_delete_key(keyval: gdk::Key) -> bool {
    keyval == gdk::Key::Delete || keyval == gdk::Key::BackSpace
}

fn valid_reorder_source_index(raw_index: i64) -> Option<usize> {
    if raw_index < 0 {
        None
    } else {
        Some(raw_index as usize)
    }
}

fn has_sorted_tracks_to_commit(tracks: &[Track]) -> bool {
    !tracks.is_empty()
}

fn resolve_queue_index_from_track_identities(
    visual_pos: u32,
    sorted_track_identity_at: impl FnOnce(u32) -> Option<glib::Object>,
    store_count: u32,
    queue_track_identity_at: impl FnMut(u32) -> Option<glib::Object>,
) -> Option<usize> {
    resolve_queue_index_from_identity_keys(
        visual_pos,
        sorted_track_identity_at,
        store_count,
        queue_track_identity_at,
    )
}

fn resolve_queue_index_from_identity_keys<T: PartialEq>(
    visual_pos: u32,
    sorted_track_identity_at: impl FnOnce(u32) -> Option<T>,
    store_count: u32,
    mut queue_track_identity_at: impl FnMut(u32) -> Option<T>,
) -> Option<usize> {
    let clicked_track_identity = sorted_track_identity_at(visual_pos)?;
    (0..store_count)
        .find(|&index| {
            queue_track_identity_at(index)
                .is_some_and(|track_identity| track_identity == clicked_track_identity)
        })
        .map(|index| index as usize)
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

/// Mount the external-path DropTarget on the scroll area: motion positions the
/// indicator line at the row under the pointer; drop emits `DropPaths` (the
/// shell resolves paths → tracks and mutates the queue).
pub(super) fn wire_path_drop(
    scroll: &gtk::ScrolledWindow,
    column_view: &gtk::ColumnView,
    store: &gio::ListStore,
    indicator: &gtk::Box,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let drop = big_app_kit::widgets::path_drop_target(gdk::DragAction::COPY);

    let (sc_m, cv_m, st_m, ind_m) = (
        scroll.downgrade(),
        column_view.downgrade(),
        store.downgrade(),
        indicator.clone(),
    );
    drop.connect_motion(move |_, _, y| {
        if let (Some(scroll), Some(cv), Some(store)) =
            (sc_m.upgrade(), cv_m.upgrade(), st_m.upgrade())
        {
            let dp = find_drop_position(&scroll, &cv, y, store.n_items() as usize);
            ind_m.set_margin_top(dp.indicator_y as i32);
        }
        ind_m.set_visible(true);
        gdk::DragAction::COPY
    });

    let ind_leave = indicator.clone();
    drop.connect_leave(move |_| ind_leave.set_visible(false));

    let (sc_d, cv_d, st_d, ind_d, out) = (
        scroll.downgrade(),
        column_view.downgrade(),
        store.downgrade(),
        indicator.clone(),
        sender.clone(),
    );
    drop.connect_drop(move |_, value, _, y| {
        ind_d.set_visible(false);
        let paths: Vec<String> = big_app_kit::widgets::drop_paths_from_value(value)
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if paths.is_empty() {
            return false;
        }
        let insert_pos = match (sc_d.upgrade(), cv_d.upgrade(), st_d.upgrade()) {
            (Some(scroll), Some(cv), Some(store)) => {
                find_drop_position(&scroll, &cv, y, store.n_items() as usize).insert_index
            }
            _ => return false,
        };
        let _ = out.output(PlaylistPanelOutput::DropPaths { insert_pos, paths });
        true
    });

    scroll.add_controller(drop);
}

/// DragSource for single-row reorder: carries the source visual index as i64.
/// Multi-row reorder is intentionally unsupported (ambiguous semantics).
pub(super) fn build_reorder_drag_source(selection: &gtk::MultiSelection) -> gtk::DragSource {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::MOVE);
    let sel = selection.clone();
    drag.connect_prepare(move |_, _, _| {
        let bitset = sel.selection();
        if bitset.size() != 1 {
            return None;
        }
        let idx = i64::from(bitset.minimum());
        Some(gdk::ContentProvider::for_value(&idx.to_value()))
    });
    drag.connect_drag_begin(|src, _| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let theme = gtk::IconTheme::for_display(&display);
        let icon = theme.lookup_icon(
            "view-list-symbolic",
            &[],
            32,
            1,
            gtk::TextDirection::Ltr,
            gtk::IconLookupFlags::empty(),
        );
        src.set_icon(Some(&icon), 16, 16);
    });
    drag
}

/// DropTarget for row reorder: positions the indicator and, on drop, emits
/// `ReorderTrack{from, to}` (the shell moves the queue track + records undo).
pub(super) fn wire_reorder_drop(
    scroll: &gtk::ScrolledWindow,
    column_view: &gtk::ColumnView,
    store: &gio::ListStore,
    indicator: &gtk::Box,
    sender: &ComponentSender<PlaylistPanel>,
) {
    // bigagents: app-local-dnd. Queue reorder transfers internal row ids, not paths.
    let drop = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);

    let (sc_m, cv_m, st_m, ind_m) = (
        scroll.downgrade(),
        column_view.downgrade(),
        store.downgrade(),
        indicator.clone(),
    );
    drop.connect_motion(move |_, _, y| {
        if let (Some(scroll), Some(cv), Some(store)) =
            (sc_m.upgrade(), cv_m.upgrade(), st_m.upgrade())
        {
            let dp = find_reorder_drop_position(&scroll, &cv, y, store.n_items() as usize);
            ind_m.set_margin_top(dp.indicator_y as i32);
        }
        ind_m.set_visible(true);
        gdk::DragAction::MOVE
    });

    let ind_leave = indicator.clone();
    drop.connect_leave(move |_| ind_leave.set_visible(false));

    let (sc_d, cv_d, st_d, ind_d, out) = (
        scroll.downgrade(),
        column_view.downgrade(),
        store.downgrade(),
        indicator.clone(),
        sender.clone(),
    );
    drop.connect_drop(move |_, value, _, y| {
        ind_d.set_visible(false);
        let Some(from) = valid_reorder_source_index(value.get::<i64>().unwrap_or(-1)) else {
            return false;
        };
        let (Some(scroll), Some(cv), Some(store)) =
            (sc_d.upgrade(), cv_d.upgrade(), st_d.upgrade())
        else {
            return false;
        };
        let to = find_reorder_drop_position(&scroll, &cv, y, store.n_items() as usize).insert_index;
        let _ = out.output(PlaylistPanelOutput::ReorderTrack { from, to });
        true
    });

    column_view.add_controller(drop);
}

/// Delete/Backspace on the selection → `RemoveTracks` (queue indices, desc).
pub(super) fn wire_delete_key(
    column_view: &gtk::ColumnView,
    selection: &gtk::MultiSelection,
    sort_model: &gtk::SortListModel,
    store: &gio::ListStore,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let key = gtk::EventControllerKey::new();
    let (sel, sm, st, out) = (
        selection.clone(),
        sort_model.clone(),
        store.clone(),
        sender.clone(),
    );
    key.connect_key_pressed(move |_, keyval, _, _| {
        if !is_queue_delete_key(keyval) {
            return glib::Propagation::Proceed;
        }
        let bitset = sel.selection();
        if bitset.is_empty() {
            return glib::Propagation::Proceed;
        }
        let mut indices: Vec<usize> = (0..bitset.size())
            .filter_map(|i| resolve_queue_index(&sm, &st, bitset.nth(i as u32)))
            .collect();
        // Highest-first so the shell's removals keep lower indices valid.
        indices.sort_unstable_by(|a, b| b.cmp(a));
        indices.dedup();
        if indices.is_empty() {
            return glib::Propagation::Proceed;
        }
        let _ = out.output(PlaylistPanelOutput::RemoveTracks { indices });
        glib::Propagation::Stop
    });
    column_view.add_controller(key);
}

/// Commit a column-header sort as the new queue order. `sorting_flag` guards
/// re-entrancy; the sort_model is captured WEAK (a strong capture forms a
/// sorter↔sort_model cycle, since the sorter belongs to the column view whose
/// model is this sort_model).
pub(super) fn wire_sort_commit(
    column_view: &gtk::ColumnView,
    sort_model: &gtk::SortListModel,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let Some(sorter) = column_view.sorter() else {
        return;
    };
    sort_model.set_sorter(Some(&sorter));

    let sorting_flag = Rc::new(Cell::new(false));
    let sm = sort_model.downgrade();
    let out = sender.clone();
    sorter.connect_changed(move |_, _| {
        if sorting_flag.get() {
            return;
        }
        let Some(sm) = sm.upgrade() else {
            return;
        };
        let flag = sorting_flag.clone();
        let out = out.clone();
        glib::idle_add_local_once(move || {
            big_app_kit::containment::guard("audio.playlist.commit_sort", || {
                flag.set(true);
                let tracks: Vec<Track> = (0..sm.n_items())
                    .filter_map(|i| sm.item(i).and_downcast::<glib::BoxedAnyObject>())
                    .map(|obj| obj.borrow::<Track>().clone())
                    .collect();
                flag.set(false);
                if has_sorted_tracks_to_commit(&tracks) {
                    let _ = out.output(PlaylistPanelOutput::CommitSortedOrder { tracks });
                }
            });
        });
    });
}

/// Map a visual (sorted) position to its queue/store index by object identity.
fn resolve_queue_index(
    sort_model: &gtk::SortListModel,
    store: &gio::ListStore,
    visual_pos: u32,
) -> Option<usize> {
    resolve_queue_index_from_track_identities(
        visual_pos,
        |position| sort_model.item(position),
        store.n_items(),
        |index| store.item(index),
    )
}

/// Collect `(queue_index, track)` for the current selection.
fn gather_selected_tracks(
    sort_model: &gtk::SortListModel,
    store: &gio::ListStore,
    bitset: &gtk::Bitset,
) -> Vec<(usize, Track)> {
    let mut selected = Vec::with_capacity(bitset.size() as usize);
    for i in 0..bitset.size() {
        let vpos = bitset.nth(i as u32);
        if let Some(idx) = resolve_queue_index(sort_model, store, vpos) {
            if let Some(obj) = store
                .item(idx as u32)
                .and_downcast::<glib::BoxedAnyObject>()
            {
                selected.push((idx, obj.borrow::<Track>().clone()));
            }
        }
    }
    selected
}

/// Right-click → select-under-pointer-if-needed → gather → show context menu.
pub(super) fn wire_right_click_menu(
    column_view: &gtk::ColumnView,
    selection: &gtk::MultiSelection,
    sort_model: &gtk::SortListModel,
    store: &gio::ListStore,
    scroll: &gtk::ScrolledWindow,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    let (sel, sm, st, sc, out) = (
        selection.clone(),
        sort_model.clone(),
        store.clone(),
        scroll.downgrade(),
        sender.clone(),
    );
    gesture.connect_released(move |g, _, x, y| {
        let Some(widget) = g.widget() else {
            return;
        };
        // Right-click on an unselected row replaces the selection so the menu
        // acts on what the user pointed at.
        if let Some(pos) = find_clicked_position(&widget, x, y) {
            if !sel.selection().contains(pos) {
                sel.unselect_all();
                sel.select_item(pos, false);
            }
        }
        let selected = gather_selected_tracks(&sm, &st, &sel.selection());
        if selected.is_empty() {
            return;
        }
        let Some(scroll) = sc.upgrade() else {
            return;
        };
        show_track_context_menu(&widget, &scroll, x, y, selected, &out);
    });
    column_view.add_controller(gesture);
}

/// Build + pop the per-track context menu; every item emits an `Output` intent.
fn show_track_context_menu(
    widget: &gtk::Widget,
    scroll: &gtk::ScrolledWindow,
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
    if converter_launcher::is_available() {
        let out = sender.clone();
        let paths: Vec<String> = selected.iter().map(|(_, t)| t.file_path.clone()).collect();
        menu.append_button(i18n("Edit / Convert…"), &[], move || {
            let _ = out.output(PlaylistPanelOutput::ConvertTracks(paths.clone()));
        });
    }

    // Anchor to the scroll viewport so coords survive list scrolling.
    let scroll_widget: gtk::Widget = scroll.clone().upcast();
    let (parent, px, py) = widget
        .compute_point(
            &scroll_widget,
            &gtk::graphene::Point::new(x as f32, y as f32),
        )
        .map_or_else(
            || (widget.clone(), x, y),
            |pt| (scroll_widget, f64::from(pt.x()), f64::from(pt.y())),
        );
    menu.popup_at(&parent, px, py);
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
