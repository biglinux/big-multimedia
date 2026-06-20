//! Detail `ColumnView` column builders for the `LibraryPanel`.
//!
//! Default-visible: album art, title (+ now-playing indicator by file path),
//! artist, duration, add-to-playlist. Hidden by default (toggled via the column-
//! config menu): track #, album, genre, year, bitrate, format, rating. The art
//! column is self-contained (panel-owned LRU cache + off-thread extraction);
//! rating and add emit `Output` intents — the shell owns the DB + dialogs.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};
use relm4::ComponentSender;

use crate::audio::column_builders::{
    make_duration_factory, make_duration_sorter, make_label_factory, make_rating_sorter,
    make_track_text_sorter, make_year_factory, make_year_sorter, unbind_click_gestures,
    update_star_icons,
};
use crate::db::models::Track;
use crate::i18n::i18n;

use super::library_panel::{DetailIndicator, LibraryPanel, LibraryPanelOutput};

/// Bounded album-art texture cache, keyed by `art_hash`.
pub(super) type ArtCache = Rc<RefCell<lru::LruCache<String, gdk::Texture>>>;

/// Build the detail column set. The title column registers its indicator widgets
/// into `indicators` for `SetPlayingPath` re-light; rating/add emit intents.
pub(super) fn build_detail_columns(
    column_view: &gtk::ColumnView,
    sender: &ComponentSender<LibraryPanel>,
    current_path: &Rc<RefCell<Option<String>>>,
    indicators: &Rc<RefCell<Vec<DetailIndicator>>>,
    art_cache: &ArtCache,
) {
    let text_sorter: fn(fn(&Track) -> String) -> gtk::CustomSorter = make_track_text_sorter;

    append_art_column(column_view, art_cache);
    append_track_number_column(column_view);
    build_title_column(column_view, current_path, indicators, text_sorter);

    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Artist"))
            .factory(&make_label_factory(|t| t.display_artist().to_string()))
            .sorter(&text_sorter(|t| t.display_artist().to_string()))
            .fixed_width(150)
            .resizable(true)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Album"))
            .factory(&make_label_factory(|t| t.display_album().to_string()))
            .sorter(&text_sorter(|t| t.display_album().to_string()))
            .fixed_width(150)
            .resizable(true)
            .visible(false)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Duration"))
            .factory(&make_duration_factory())
            .sorter(&make_duration_sorter())
            .fixed_width(80)
            .resizable(true)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Genre"))
            .factory(&make_label_factory(|t| {
                t.genre.as_deref().unwrap_or("").to_string()
            }))
            .sorter(&text_sorter(|t| {
                t.genre.as_deref().unwrap_or("").to_string()
            }))
            .fixed_width(100)
            .resizable(true)
            .visible(false)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Year"))
            .factory(&make_year_factory())
            .sorter(&make_year_sorter())
            .fixed_width(60)
            .resizable(true)
            .visible(false)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Bitrate"))
            .factory(&make_label_factory(|t| {
                t.bitrate
                    .map(|b| {
                        big_media_components::audio::labels::bitrate_kbps_label_from_bps(b.into())
                    })
                    .unwrap_or_default()
            }))
            .fixed_width(90)
            .resizable(true)
            .visible(false)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Format"))
            .factory(&make_label_factory(|t| {
                t.format.as_deref().unwrap_or("").to_string()
            }))
            .fixed_width(80)
            .resizable(true)
            .visible(false)
            .build(),
    );

    append_rating_column(column_view, sender);
    append_add_column(column_view, sender);
}

