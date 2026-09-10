//! Audio waveform segment math + list refresh: pixel-space hit-tests for
//! markers / bodies / per-segment delete circles, segments→TrimRange
//! conversion, and the right-side list builder mirroring the video edit
//! page. Hit-test tolerance lives in pixels because users drag at screen
//! resolution, not seconds.

use adw::prelude::*;
use big_relm4_components::list::{
    bounded_list_box::clear_list_box_rows,
    button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec},
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::converter::ffmpeg::TrimRange;
use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;

use super::AudioSegment;

#[must_use]
pub(super) fn segments_to_trim_ranges(segments: &[AudioSegment]) -> Vec<TrimRange> {
    segments
        .iter()
        .map(|segment| TrimRange {
            start: Some(segment.start),
            end: Some(segment.end),
        })
        .collect()
}

#[must_use]
pub(super) fn x_to_time(x: f64, width: f64, duration: f64) -> f64 {
    if width <= 0.0 || duration <= 0.0 {
        return 0.0;
    }
    (x / width).clamp(0.0, 1.0) * duration
}

#[must_use]
pub(super) fn hit_test_marker(
    segments: &[AudioSegment],
    x: f64,
    width: f64,
    duration: f64,
    tolerance_px: f64,
) -> Option<(usize, bool)> {
    for (idx, segment) in segments.iter().enumerate() {
        let x1 = (segment.start / duration).clamp(0.0, 1.0) * width;
        let x2 = (segment.end / duration).clamp(0.0, 1.0) * width;
        if (x - x1).abs() <= tolerance_px {
            return Some((idx, true));
        }
        if (x - x2).abs() <= tolerance_px {
            return Some((idx, false));
        }
    }
    None
}

#[must_use]
pub(super) fn hit_test_segment_body(
    segments: &[AudioSegment],
    x: f64,
    width: f64,
    duration: f64,
    edge_margin_px: f64,
) -> Option<usize> {
    for (idx, segment) in segments.iter().enumerate() {
        let x1 = (segment.start / duration).clamp(0.0, 1.0) * width;
        let x2 = (segment.end / duration).clamp(0.0, 1.0) * width;
        if x >= x1 + edge_margin_px && x <= x2 - edge_margin_px {
            return Some(idx);
        }
    }
    None
}

#[must_use]
pub(super) fn hit_test_segment_any(
    segments: &[AudioSegment],
    x: f64,
    width: f64,
    duration: f64,
    tolerance_px: f64,
) -> Option<usize> {
    if let Some((idx, _)) = hit_test_marker(segments, x, width, duration, tolerance_px) {
        return Some(idx);
    }
    hit_test_segment_body(segments, x, width, duration, tolerance_px)
}

#[must_use]
pub(super) fn delete_button_circle(
    segment: &AudioSegment,
    width: f64,
    duration: f64,
    height: f64,
) -> (f64, f64, f64) {
    let x2 = (segment.end / duration).clamp(0.0, 1.0) * width;
    let radius = 9.0;
    let cx = (x2 - radius - 6.0).clamp(radius + 2.0, width - radius - 2.0);
    let cy = (height - radius - 6.0).max(radius + 2.0);
    (cx, cy, radius)
}

#[must_use]
pub(super) fn hit_test_delete_button(
    segment: &AudioSegment,
    x: f64,
    width: f64,
    duration: f64,
    height: f64,
) -> bool {
    let (cx, _cy, radius) = delete_button_circle(segment, width, duration, height);
    (x - cx).abs() <= (radius + 3.0)
}

pub(super) fn draw_time_label(cr: &gtk::cairo::Context, x_center: f64, y: f64, text: &str) {
    cr.set_font_size(10.0);
    let ext = cr.text_extents(text).ok();
    let text_w = ext.map_or((text.len() as f64) * 6.0, |e| e.width());
    let box_x = x_center - (text_w / 2.0) - 4.0;
    let box_w = text_w + 8.0;
    let box_h = 14.0;

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.72);
    cr.rectangle(box_x, y, box_w, box_h);
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.92);
    cr.move_to(box_x + 4.0, y + 10.5);
    let _ = cr.show_text(text);
}

