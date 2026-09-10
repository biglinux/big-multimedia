//! Audio waveform interactions: click (mark/seek/delete), drag (handle/body
//! with 8 px threshold and 0.1 s minimum), motion (cursor + delete-button
//! hover). Cut mode uses crosshair/w-resize/e-resize/move/pointer.

use gtk::prelude::*;

use super::build::AudioEditPageBuild;
use super::render::install_waveform_renderer;
use super::segments::{
    hit_test_delete_button, hit_test_marker, hit_test_segment_any, hit_test_segment_body,
    refresh_segments_list, segments_to_trim_ranges, x_to_time,
};
use super::{AudioSegment, DragMode, fire};

pub(super) fn install_waveform_behavior(build: &AudioEditPageBuild) {
    install_waveform_click_controller(build);
    install_waveform_drag_controller(build);
    install_waveform_motion_controller(build);
    install_waveform_renderer(build);
}

fn install_waveform_click_controller(build: &AudioEditPageBuild) {
    let duration = build.page.duration.clone();
    let position = build.page.position.clone();
    let segments = build.page.segments.clone();
    let list = build.page.segments_list.clone();
    let placeholder = build.page.segments_placeholder.clone();
    let row = build.page.segments_row.clone();
    // Weak: this gesture lives ON the waveform — a strong capture is a
    // widget⇄gesture cycle (the rows it refreshes also point back here).
    let waveform_weak = build.page.waveform.downgrade();
    let on_apply_trim = build.page.on_apply_trim.clone();
    let hover_segment = build.page.hover_segment.clone();
    let hover_delete_button = build.page.hover_delete_button.clone();
    let drag_mode = build.page.drag_mode.clone();
    let pending_mark_click = build.page.pending_mark.clone();
    let cut_mode_click = build.page.cut_mode_active.clone();
    let mpv = build.page.mpv.clone();

    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_pressed(move |gesture, _n_press, x, _y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let width = f64::from(widget.width());
        let height = f64::from(widget.height());
        let duration = duration.get();
        if width <= 0.0 || duration <= 0.0 {
            return;
        }

        if let Some(idx) = hover_segment.get() {
            let borrowed = segments.borrow();
            if idx < borrowed.len()
                && hit_test_delete_button(&borrowed[idx], x, width, duration, height)
            {
                drop(borrowed);
                if idx < segments.borrow().len() {
                    segments.borrow_mut().remove(idx);
                    let Some(waveform) = waveform_weak.upgrade() else {
                        return;
                    };
                    refresh_segments_list(
                        &list,
                        &segments,
                        &placeholder,
                        &row,
                        mpv.clone(),
                        waveform,
                    );
                    fire(&on_apply_trim, segments_to_trim_ranges(&segments.borrow()));
                    hover_segment.set(None);
                    hover_delete_button.set(false);
                    widget.queue_draw();
                }
                return;
            }
        }

        let borrowed = segments.borrow();
        if hit_test_marker(&borrowed, x, width, duration, 6.0).is_some()
            || hit_test_segment_body(&borrowed, x, width, duration, 8.0).is_some()
        {
            return;
        }
        drop(borrowed);

        if !matches!(&*drag_mode.borrow(), DragMode::None) {
            return;
        }

        let click_time = (x / width).clamp(0.0, 1.0) * duration;
        if cut_mode_click.get() {
            if let Some(start) = pending_mark_click.get() {
                let (s, e) = if start <= click_time {
                    (start, click_time)
                } else {
                    (click_time, start)
                };
                if (e - s) >= 0.1 {
                    segments
                        .borrow_mut()
                        .push(AudioSegment { start: s, end: e });
                    let Some(waveform) = waveform_weak.upgrade() else {
                        return;
                    };
                    refresh_segments_list(
                        &list,
                        &segments,
                        &placeholder,
                        &row,
                        mpv.clone(),
                        waveform,
                    );
                    fire(&on_apply_trim, segments_to_trim_ranges(&segments.borrow()));
                }
                pending_mark_click.set(None);
            } else {
                pending_mark_click.set(Some(click_time));
                position.set(click_time);
                if let Some(ref player) = *mpv.borrow() {
                    player.seek_absolute(click_time);
                }
            }
        } else {
            position.set(click_time);
            if let Some(ref player) = *mpv.borrow() {
                player.seek_absolute(click_time);
            }
        }
        widget.queue_draw();
    });
    build.page.waveform.add_controller(click);
}