/// Album-art column: panel-owned LRU cache hit → set immediately; miss → extract
/// the embedded art off-thread, thumbnail it, set + cache. Paintable cleared on
/// unbind (recycled-row leak guard).
fn append_art_column(column_view: &gtk::ColumnView, art_cache: &ArtCache) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .width_request(36)
            .height_request(36)
            .overflow(gtk::Overflow::Hidden)
            .build();
        picture.add_css_class("album-art-cell");
        item.set_child(Some(&picture));
    });
    let cache = art_cache.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
        let track = obj.borrow::<Track>();
        let picture = item.child().and_downcast::<gtk::Picture>().unwrap();
        picture.set_paintable(gdk::Paintable::NONE);

        let hash = match track.art_hash {
            Some(ref h) if !h.is_empty() => h.clone(),
            _ => return,
        };
        if let Some(texture) = cache.borrow_mut().get(&hash) {
            picture.set_paintable(Some(texture));
            return;
        }
        let file_path = track.file_path.clone();
        let picture = picture.clone();
        let cache = cache.clone();
        big_media_components::player_chrome::spawn_blocking_result(
            move || crate::library::metadata::extract_album_art(&file_path),
            move |data| {
                let Some(data) = data else { return };
                if let Some(texture) =
                    big_media_components::media::images::bytes_to_thumbnail(&data, 36)
                {
                    picture.set_paintable(Some(&texture));
                    cache.borrow_mut().put(hash, texture);
                }
            },
        );
    });
    factory.connect_unbind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(picture) = item.child().and_downcast::<gtk::Picture>() {
            picture.set_paintable(gdk::Paintable::NONE);
        }
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title("")
            .factory(&factory)
            .fixed_width(44)
            .build(),
    );
}

/// Track-number column (from tag, not list position); hidden by default.
fn append_track_number_column(column_view: &gtk::ColumnView) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(4)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        item.set_child(Some(&label));
    });
    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
        let track = obj.borrow::<Track>();
        let label = item.child().and_downcast::<gtk::Label>().unwrap();
        label.set_label(
            &track
                .track_number
                .map(|n| n.to_string())
                .unwrap_or_default(),
        );
    });
    let sorter = gtk::CustomSorter::new(|a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        ta.track_number
            .unwrap_or(0)
            .cmp(&tb.track_number.unwrap_or(0))
            .into()
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title("#")
            .factory(&factory)
            .sorter(&sorter)
            .fixed_width(50)
            .resizable(true)
            .visible(false)
            .build(),
    );
}

/// Title column: now-playing speaker icon (visible when the row's file path is
/// the playing path) + file-path tooltip. Registers (path, icon, label) for
/// `SetPlayingPath` re-light; stashes `vis-pos` for right-click hit-test (L3).
fn build_title_column(
    column_view: &gtk::ColumnView,
    current_path: &Rc<RefCell<Option<String>>>,
    indicators: &Rc<RefCell<Vec<DetailIndicator>>>,
    text_sorter: fn(fn(&Track) -> String) -> gtk::CustomSorter,
) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let hbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        let icon = gtk::Image::builder()
            .icon_name("audio-volume-high-symbolic")
            .pixel_size(12)
            .visible(false)
            .build();
        icon.add_css_class("playing-indicator");
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .margin_start(4)
            .margin_end(4)
            .margin_top(4)
            .margin_bottom(4)
            .build();
        hbox.append(&icon);
        hbox.append(&label);
        item.set_child(Some(&hbox));
    });
    let cur_bind = current_path.clone();
    let ind_bind = indicators.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().unwrap();
        let track = obj
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let hbox = item.child().and_downcast::<gtk::Box>().unwrap();
        // SAFETY: read by the right-click hit-test (L3) to resolve a row index.
        unsafe {
            hbox.set_data("vis-pos", item.position());
        }
        big_relm4_components::feedback::tooltip::set(&hbox, &track.file_path);
        let Some(icon) = hbox.first_child().and_downcast::<gtk::Image>() else {
            return;
        };
        let Some(label) = hbox.last_child().and_downcast::<gtk::Label>() else {
            return;
        };
        label.set_label(track.display_title());
        let is_current = cur_bind.borrow().as_deref() == Some(track.file_path.as_str());
        icon.set_visible(is_current);
        ind_bind
            .borrow_mut()
            .push((track.file_path.clone(), icon, label));
    });
    let ind_unbind = indicators.clone();
    factory.connect_unbind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let hbox = item.child().and_downcast::<gtk::Box>().unwrap();
        if let Some(icon) = hbox.first_child().and_downcast::<gtk::Image>() {
            ind_unbind.borrow_mut().retain(|(_, img, _)| img != &icon);
        }
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Title"))
            .factory(&factory)
            .sorter(&text_sorter(|t| t.display_title().to_string()))
            .expand(true)
            .resizable(true)
            .build(),
    );
}

