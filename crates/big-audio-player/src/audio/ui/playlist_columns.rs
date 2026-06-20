//! Queue `ColumnView` column builders for the `PlaylistPanel`.
//!
//! The default column set (#, Title+indicator, Artist, Album, Duration, Genre,
//! Year, Bitrate, Format, Rating, Remove). Interactive columns emit `Output`
//! intents; the title column hosts the object-identity now-playing indicator.

use adw::prelude::*;
use gtk::{glib, pango};
use relm4::ComponentSender;
use std::cell::RefCell;
use std::rc::Rc;

use crate::audio::column_builders::{
    make_duration_factory, make_duration_sorter, make_label_factory, make_rating_sorter,
    make_track_text_sorter, make_year_factory, make_year_sorter, unbind_click_gestures,
    update_star_icons,
};
use crate::db::models::Track;
use crate::i18n::i18n;

use super::playlist_panel::{PlaylistPanel, PlaylistPanelOutput, RowIndicator};

/// Build the default visible column set. Interactive columns (rating, remove)
/// emit `Output` intents; the title column hosts the object-identity indicator.
pub(super) fn build_columns(
    column_view: &gtk::ColumnView,
    indicators: &Rc<RefCell<Vec<RowIndicator>>>,
    current_obj: &Rc<RefCell<Option<glib::Object>>>,
    sender: &ComponentSender<PlaylistPanel>,
) {
    let text_sorter: fn(fn(&Track) -> String) -> gtk::CustomSorter = make_track_text_sorter;

    // # (position) — re-derived from list position, not stored.
    let num_factory = gtk::SignalListItemFactory::new();
    num_factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(4)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        item.set_child(Some(&label));
    });
    num_factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = item.child().and_downcast::<gtk::Label>().unwrap();
        label.set_label(&(item.position() + 1).to_string());
    });
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title("#")
            .factory(&num_factory)
            .fixed_width(40)
            .resizable(false)
            .build(),
    );

    build_title_column(column_view, indicators, current_obj, text_sorter);

    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Artist"))
            .factory(&make_label_factory(|t| t.display_artist().to_string()))
            .sorter(&text_sorter(|t| t.display_artist().to_string()))
            .fixed_width(160)
            .resizable(true)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Album"))
            .factory(&make_label_factory(|t| t.display_album().to_string()))
            .sorter(&text_sorter(|t| t.display_album().to_string()))
            .fixed_width(120)
            .resizable(true)
            .visible(false)
            .build(),
    );
    column_view.append_column(
        &gtk::ColumnViewColumn::builder()
            .title(i18n("Duration"))
            .factory(&make_duration_factory())
            .sorter(&make_duration_sorter())
            .fixed_width(65)
            .resizable(true)
            .build(),
    );

    // Extra columns, hidden by default (toggled via the column-config menu —
    // a later commit). Mirrors the god-object's column set.
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
            .sorter(&make_bitrate_sorter())
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
            .sorter(&text_sorter(|t| {
                t.format.as_deref().unwrap_or("").to_string()
            }))
            .fixed_width(80)
            .resizable(true)
            .visible(false)
            .build(),
    );

    build_rating_column(column_view, sender);
    build_remove_column(column_view, sender);
}

/// Title column: now-playing icon + accent label, registered for live re-light.
fn build_title_column(
    column_view: &gtk::ColumnView,
    indicators: &Rc<RefCell<Vec<RowIndicator>>>,
    current_obj: &Rc<RefCell<Option<glib::Object>>>,
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
            .icon_name("media-playback-start-symbolic")
            .pixel_size(16)
            .visible(false)
            .build();
        icon.add_css_class("playing-indicator");
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .margin_start(4)
            .margin_end(4)
            .margin_top(2)
            .margin_bottom(2)
            .build();
        hbox.append(&icon);
        hbox.append(&label);
        item.set_child(Some(&hbox));
    });
    let ind_bind = indicators.clone();
    let cur_bind = current_obj.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let obj = item.item().unwrap();
        let track = obj
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let hbox = item.child().and_downcast::<gtk::Box>().unwrap();
        // SAFETY: read only by playlist_geom::find_clicked_position (right-click
        // hit-test) so a context-menu click can resolve the visual row index.
        unsafe {
            hbox.set_data("vis-pos", item.position());
        }
        let Some(icon) = hbox.first_child().and_downcast::<gtk::Image>() else {
            return;
        };
        let Some(label) = hbox.last_child().and_downcast::<gtk::Label>() else {
            return;
        };
        label.set_label(track.display_title());
        // Object identity (not file_path) so duplicates highlight independently.
        let is_current = cur_bind.borrow().as_ref() == Some(&obj);
        icon.set_visible(is_current);
        if is_current {
            label.add_css_class("now-playing-accent");
        } else {
            label.remove_css_class("now-playing-accent");
        }
        ind_bind.borrow_mut().push((obj.clone(), icon, label));
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

/// Rating column: 5 stars, one click gesture per row → optimistic update + emit.
fn build_rating_column(column_view: &gtk::ColumnView, sender: &ComponentSender<PlaylistPanel>) {
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

        // One gesture per row; x-position picks the star (cheaper than 5 gestures).
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
            let _ = out.output(PlaylistPanelOutput::RateTrack {
                id: track_id,
                rating: clicked,
            });
            gest.set_state(gtk::EventSequenceState::Claimed);
        });
        hbox.add_controller(gesture);
    });
    // Drop gesture controllers on recycle; otherwise they accumulate per row.
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

/// Remove column: per-row delete button → emit `RemoveTracks` (shell mutates).
fn build_remove_column(column_view: &gtk::ColumnView, sender: &ComponentSender<PlaylistPanel>) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let btn = gtk::Button::builder()
            .icon_name("edit-delete-symbolic")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        btn.update_property(&[gtk::accessible::Property::Label(&i18n("Remove track"))]);
        btn.set_opacity(0.0);
        item.set_child(Some(&btn));
    });
    let out = sender.clone();
    factory.connect_bind(move |_, item| {
        let item = item.downcast_ref::<gtk::ListItem>().unwrap();
        let pos = item.position() as usize;
        let btn = item.child().and_downcast::<gtk::Button>().unwrap();
        let out = out.clone();
        let handler = btn.connect_clicked(move |_| {
            let _ = out.output(PlaylistPanelOutput::RemoveTracks { indices: vec![pos] });
        });
        // SAFETY: stash the handler id so unbind disconnects it; otherwise a
        // recycled button fires a closure bound to the wrong (old) row.
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
            .fixed_width(32)
            .resizable(false)
            .build(),
    );
}

/// Bitrate column sorter (numeric, missing → 0). Local because column_builders
/// has no bitrate helper.
fn make_bitrate_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        ta.bitrate.unwrap_or(0).cmp(&tb.bitrate.unwrap_or(0)).into()
    })
}
