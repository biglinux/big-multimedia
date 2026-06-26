//! `ColumnView` sorter + factory helpers for `Track` columns. The generic
//! `ColumnView` chrome (column-config header menu, width serialize/apply/persist)
//! moved to the shared `big_relm4_components::layout::{column_persist,
//! column_chrome}` (RESTRUCTURE P5); only the `Track`-specific sorters and cell
//! factories stay here.

use gtk::prelude::*;
use gtk::{glib, pango};

use crate::db::models::Track;

/// Case-insensitive text sort.
pub(super) fn make_track_text_sorter(get_key: fn(&Track) -> String) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        get_key(&ta)
            .to_lowercase()
            .cmp(&get_key(&tb).to_lowercase())
            .into()
    })
}

pub(super) fn make_year_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        ta.year.unwrap_or(0).cmp(&tb.year.unwrap_or(0)).into()
    })
}

/// NaN → Equal.
pub(super) fn make_duration_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        ta.duration_secs
            .partial_cmp(&tb.duration_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
            .into()
    })
}

pub(super) fn make_rating_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let ta = a
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        let tb = b
            .downcast_ref::<glib::BoxedAnyObject>()
            .unwrap()
            .borrow::<Track>();
        ta.rating.cmp(&tb.rating).into()
    })
}

/// Right-aligned numeric column; empty when year missing.
pub(super) fn make_year_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(4)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let boxed_track = list_item
            .item()
            .and_downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let track = boxed_track.borrow::<Track>();
        let label = list_item.child().and_downcast::<gtk::Label>().unwrap();
        if let Some(y) = track.year {
            label.set_label(&y.to_string());
        } else {
            label.set_label("");
        }
    });
    factory
}

pub(super) fn update_star_icons(hbox: &gtk::Box, rating: i32) {
    let mut child = hbox.first_child();
    let mut idx = 0_i32;
    while let Some(widget) = child {
        if let Some(img) = widget.downcast_ref::<gtk::Image>() {
            if idx < rating {
                img.set_icon_name(Some("starred-symbolic"));
            } else {
                img.set_icon_name(Some("non-starred-symbolic"));
            }
        }
        idx += 1;
        child = widget.next_sibling();
    }
}

/// Rating column unbind path — without this, gestures leak across recycled rows.
pub(super) fn unbind_click_gestures(widget: &impl IsA<gtk::Widget>) {
    let mut to_remove = Vec::new();
    let ctrl = widget.observe_controllers();
    for i in 0..ctrl.n_items() {
        if let Some(c) = ctrl.item(i) {
            if c.downcast_ref::<gtk::GestureClick>().is_some() {
                to_remove.push(c.downcast::<gtk::EventController>().unwrap());
            }
        }
    }
    for c in to_remove {
        widget.remove_controller(&c);
    }
}

/// Left-aligned, ellipsized text column.
pub(super) fn make_label_factory(get_text: fn(&Track) -> String) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::End)
            .margin_start(4)
            .margin_end(4)
            .margin_top(2)
            .margin_bottom(2)
            .build();
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let boxed_track = list_item
            .item()
            .and_downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let track = boxed_track.borrow::<Track>();
        let label = list_item.child().and_downcast::<gtk::Label>().unwrap();
        label.set_label(&get_text(&track));
    });
    factory
}

/// Right-aligned numeric column; empty when duration ≤ 0.
pub(super) fn make_duration_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(2)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(|_, list_item| {
        let list_item = list_item.downcast_ref::<gtk::ListItem>().unwrap();
        let boxed_track = list_item
            .item()
            .and_downcast::<glib::BoxedAnyObject>()
            .unwrap();
        let track = boxed_track.borrow::<Track>();
        let label = list_item.child().and_downcast::<gtk::Label>().unwrap();
        if track.duration_secs > 0.0 {
            label.set_label(&track.duration_display());
        } else {
            label.set_label("");
        }
    });
    factory
}