#[allow(clippy::too_many_lines)] // GTK drag-controller wiring — long by nature
fn install_waveform_drag_controller(build: &AudioEditPageBuild) {
    let duration = build.page.duration.clone();
    let segments = build.page.segments.clone();
    let drag_mode = build.page.drag_mode.clone();
    let position = build.page.position.clone();
    let mpv = build.page.mpv.clone();
    let on_apply_trim = build.page.on_apply_trim.clone();

    let drag = gtk::GestureDrag::new();
    {
        let duration = duration.clone();
        let segments = segments.clone();
        let drag_mode = drag_mode.clone();
        // Gesture lives ON the waveform: read it through the gesture's own
        // widget — a strong capture here is a widget⇄gesture cycle.
        drag.connect_drag_begin(move |gesture, start_x, _start_y| {
            let duration = duration.get();
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = f64::from(widget.width());
            if duration <= 0.0 || width <= 0.0 {
                *drag_mode.borrow_mut() = DragMode::None;
                return;
            }
            let borrowed = segments.borrow();
            if let Some((idx, is_start)) = hit_test_marker(&borrowed, start_x, width, duration, 8.0)
            {
                *drag_mode.borrow_mut() = if is_start {
                    DragMode::StartHandle { idx }
                } else {
                    DragMode::EndHandle { idx }
                };
                return;
            }
            if let Some(idx) = hit_test_segment_body(&borrowed, start_x, width, duration, 8.0) {
                let seg = &borrowed[idx];
                let anchor_time = x_to_time(start_x, width, duration);
                *drag_mode.borrow_mut() = DragMode::SegmentBody {
                    idx,
                    anchor_time,
                    orig_start: seg.start,
                    orig_end: seg.end,
                };
                return;
            }
            *drag_mode.borrow_mut() = DragMode::None;
        });
    }

    {
        let duration = duration.clone();
        let segments = segments.clone();
        let drag_mode = drag_mode.clone();
        let position = position.clone();
        let mpv = mpv.clone();
        let on_apply_trim = on_apply_trim.clone();
        drag.connect_drag_update(move |gesture, offset_x, _offset_y| {
            let duration = duration.get();
            let Some(widget) = gesture.widget() else {
                return;
            };
            let width = f64::from(widget.width());
            if duration <= 0.0 || width <= 0.0 {
                return;
            }
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let current_x = start_x + offset_x;
            let current_time = x_to_time(current_x, width, duration);

            let mut changed = false;
            {
                let mut segs = segments.borrow_mut();
                match &*drag_mode.borrow() {
                    DragMode::StartHandle { idx } if *idx < segs.len() => {
                        let end = segs[*idx].end;
                        segs[*idx].start = current_time.min(end - 0.1).max(0.0);
                        changed = true;
                    }
                    DragMode::EndHandle { idx } if *idx < segs.len() => {
                        let start = segs[*idx].start;
                        segs[*idx].end = current_time.max(start + 0.1).min(duration);
                        changed = true;
                    }
                    DragMode::SegmentBody {
                        idx,
                        anchor_time,
                        orig_start,
                        orig_end,
                    } if *idx < segs.len() => {
                        let delta = current_time - *anchor_time;
                        let len = (orig_end - orig_start).max(0.1);
                        let mut new_start = orig_start + delta;
                        let mut new_end = orig_end + delta;
                        if new_start < 0.0 {
                            new_start = 0.0;
                            new_end = len;
                        }
                        if new_end > duration {
                            new_end = duration;
                            new_start = (duration - len).max(0.0);
                        }
                        segs[*idx].start = new_start;
                        segs[*idx].end = new_end;
                        changed = true;
                    }
                    _ => {}
                }
            }

            if changed {
                position.set(current_time);
                if let Some(ref player) = *mpv.borrow() {
                    player.seek_absolute(current_time);
                }
                fire(&on_apply_trim, segments_to_trim_ranges(&segments.borrow()));
                widget.queue_draw();
            }
        });
    }

    {
        let drag_mode = drag_mode.clone();
        drag.connect_drag_end(move |gesture, _x, _y| {
            *drag_mode.borrow_mut() = DragMode::None;
            if let Some(widget) = gesture.widget() {
                widget.queue_draw();
            }
        });
    }

    build.page.waveform.add_controller(drag);
}

fn install_waveform_motion_controller(build: &AudioEditPageBuild) {
    let duration = build.page.duration.clone();
    let segments = build.page.segments.clone();
    let hover_segment = build.page.hover_segment.clone();
    let hover_delete_button = build.page.hover_delete_button.clone();
    let cut_mode = build.page.cut_mode_active.clone();

    let motion = gtk::EventControllerMotion::new();
    motion.connect_motion(move |controller, x, _y| {
        let Some(widget) = controller.widget() else {
            return;
        };
        let width = f64::from(widget.width());
        let height = f64::from(widget.height());
        let duration = duration.get();
        if width <= 0.0 || duration <= 0.0 {
            hover_segment.set(None);
            hover_delete_button.set(false);
            widget.queue_draw();
            return;
        }
        let segs = segments.borrow();
        let hit = hit_test_segment_any(&segs, x, width, duration, 8.0);
        hover_segment.set(hit);
        if let Some(idx) = hit {
            if idx < segs.len() {
                hover_delete_button.set(hit_test_delete_button(
                    &segs[idx], x, width, duration, height,
                ));
            } else {
                hover_delete_button.set(false);
            }
        } else {
            hover_delete_button.set(false);
        }
        let cursor_name: Option<&str> = if cut_mode.get() {
            let is_delete = hover_delete_button.get();
            let marker_hit = hit_test_marker(&segs, x, width, duration, 8.0);
            let body_hit = hit_test_segment_body(&segs, x, width, duration, 8.0);
            if is_delete {
                Some("pointer")
            } else if let Some((_, is_start)) = marker_hit {
                if is_start {
                    Some("w-resize")
                } else {
                    Some("e-resize")
                }
            } else if body_hit.is_some() {
                Some("move")
            } else {
                Some("crosshair")
            }
        } else {
            None
        };
        widget.set_cursor(
            cursor_name
                .and_then(|name| gtk::gdk::Cursor::from_name(name, None))
                .as_ref(),
        );
        widget.queue_draw();
    });

    {
        let hover_segment = build.page.hover_segment.clone();
        let hover_delete_button = build.page.hover_delete_button.clone();
        motion.connect_leave(move |controller| {
            let Some(widget) = controller.widget() else {
                return;
            };
            hover_segment.set(None);
            hover_delete_button.set(false);
            widget.set_cursor(None);
            widget.queue_draw();
        });
    }

    build.page.waveform.add_controller(motion);
}
