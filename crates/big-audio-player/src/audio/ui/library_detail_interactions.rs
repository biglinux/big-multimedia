//! Detail `ColumnView` interactions for the `LibraryPanel`: drag-source (export
//! selected file paths), Ctrl/Shift multi-select, and the right-click track
//! context menu. Each menu item emits an `Output` intent — the shell owns the
//! queue, mpv, and dialogs. Row activation is wired in `library_panel` (init).

use adw::prelude::*;
use gtk::{gdk, glib};
use relm4::ComponentSender;

use big_relm4_components::feedback::context_popover::BigContextPopover;

use crate::audio::ui::playlist_geom::find_clicked_position;
use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailSelectionAction {
    Ignore,
    ToggleOff(u32),
    ToggleOn(u32),
    SelectRange { start: u32, count: u32 },
}

/// Wire the detail view's drag-source, multi-select gesture, and right-click
/// context menu onto `column_view`.
pub(super) fn build_detail_interactions(
    column_view: &gtk::ColumnView,
    selection: &gtk::MultiSelection,
    sort_model: &gtk::SortListModel,
    sender: &ComponentSender<LibraryPanel>,
) {
    column_view.add_controller(build_drag_source(selection));
    column_view.add_controller(build_multi_select_gesture(selection));
    column_view.add_controller(build_right_click_gesture(sort_model, sender));
}

