//! The library track list's right-click menu. Each item emits an `Output`
//! intent — the shell owns the queue, mpv, and dialogs. Selection, drags and
//! row activation come from the drawn list (`track_list`, `library_panel`).

use adw::prelude::*;
use gtk::{gio, glib};
use relm4::ComponentSender;

use big_relm4_components::feedback::context_popover::BigContextPopover;

use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{LibraryPanel, LibraryPanelInput, LibraryPanelOutput};

/// Every track of the list, in its order: what "Play" queues.
pub(super) fn gather_visual_tracks(store: &gio::ListStore) -> Vec<Track> {
    collect_visual_tracks(store.n_items(), |index| store.item(index))
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

pub(super) fn detail_context_index(track_count: usize, clicked_position: u32) -> Option<usize> {
    let index = clicked_position as usize;
    if index < track_count {
        Some(index)
    } else {
        None
    }
}

/// Build + pop the per-track context menu; each item emits an `Output` intent.
pub(super) fn show_detail_context_menu(
    widget: &big_item_view::ItemView,
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

    {
        let view = widget.downgrade();
        let out = sender.clone();
        let track_id = track.id;
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
                    .and_downcast::<gio::ListStore>()
            {
                super::track_list::rate_in_place(&store, index as u32, rating);
                view.refresh(index as u32..index as u32 + 1);
            }
            let _ = out.output(LibraryPanelOutput::RateTrack {
                id: track_id,
                rating,
            });
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

    use super::{collect_visual_tracks, collect_visual_tracks_from_tracks, detail_context_index};
    use crate::db::models::Track;

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