pub(super) fn seek_next_or_restart_segment(
    mpv: &Rc<RefCell<Option<MpvBackend>>>,
    next_segment_start: Option<f64>,
    first_segment_start: f64,
) {
    let binding = mpv.borrow();
    let Some(player) = binding.as_ref() else {
        return;
    };

    if let Some(next_start) = next_segment_start {
        player.seek_absolute(next_start);
    } else {
        player.pause();
        player.seek_absolute(first_segment_start);
    }
}

pub(super) fn refresh_segments_list(
    list: &gtk::ListBox,
    segments: &Rc<RefCell<Vec<AudioSegment>>>,
    placeholder: &adw::ActionRow,
    row: &adw::ActionRow,
    mpv: Rc<RefCell<Option<MpvBackend>>>,
    waveform: gtk::DrawingArea,
) {
    clear_list_box_rows(list);
    if segments.borrow().is_empty() {
        placeholder.set_visible(true);
        row.set_visible(false);
        return;
    }
    placeholder.set_visible(false);
    row.set_visible(true);

    for (idx, segment) in segments.borrow().iter().enumerate() {
        let segment_row_builder = BigButtonStripRow::new(
            BigButtonStripRowSpec::new(format!(
                "{} → {}",
                format_time(segment.start),
                format_time(segment.end)
            ))
            .subtitle(format_time(segment.end - segment.start)),
        );

        let goto_button = gtk::Button::builder()
            .icon_name("media-playback-start-symbolic")
            .css_classes(["flat", "circular"])
            .build();
        crate::shared::tooltip::set(&goto_button, &i18n("Go to segment"));
        goto_button.update_property(&[gtk::accessible::Property::Label(&i18n("Go to segment"))]);
        {
            let mpv = mpv.clone();
            let start = segment.start;
            goto_button.connect_clicked(move |_| {
                if let Some(ref player) = *mpv.borrow() {
                    player.seek_absolute(start);
                }
            });
        }
        segment_row_builder.append(&goto_button);

        let remove_button = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .css_classes(["flat", "circular"])
            .build();
        crate::shared::tooltip::set(&remove_button, &i18n("Remove segment"));
        remove_button.update_property(&[gtk::accessible::Property::Label(&i18n("Remove segment"))]);
        {
            // Weak widget captures: this closure lives on a button INSIDE
            // `list` — strong list/row/waveform refs here form a
            // rows⇄list⇄waveform island that outlives the page on close.
            let list = list.downgrade();
            let placeholder = placeholder.downgrade();
            let row = row.downgrade();
            let mpv = mpv.clone();
            let waveform = waveform.downgrade();
            let segments_ref = segments.clone();
            remove_button.connect_clicked(move |_| {
                let (Some(list), Some(placeholder), Some(row), Some(waveform)) = (
                    list.upgrade(),
                    placeholder.upgrade(),
                    row.upgrade(),
                    waveform.upgrade(),
                ) else {
                    return;
                };
                let mut owned = segments_ref.borrow_mut();
                if idx < owned.len() {
                    owned.remove(idx);
                }
                drop(owned);
                refresh_segments_list(
                    &list,
                    &segments_ref,
                    &placeholder,
                    &row,
                    mpv.clone(),
                    waveform.clone(),
                );
                waveform.queue_draw();
            });
        }
        segment_row_builder.append(&remove_button);
        let segment_row = segment_row_builder.into_root();
        list.append(&segment_row);
    }
}

#[must_use]
pub(super) fn format_time(secs: f64) -> String {
    let total = secs.max(0.0);
    let mins = (total / 60.0).floor() as u64;
    let rem = total % 60.0;
    if mins >= 60 {
        let hours = mins / 60;
        let mins = mins % 60;
        format!("{hours}:{mins:02}:{rem:05.2}")
    } else {
        format!("{mins}:{rem:05.2}")
    }
}
