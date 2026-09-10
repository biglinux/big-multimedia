//! Cairo draw handler for the audio waveform — zoom-windowed min/max
//! samples, segment overlays, hover delete-button, playhead and time labels.

use std::f64::consts::TAU;

use gtk::prelude::*;

use super::build::AudioEditPageBuild;
use super::segments::{delete_button_circle, draw_time_label, format_time};

#[allow(clippy::too_many_lines)] // GTK draw-function wiring — long by nature
pub(super) fn install_waveform_renderer(build: &AudioEditPageBuild) {
    let duration = build.page.duration.clone();
    let position = build.page.position.clone();
    let waveform_samples = build.page.waveform_data.clone();
    let segments = build.page.segments.clone();
    let hover_segment = build.page.hover_segment.clone();
    let hover_delete_button = build.page.hover_delete_button.clone();
    let pending_mark = build.page.pending_mark.clone();
    let zoom_level = build.page.zoom_level.clone();

    build.page.waveform.set_draw_func(
        move |_area, cr: &gtk::cairo::Context, width: i32, height: i32| {
            let width_f = f64::from(width.max(1));
            let height_f = f64::from(height.max(1));

            cr.set_source_rgb(0.09, 0.10, 0.12);
            let _ = cr.paint();

            if let Some(samples) = waveform_samples.borrow().as_ref() {
                cr.set_source_rgb(0.30, 0.65, 1.0);
                cr.set_line_width(1.0);
                let mid = height_f / 2.0;
                let total = samples.len();
                let zoom = zoom_level.get().max(1.0);
                let view_len = (total as f64 / zoom).max(1.0) as usize;
                let center = {
                    let pos = position.get();
                    let dur = duration.get();
                    if dur > 0.0 {
                        ((pos / dur) * total as f64) as usize
                    } else {
                        total / 2
                    }
                };
                let half = view_len / 2;
                let view_start = center
                    .saturating_sub(half)
                    .min(total.saturating_sub(view_len));
                let view_end = (view_start + view_len).min(total);
                let view_samples = &samples[view_start..view_end];
                let stride = ((view_samples.len() as f64 / width_f).ceil() as usize).max(1);
                for x in 0..width {
                    let start = x as usize * stride;
                    if start >= view_samples.len() {
                        break;
                    }
                    let end = (start + stride).min(view_samples.len());
                    let slice = &view_samples[start..end];
                    let mut min = 1.0_f32;
                    let mut max = -1.0_f32;
                    for sample in slice {
                        min = min.min(*sample);
                        max = max.max(*sample);
                    }
                    let y1 = mid - f64::from(max) * (height_f * 0.45);
                    let y2 = mid - f64::from(min) * (height_f * 0.45);
                    cr.move_to(f64::from(x), y1);
                    cr.line_to(f64::from(x), y2);
                }
                let _ = cr.stroke();
            } else {
                cr.set_source_rgb(0.7, 0.7, 0.7);
                cr.move_to(0.0, height_f / 2.0);
                cr.line_to(width_f, height_f / 2.0);
                let _ = cr.stroke();
            }

            let dur = duration.get();
            if dur <= 0.0 {
                return;
            }

            let hovered = hover_segment.get();
            for (idx, seg) in segments.borrow().iter().enumerate() {
                let x1 = (seg.start / dur).clamp(0.0, 1.0) * width_f;
                let x2 = (seg.end / dur).clamp(0.0, 1.0) * width_f;

                let is_hover = hovered == Some(idx);
                cr.set_source_rgba(0.20, 0.60, 1.00, if is_hover { 0.40 } else { 0.28 });
                cr.rectangle(x1, 0.0, (x2 - x1).max(2.0), height_f);
                let _ = cr.fill();

                cr.set_line_width(if is_hover { 2.0 } else { 1.0 });
                cr.set_source_rgba(0.85, 0.20, 0.20, 0.95);
                cr.move_to(x1, 0.0);
                cr.line_to(x1, height_f);
                let _ = cr.stroke();

                cr.set_source_rgba(0.20, 0.82, 0.35, 0.95);
                cr.move_to(x2, 0.0);
                cr.line_to(x2, height_f);
                let _ = cr.stroke();

                let cy = height_f * 0.5;
                let r = if is_hover { 5.0 } else { 4.0 };
                cr.set_source_rgba(0.85, 0.20, 0.20, 1.0);
                cr.arc(x1, cy, r, 0.0, TAU);
                let _ = cr.fill();
                cr.set_source_rgba(0.20, 0.82, 0.35, 1.0);
                cr.arc(x2, cy, r, 0.0, TAU);
                let _ = cr.fill();

                if is_hover {
                    cr.set_source_rgba(1.0, 0.85, 0.10, 0.8);
                    cr.set_line_width(1.5);
                    cr.rectangle(x1 + 0.5, 0.5, (x2 - x1 - 1.0).max(1.0), height_f - 1.0);
                    let _ = cr.stroke();

                    let (bx, by, br) = delete_button_circle(seg, width_f, dur, height_f);
                    cr.set_source_rgba(
                        0.90,
                        0.20,
                        0.20,
                        if hover_delete_button.get() {
                            0.95
                        } else {
                            0.78
                        },
                    );
                    cr.arc(bx, by, br, 0.0, TAU);
                    let _ = cr.fill();
                    cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
                    cr.set_line_width(1.5);
                    cr.move_to(bx - 3.5, by - 3.5);
                    cr.line_to(bx + 3.5, by + 3.5);
                    let _ = cr.stroke();
                    cr.move_to(bx + 3.5, by - 3.5);
                    cr.line_to(bx - 3.5, by + 3.5);
                    let _ = cr.stroke();
                }

                draw_time_label(cr, x1, 4.0, &format_time(seg.start));
                draw_time_label(cr, x2, 22.0, &format_time(seg.end));
            }

            if let Some(mark) = pending_mark.get() {
                let x = (mark / dur).clamp(0.0, 1.0) * width_f;
                cr.set_source_rgb(1.0, 0.75, 0.2);
                cr.move_to(x, 0.0);
                cr.line_to(x, height_f);
                let _ = cr.stroke();
            }

            let x = (position.get() / dur).clamp(0.0, 1.0) * width_f;
            cr.set_source_rgb(1.0, 0.5, 0.0);
            cr.move_to(x, 0.0);
            cr.line_to(x, height_f);
            let _ = cr.stroke();

            let pos = position.get();
            let pos_str = format_time(pos);
            let rem_str = format!("-{}", format_time(dur - pos));
            cr.set_font_size(10.5);
            let pad_x = 6.0;
            let pad_y = height_f - 5.0;
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
            cr.move_to(pad_x + 1.0, pad_y + 1.0);
            let _ = cr.show_text(&pos_str);
            cr.set_source_rgba(0.9, 0.9, 0.9, 1.0);
            cr.move_to(pad_x, pad_y);
            let _ = cr.show_text(&pos_str);
            let rem_ext = cr.text_extents(&rem_str).ok();
            let rem_w = rem_ext.map_or((rem_str.len() as f64) * 6.5, |e| e.width());
            let rx = width_f - rem_w - pad_x;
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.6);
            cr.move_to(rx + 1.0, pad_y + 1.0);
            let _ = cr.show_text(&rem_str);
            cr.set_source_rgba(0.9, 0.9, 0.9, 1.0);
            cr.move_to(rx, pad_y);
            let _ = cr.show_text(&rem_str);
        },
    );
}
