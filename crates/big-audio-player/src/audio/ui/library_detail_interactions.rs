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
        if n_press != 1 {
            return;
        }
        let mods = gesture.current_event_state();
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        if !ctrl && !shift {
            return;
        }
        let Some(widget) = gesture.widget() else {
            return;
        };
        let Some(pos) = find_clicked_position(&widget, x, y) else {
            return;
        };
        gesture.set_state(gtk::EventSequenceState::Claimed);
        if ctrl {
            if sel.is_selected(pos) {
                sel.unselect_item(pos);
            } else {
                sel.select_item(pos, false);
            }
        } else if shift {
            let bitset = sel.selection();
            let anchor = if bitset.is_empty() {
                0
            } else {
                bitset.minimum()
            };
            let (start, count) = if pos >= anchor {
                (anchor, pos - anchor + 1)
            } else {
                (pos, anchor - pos + 1)
            };
            sel.unselect_all();
            sel.select_range(start, count, false);
        }
    });
    gesture
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
        if (pos as usize) >= tracks.len() {
            return;
        }
        show_detail_context_menu(&widget, x, y, tracks, pos as usize, &out);
    });
    gesture
}

/// Collect every detail track in visual (sorted) order.
fn gather_visual_tracks(sort_model: &gtk::SortListModel) -> Vec<Track> {
    (0..sort_model.n_items())
        .filter_map(|i| sort_model.item(i).and_downcast::<glib::BoxedAnyObject>())
        .map(|obj| obj.borrow::<Track>().clone())
        .collect()
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