/// Rating column: 5 stars, click → optimistic update + `RateTrack` intent.
fn append_rating_column(column_view: &gtk::ColumnView, sender: &ComponentSender<LibraryPanel>) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let hbox = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(0)
            .halign(gtk::Align::Center)
            .build();
        for _ in 0..5 {
            hbox.append(
                &gtk::Image::builder()
                    .icon_name("non-starred-symbolic")
                    .pixel_size(14)
                    .build(),
            );
        }
        hbox.update_property(&[gtk::accessible::Property::Label(&i18n("Rating"))]);
        item.set_child(Some(&hbox));
    });
    let out = sender.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
        let track = obj.borrow::<Track>();
        let track_id = track.id;
        let hbox = item.child().and_downcast::<gtk::Box>().unwrap();
        update_star_icons(&hbox, track.rating);

        let hbox_ref = hbox.downgrade();
        let out = out.clone();
        let gesture = gtk::GestureClick::new();
        gesture.connect_released(move |gest, _, x, _| {
            let Some(hbox) = hbox_ref.upgrade() else {
                return;
            };
            let star_w = f64::from(hbox.width()) / 5.0;
            let clicked = ((x / star_w) as i32 + 1).clamp(1, 5);
            update_star_icons(&hbox, clicked);
            let _ = out.output(LibraryPanelOutput::RateTrack {
                id: track_id,
                rating: clicked,
            });
            gest.set_state(gtk::EventSequenceState::Claimed);
        });
        hbox.add_controller(gesture);
    });
    factory.connect_unbind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(hbox) = item.child().and_downcast::<gtk::Box>() {
            unbind_click_gestures(&hbox);
        }
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Rating"))
            .factory(&factory)
            .sorter(&make_rating_sorter())
            .fixed_width(100)
            .resizable(true)
            .visible(false)
            .build(),
    );
}

/// Trailing "add to playlist" button column → `ShowAddToPlaylist` intent.
fn append_add_column(column_view: &gtk::ColumnView, sender: &ComponentSender<LibraryPanel>) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let btn = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        btn.update_property(&[gtk::accessible::Property::Label(&i18n(
            "Add track to playlist",
        ))]);
        btn.set_opacity(0.0);
        item.set_child(Some(&btn));
    });
    let out = sender.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().and_downcast::<glib::BoxedAnyObject>().unwrap();
        let track = obj.borrow::<Track>().clone();
        let btn = item.child().and_downcast::<gtk::Button>().unwrap();
        let out = out.clone();
        let handler = btn.connect_clicked(move |_| {
            let _ = out.output(LibraryPanelOutput::ShowAddToPlaylist(track.clone()));
        });
        // SAFETY: stash the handler id so unbind disconnects the recycled row's
        // stale closure.
        unsafe {
            btn.set_data("handler-id", handler);
        }
    });
    factory.connect_unbind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(btn) = item.child().and_downcast::<gtk::Button>() {
            // SAFETY: matches the id stashed in bind above.
            unsafe {
                if let Some(id) = btn.steal_data::<glib::SignalHandlerId>("handler-id") {
                    btn.disconnect(id);
                }
            }
        }
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title("")
            .factory(&factory)
            .fixed_width(36)
            .resizable(false)
            .build(),
    );
}
