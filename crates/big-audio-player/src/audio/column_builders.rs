//! `ColumnView` sorter + factory helpers for `Track` columns. The generic
//! `ColumnView` chrome (column-config header menu, width serialize/apply/persist)
//! moved to the shared `big_relm4_components::layout::{column_persist,
//! column_chrome}` (RESTRUCTURE P5); only the `Track`-specific sorters and cell
//! factories stay here.

use big_relm4_components::list::signal_factory;
use gtk::prelude::*;
use gtk::{glib, pango};
use std::cmp::Ordering;

use crate::db::models::Track;

const STARRED_ICON_NAME: &str = "starred-symbolic";
const NON_STARRED_ICON_NAME: &str = "non-starred-symbolic";

fn compare_case_insensitive_text(left_text: &str, right_text: &str) -> Ordering {
    left_text.to_lowercase().cmp(&right_text.to_lowercase())
}

fn compare_track_text_values(
    track_a: &Track,
    track_b: &Track,
    get_key: fn(&Track) -> String,
) -> Ordering {
    compare_case_insensitive_text(&get_key(track_a), &get_key(track_b))
}

fn compare_track_year_values(track_a: &Track, track_b: &Track) -> Ordering {
    track_a.year.unwrap_or(0).cmp(&track_b.year.unwrap_or(0))
}

fn compare_track_duration_values(track_a: &Track, track_b: &Track) -> Ordering {
    track_a
        .duration_secs
        .partial_cmp(&track_b.duration_secs)
        .unwrap_or(Ordering::Equal)
}

fn compare_track_rating_values(track_a: &Track, track_b: &Track) -> Ordering {
    track_a.rating.cmp(&track_b.rating)
}

fn year_cell_label(year: Option<i32>) -> String {
    year.map_or_else(String::new, |year| year.to_string())
}

fn duration_cell_label(track: &Track) -> String {
    if track.duration_secs > 0.0 {
        track.duration_display()
    } else {
        String::new()
    }
}

fn star_icon_name_for_position(star_position: usize, rating: i32) -> &'static str {
    if (star_position as i32) < rating {
        STARRED_ICON_NAME
    } else {
        NON_STARRED_ICON_NAME
    }
}

fn star_icon_names_for_rating(rating: i32, star_count: usize) -> Vec<&'static str> {
    (0..star_count)
        .map(|star_position| star_icon_name_for_position(star_position, rating))
        .collect()
}

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
        compare_track_text_values(&track_a, &track_b, get_key).into()
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
        compare_track_year_values(&track_a, &track_b).into()
    })
}

/// NaN sorts as equal.
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
        compare_track_duration_values(&track_a, &track_b).into()
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
        compare_track_rating_values(&track_a, &track_b).into()
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
        label.set_label(&year_cell_label(track.year));
    });
    factory
}

pub(super) fn update_star_icons(hbox: &gtk::Box, rating: i32) {
    let star_icon_names =
        star_icon_names_for_rating(rating, hbox.observe_children().n_items() as usize);
    let mut star_icon_names_iter = star_icon_names.into_iter();
    let mut child = hbox.first_child();
    while let Some(widget) = child {
        let icon_name = star_icon_names_iter.next().unwrap_or(NON_STARRED_ICON_NAME);
        if let Some(img) = widget.downcast_ref::<gtk::Image>() {
            img.set_icon_name(Some(icon_name));
        }
        child = widget.next_sibling();
    }
}

/// Rating column unbind path: without this, gestures leak across recycled rows.
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

/// Right-aligned numeric column; empty when duration <= 0.
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
        label.set_label(&duration_cell_label(&track));
    });
    factory
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_with_sort_values(
        title: &str,
        year: Option<i32>,
        duration_secs: f64,
        rating: i32,
    ) -> Track {
        let mut track = Track::new_external_path_with_title(
            format!("/music/{title}.flac"),
            Some(title.to_string()),
        );
        track.year = year;
        track.duration_secs = duration_secs;
        track.rating = rating;
        track
    }

    #[test]
    fn text_comparison_is_case_insensitive() {
        assert_eq!(
            compare_case_insensitive_text("alpha", "Bravo"),
            Ordering::Less
        );
        assert_eq!(
            compare_case_insensitive_text("ALBUM", "album"),
            Ordering::Equal
        );
        assert_eq!(
            compare_case_insensitive_text("Zulu", "bravo"),
            Ordering::Greater
        );
    }

    #[test]
    fn track_text_comparison_uses_requested_key() {
        let mut earlier_track = track_with_sort_values("b-title", Some(2020), 1.0, 1);
        earlier_track.artist = Some("Alpha".to_string());
        let mut later_track = track_with_sort_values("a-title", Some(2020), 1.0, 1);
        later_track.artist = Some("bravo".to_string());

        assert_eq!(
            compare_track_text_values(&earlier_track, &later_track, |track| track
                .artist
                .clone()
                .unwrap_or_default()),
            Ordering::Less
        );
    }

    #[test]
    fn numeric_track_comparisons_use_visible_column_values() {
        let older_track = track_with_sort_values("older", Some(1999), 61.0, 2);
        let newer_track = track_with_sort_values("newer", Some(2024), 125.0, 5);
        let unknown_year_track = track_with_sort_values("unknown", None, f64::NAN, 0);

        assert_eq!(
            compare_track_year_values(&older_track, &newer_track),
            Ordering::Less
        );
        assert_eq!(
            compare_track_year_values(&unknown_year_track, &older_track),
            Ordering::Less
        );
        assert_eq!(
            compare_track_duration_values(&newer_track, &older_track),
            Ordering::Greater
        );
        assert_eq!(
            compare_track_duration_values(&unknown_year_track, &older_track),
            Ordering::Equal
        );
        assert_eq!(
            compare_track_rating_values(&older_track, &newer_track),
            Ordering::Less
        );
    }

    #[test]
    fn year_cell_label_is_blank_only_for_missing_year() {
        assert_eq!(year_cell_label(Some(2024)), "2024");
        assert_eq!(year_cell_label(None), "");
    }

    #[test]
    fn duration_cell_label_is_blank_only_for_non_positive_duration() {
        let zero_duration_track = track_with_sort_values("zero", Some(2024), 0.0, 0);
        let negative_duration_track = track_with_sort_values("negative", Some(2024), -1.0, 0);
        let positive_duration_track = track_with_sort_values("positive", Some(2024), 65.0, 0);

        assert_eq!(duration_cell_label(&zero_duration_track), "");
        assert_eq!(duration_cell_label(&negative_duration_track), "");
        assert_eq!(duration_cell_label(&positive_duration_track), "1:05");
    }

    #[test]
    fn star_icon_names_follow_rating_threshold() {
        assert_eq!(
            star_icon_names_for_rating(3, 5),
            vec![
                STARRED_ICON_NAME,
                STARRED_ICON_NAME,
                STARRED_ICON_NAME,
                NON_STARRED_ICON_NAME,
                NON_STARRED_ICON_NAME,
            ]
        );
        assert_eq!(
            star_icon_names_for_rating(0, 2),
            vec![NON_STARRED_ICON_NAME, NON_STARRED_ICON_NAME]
        );
        assert_eq!(
            star_icon_names_for_rating(7, 2),
            vec![STARRED_ICON_NAME, STARRED_ICON_NAME]
        );
        assert_eq!(
            star_icon_names_for_rating(-1, 2),
            vec![NON_STARRED_ICON_NAME, NON_STARRED_ICON_NAME]
        );
    }
}
