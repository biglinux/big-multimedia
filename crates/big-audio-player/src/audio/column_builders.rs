//! `ColumnView` sorter + factory helpers for `Track` columns. The generic
//! `ColumnView` chrome (column-config header menu, width serialize/apply/persist)
//! moved to the shared `big_relm4_components::layout::{column_persist,
//! column_chrome}` (RESTRUCTURE P5); only the `Track`-specific sorters and cell
//! factories stay here.

use big_relm4_components::list::signal_factory;
use gtk::prelude::*;
use gtk::{glib, pango};

use crate::db::models::Track;

/// Case-insensitive text sort.
pub(super) fn make_track_text_sorter(get_key: fn(&Track) -> String) -> gtk::CustomSorter {
    gtk::CustomSorter::new(move |a, b| {
        let Some(track_a_object) = a.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let Some(track_b_object) = b.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let track_a = track_a_object.borrow::<Track>();
        let track_b = track_b_object.borrow::<Track>();
        get_key(&track_a)
            .to_lowercase()
            .cmp(&get_key(&track_b).to_lowercase())
            .into()
    })
}

pub(super) fn make_year_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let Some(track_a_object) = a.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let Some(track_b_object) = b.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let track_a = track_a_object.borrow::<Track>();
        let track_b = track_b_object.borrow::<Track>();
        track_a
            .year
            .unwrap_or(0)
            .cmp(&track_b.year.unwrap_or(0))
            .into()
    })
}

/// NaN → Equal.
pub(super) fn make_duration_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let Some(track_a_object) = a.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let Some(track_b_object) = b.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let track_a = track_a_object.borrow::<Track>();
        let track_b = track_b_object.borrow::<Track>();
        track_a
            .duration_secs
            .partial_cmp(&track_b.duration_secs)
            .unwrap_or(std::cmp::Ordering::Equal)
            .into()
    })
}

pub(super) fn make_rating_sorter() -> gtk::CustomSorter {
    gtk::CustomSorter::new(|a, b| {
        let Some(track_a_object) = a.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let Some(track_b_object) = b.downcast_ref::<glib::BoxedAnyObject>() else {
            return std::cmp::Ordering::Equal.into();
        };
        let track_a = track_a_object.borrow::<Track>();
        let track_b = track_b_object.borrow::<Track>();
        track_a.rating.cmp(&track_b.rating).into()
    })
}

/// Right-aligned numeric column; empty when year missing.
pub(super) fn make_year_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(4)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(boxed_track) = signal_factory::bound_model_as::<glib::BoxedAnyObject>(list_item)
        else {
            return;
        };
        let track = boxed_track.borrow::<Track>();
        let Some(label) = signal_factory::child_as::<gtk::Label>(list_item) else {
            return;
        };
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
                if let Ok(controller) = c.downcast::<gtk::EventController>() {
                    to_remove.push(controller);
                }
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
    factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
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
    factory.connect_bind(move |_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(boxed_track) = signal_factory::bound_model_as::<glib::BoxedAnyObject>(list_item)
        else {
            return;
        };
        let track = boxed_track.borrow::<Track>();
        let Some(label) = signal_factory::child_as::<gtk::Label>(list_item) else {
            return;
        };
        label.set_label(&get_text(&track));
    });
    factory
}

/// Right-aligned numeric column; empty when duration ≤ 0.
pub(super) fn make_duration_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let label = gtk::Label::builder()
            .xalign(1.0)
            .margin_start(2)
            .margin_end(4)
            .build();
        label.add_css_class("numeric");
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(boxed_track) = signal_factory::bound_model_as::<glib::BoxedAnyObject>(list_item)
        else {
            return;
        };
        let track = boxed_track.borrow::<Track>();
        let Some(label) = signal_factory::child_as::<gtk::Label>(list_item) else {
            return;
        };
        if track.duration_secs > 0.0 {
            label.set_label(&track.duration_display());
        } else {
            label.set_label("");
        }
    });
    factory
}