/// DragSource (COPY): carries the selected rows' file paths, newline-joined, so
/// a drop onto the queue (or an external target) can resolve them.
fn build_drag_source(selection: &gtk::MultiSelection) -> gtk::DragSource {
    let drag = gtk::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    let sel = selection.clone();
    drag.connect_prepare(move |_, _, _| {
        let bitset = sel.selection();
        if bitset.is_empty() {
            return None;
        }
        let mut paths = Vec::with_capacity(bitset.size() as usize);
        for n in 0..bitset.size() {
            if let Some(track) = sel
                .item(bitset.nth(n as u32))
                .and_downcast::<glib::BoxedAnyObject>()
                .map(|obj| obj.borrow::<Track>().file_path.clone())
            {
                paths.push(track);
            }
        }
        if paths.is_empty() {
            return None;
        }
        Some(gdk::ContentProvider::for_value(
            &paths.join("\n").to_value(),
        ))
    });
    drag.connect_drag_begin(|src, _| {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let icon = gtk::IconTheme::for_display(&display).lookup_icon(
            "audio-x-generic",
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

/// Capture-phase click gesture implementing Ctrl (toggle) / Shift (range) multi-
/// select; plain clicks fall through to the default `ColumnView` handler.
fn build_multi_select_gesture(selection: &gtk::MultiSelection) -> gtk::GestureClick {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(1);
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let sel = selection.clone();
    gesture.connect_pressed(move |gesture, n_press, x, y| {
        let mods = gesture.current_event_state();
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let Some(widget) = gesture.widget() else {
            return;
        };
        let clicked_position = find_clicked_position(&widget, x, y);
        let selected_positions = sel.selection();
        let selection_anchor = if selected_positions.is_empty() {
            None
        } else {
            Some(selected_positions.minimum())
        };
        let action = detail_selection_action(
            n_press,
            ctrl,
            shift,
            clicked_position,
            selection_anchor,
            clicked_position.is_some_and(|position| sel.is_selected(position)),
        );
        match action {
            DetailSelectionAction::Ignore => (),
            DetailSelectionAction::ToggleOff(position) => {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sel.unselect_item(position);
            }
            DetailSelectionAction::ToggleOn(position) => {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sel.select_item(position, false);
            }
            DetailSelectionAction::SelectRange { start, count } => {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                sel.unselect_all();
                sel.select_range(start, count, false);
            }
        }
    });
    gesture
}

fn detail_selection_action(
    click_count: i32,
    has_control: bool,
    has_shift: bool,
    clicked_position: Option<u32>,
    selection_anchor: Option<u32>,
    is_clicked_selected: bool,
) -> DetailSelectionAction {
    if click_count != 1 || (!has_control && !has_shift) {
        return DetailSelectionAction::Ignore;
    }
    let Some(position) = clicked_position else {
        return DetailSelectionAction::Ignore;
    };
    if has_control {
        if is_clicked_selected {
            return DetailSelectionAction::ToggleOff(position);
        }
        return DetailSelectionAction::ToggleOn(position);
    }
    let anchor = selection_anchor.unwrap_or(0);
    let (start, count) = if position >= anchor {
        (anchor, position - anchor + 1)
    } else {
        (position, anchor - position + 1)
    };
    DetailSelectionAction::SelectRange { start, count }
}

/// Right-click gesture → resolve the clicked row → per-track context menu.
fn build_right_click_gesture(
    sort_model: &gtk::SortListModel,
    sender: &ComponentSender<LibraryPanel>,
) -> gtk::GestureClick {
    let gesture = gtk::GestureClick::builder().button(3).build();
    let sm = sort_model.clone();
    let out = sender.clone();
    gesture.connect_released(move |gesture, _, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let Some(pos) = find_clicked_position(&widget, x, y) else {
            return;
        };
        let tracks = gather_visual_tracks(&sm);
        let Some(index) = detail_context_index(tracks.len(), pos) else {
            return;
        };
        show_detail_context_menu(&widget, x, y, tracks, index, &out);
    });
    gesture
}

/// Collect every detail track in visual (sorted) order.
fn gather_visual_tracks(sort_model: &gtk::SortListModel) -> Vec<Track> {
    collect_visual_tracks(sort_model.n_items(), |index| sort_model.item(index))
}

fn collect_visual_tracks(
    item_count: u32,
    item_at: impl Fn(u32) -> Option<glib::Object>,
) -> Vec<Track> {
    collect_visual_tracks_from_tracks(item_count, |index| {
        item_at(index)
            .and_downcast::<glib::BoxedAnyObject>()
            .map(|obj| obj.borrow::<Track>().clone())
    })
}

fn collect_visual_tracks_from_tracks(
    item_count: u32,
    track_at: impl Fn(u32) -> Option<Track>,
) -> Vec<Track> {
    (0..item_count).filter_map(track_at).collect()
}

fn detail_context_index(track_count: usize, clicked_position: u32) -> Option<usize> {
    let index = clicked_position as usize;
    if index < track_count {
        Some(index)
    } else {
        None
    }
}

/// Build + pop the per-track context menu; each item emits an `Output` intent.
fn show_detail_context_menu(
    widget: &gtk::Widget,
    x: f64,
    y: f64,
    tracks: Vec<Track>,
    index: usize,
    sender: &ComponentSender<LibraryPanel>,
) {
    let track = tracks[index].clone();
    let menu = BigContextPopover::new(false);

    {
        let out = sender.clone();
        let tracks = tracks.clone();
        menu.append_button(i18n("Play"), &[], move || {
            let _ = out.output(LibraryPanelOutput::PlayAllFrom {
                tracks: tracks.clone(),
                index,
            });
        });
    }
    {
        let out = sender.clone();
        let t = track.clone();
        menu.append_button(i18n("Play Next"), &[], move || {
            let _ = out.output(LibraryPanelOutput::EnqueueNext(t.clone()));
        });
    }
    {
        let out = sender.clone();
        let t = track.clone();
        menu.append_button(i18n("Add to Queue"), &[], move || {
            let _ = out.output(LibraryPanelOutput::EnqueueTrack(t.clone()));
        });
    }

    menu.append_separator();
    {
        let out = sender.clone();
        let t = track.clone();
        menu.append_button(i18n("Add to Playlist…"), &[], move || {
            let _ = out.output(LibraryPanelOutput::ShowAddToPlaylist(t.clone()));
        });
    }
    {
        let out = sender.clone();
        let fp = track.file_path.clone();
        menu.append_button(i18n("Show in File Manager"), &[], move || {
            let _ = out.output(LibraryPanelOutput::RevealInFileManager(fp.clone()));
        });
    }
    {
        // The tag-editor dialog is panel-owned; open it directly (the detail
        // track is already a full `Track`). Save emits `WriteTags` to the shell.
        let s = sender.clone();
        let t = track.clone();
        menu.append_button(i18n("Edit Tags…"), &[], move || {
            s.input(LibraryPanelInput::EditTags(t.clone()));
        });
    }
    {
        let out = sender.clone();
        let t = track.clone();
        menu.append_button(i18n("Track Info"), &[], move || {
            let _ = out.output(LibraryPanelOutput::ShowTrackInfo(t.clone()));
        });
    }

    menu.popup_at(widget, x, y);
}

#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    use gtk::glib;
    #[cfg(not(miri))]
    use gtk::prelude::Cast;

    use super::{
        DetailSelectionAction, collect_visual_tracks, collect_visual_tracks_from_tracks,
        detail_context_index, detail_selection_action,
    };
    use crate::db::models::Track;

    #[test]
    fn detail_selection_ignores_plain_double_and_missing_position_clicks() {
        assert_eq!(
            detail_selection_action(1, false, false, Some(3), Some(1), false),
            DetailSelectionAction::Ignore
        );
        assert_eq!(
            detail_selection_action(2, true, false, Some(3), Some(1), false),
            DetailSelectionAction::Ignore
        );
        assert_eq!(
            detail_selection_action(1, true, false, None, Some(1), false),
            DetailSelectionAction::Ignore
        );
    }

    #[test]
    fn detail_selection_control_toggles_clicked_row() {
        assert_eq!(
            detail_selection_action(1, true, false, Some(4), Some(2), true),
            DetailSelectionAction::ToggleOff(4)
        );
        assert_eq!(
            detail_selection_action(1, true, false, Some(4), Some(2), false),
            DetailSelectionAction::ToggleOn(4)
        );
    }

    #[test]
    fn detail_selection_shift_uses_anchor_or_zero() {
        assert_eq!(
            detail_selection_action(1, false, true, Some(5), Some(2), false),
            DetailSelectionAction::SelectRange { start: 2, count: 4 }
        );
        assert_eq!(
            detail_selection_action(1, false, true, Some(2), Some(5), false),
            DetailSelectionAction::SelectRange { start: 2, count: 4 }
        );
        assert_eq!(
            detail_selection_action(1, false, true, Some(3), None, false),
            DetailSelectionAction::SelectRange { start: 0, count: 4 }
        );
    }

    #[test]
    fn detail_selection_control_takes_precedence_over_shift() {
        assert_eq!(
            detail_selection_action(1, true, true, Some(4), Some(2), false),
            DetailSelectionAction::ToggleOn(4)
        );
    }

    #[test]
    fn detail_context_index_accepts_only_visible_track_positions() {
        assert_eq!(detail_context_index(3, 0), Some(0));
        assert_eq!(detail_context_index(3, 2), Some(2));
        assert_eq!(detail_context_index(3, 3), None);
        assert_eq!(detail_context_index(0, 0), None);
    }

    #[test]
    fn collect_visual_tracks_preserves_sorted_model_order() {
        let visual_tracks = [
            Some(Track::new_external_path_with_title(
                "/music/one.flac",
                Some("One".to_string()),
            )),
            None,
            Some(Track::new_external_path_with_title(
                "/music/two.flac",
                Some("Two".to_string()),
            )),
        ];

        let file_paths: Vec<String> =
            collect_visual_tracks_from_tracks(visual_tracks.len() as u32, |index| {
                visual_tracks.get(index as usize).and_then(Clone::clone)
            })
            .into_iter()
            .map(|track| track.file_path)
            .collect();

        assert_eq!(file_paths, ["/music/one.flac", "/music/two.flac"]);
    }

    #[cfg(not(miri))]
    #[test]
    fn collect_visual_tracks_reads_boxed_track_objects() {
        let first = Track::new_external_path_with_title("/music/one.flac", Some("One".into()));
        let second = Track::new_external_path_with_title("/music/two.flac", Some("Two".into()));
        let visual_items = [
            Some(glib::BoxedAnyObject::new(first.clone()).upcast::<glib::Object>()),
            None,
            Some(glib::BoxedAnyObject::new(second.clone()).upcast::<glib::Object>()),
        ];

        let tracks = collect_visual_tracks(visual_items.len() as u32, |index| {
            visual_items.get(index as usize).and_then(Clone::clone)
        });

        assert_eq!(
            tracks
                .into_iter()
                .map(|track| track.file_path)
                .collect::<Vec<_>>(),
            [first.file_path, second.file_path]
        );
    }
}
