//! Shared cut-timeline drawing and pointer interaction for media playback bars.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

const EDGE_GRAB_PX: f64 = 8.0;

/// Segment edge/body adjustment emitted after a drag gesture completes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutSegmentAdjustment {
    /// Segment id.
    pub id: u64,
    /// New start in seconds.
    pub start: f64,
    /// New end in seconds.
    pub end: f64,
}

#[derive(Clone, Copy)]
enum CutDrag {
    None,
    Start(u64),
    End(u64),
    Body {
        id: u64,
        anchor: f64,
        orig_start: f64,
        orig_end: f64,
    },
}

/// Draw the cut timeline track, played range, cut segments, pending mark and
/// playhead.
pub fn install_cut_timeline_draw(
    area: &gtk::DrawingArea,
    cut_segments: &Rc<RefCell<Vec<(u64, f64, f64)>>>,
    pending_mark: &Rc<RefCell<Option<f64>>>,
    seek_fraction: &Rc<Cell<f64>>,
    duration_secs: &Rc<Cell<f64>>,
) {
    let cut_segments = cut_segments.clone();
    let pending_mark = pending_mark.clone();
    let seek_fraction = seek_fraction.clone();
    let duration_secs = duration_secs.clone();
    area.set_draw_func(move |area, cr, width, height| {
        let duration = duration_secs.get().max(0.001);
        let width = f64::from(width);
        let height = f64::from(height);
        let track_height = (height * 0.5).clamp(6.0, 12.0);
        let track_top = (height - track_height) / 2.0;
        let radius = track_height / 2.0;
        let seek_fraction = seek_fraction.get().clamp(0.0, 1.0);
        let x_for = |time: f64| (time / duration).clamp(0.0, 1.0) * width;

        let foreground = area.color();
        cr.set_source_rgba(
            f64::from(foreground.red()),
            f64::from(foreground.green()),
            f64::from(foreground.blue()),
            0.28,
        );
        rounded_rect(cr, 0.0, track_top, width, track_height, radius);
        let _ = cr.fill();

        cr.set_source_rgba(0.20, 0.52, 0.89, 0.95);
        rounded_rect(
            cr,
            0.0,
            track_top,
            seek_fraction * width,
            track_height,
            radius,
        );
        let _ = cr.fill();

        for (_, start, end) in cut_segments.borrow().iter() {
            let x0 = x_for(*start);
            let x1 = x_for(*end);
            cr.set_source_rgba(0.898, 0.647, 0.039, 0.95);
            cr.rectangle(x0, track_top, (x1 - x0).max(2.0), track_height);
            let _ = cr.fill();
        }

        if let Some(mark) = *pending_mark.borrow() {
            cr.set_source_rgba(0.18, 0.76, 0.35, 1.0);
            cr.rectangle(x_for(mark) - 1.0, 0.0, 2.0, height);
            let _ = cr.fill();
        }

        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cr.rectangle(seek_fraction * width - 1.0, 0.0, 2.0, height);
        let _ = cr.fill();
    });
}

/// Wire click/drag interaction for a cut timeline.
pub fn wire_cut_timeline_interaction(
    area: &gtk::DrawingArea,
    cut_segments: &Rc<RefCell<Vec<(u64, f64, f64)>>>,
    duration_secs: &Rc<Cell<f64>>,
    on_seek_fraction: impl Fn(f64) + 'static,
    on_segment_adjusted: impl Fn(CutSegmentAdjustment) + 'static,
) {
    let on_seek_fraction: Rc<dyn Fn(f64)> = Rc::new(on_seek_fraction);
    let on_segment_adjusted: Rc<dyn Fn(CutSegmentAdjustment)> = Rc::new(on_segment_adjusted);
    let drag_state: Rc<Cell<CutDrag>> = Rc::new(Cell::new(CutDrag::None));

    let click = gtk::GestureClick::new();
    click.set_button(1);
    {
        let cut_segments = cut_segments.clone();
        let duration_secs = duration_secs.clone();
        let on_seek_fraction = on_seek_fraction.clone();
        click.connect_pressed(move |gesture, _, x, _| {
            let Some(area) = gesture.widget() else {
                return;
            };
            let width = f64::from(area.width()).max(1.0);
            let duration = duration_secs.get().max(0.001);
            let segments = cut_segments.borrow();
            if hit_handle(&segments, x, width, duration, EDGE_GRAB_PX).is_some()
                || hit_body(&segments, x, width, duration).is_some()
            {
                return;
            }
            on_seek_fraction((x / width).clamp(0.0, 1.0));
        });
    }
    area.add_controller(click);

    let drag = gtk::GestureDrag::new();
    {
        let drag_state = drag_state.clone();
        let cut_segments = cut_segments.clone();
        let duration_secs = duration_secs.clone();
        drag.connect_drag_begin(move |gesture, start_x, _| {
            let Some(area) = gesture.widget() else {
                return;
            };
            gesture.set_state(gtk::EventSequenceState::Claimed);
            let width = f64::from(area.width()).max(1.0);
            let duration = duration_secs.get().max(0.001);
            let segments = cut_segments.borrow();
            let mode = if let Some((id, is_end)) =
                hit_handle(&segments, start_x, width, duration, EDGE_GRAB_PX)
            {
                if is_end {
                    CutDrag::End(id)
                } else {
                    CutDrag::Start(id)
                }
            } else if let Some(id) = hit_body(&segments, start_x, width, duration) {
                let (_, start, end) = segments
                    .iter()
                    .find(|(segment_id, _, _)| *segment_id == id)
                    .copied()
                    .unwrap_or((id, 0.0, 0.0));
                CutDrag::Body {
                    id,
                    anchor: (start_x / width).clamp(0.0, 1.0) * duration,
                    orig_start: start,
                    orig_end: end,
                }
            } else {
                CutDrag::None
            };
            drag_state.set(mode);
        });
    }
    {
        let drag_state = drag_state.clone();
        let cut_segments = cut_segments.clone();
        let duration_secs = duration_secs.clone();
        let on_seek_fraction = on_seek_fraction.clone();
        drag.connect_drag_update(move |gesture, offset_x, _| {
            let Some(area) = gesture.widget() else {
                return;
            };
            let Some((start_x, _)) = gesture.start_point() else {
                return;
            };
            let width = f64::from(area.width()).max(1.0);
            let duration = duration_secs.get().max(0.001);
            let fraction = ((start_x + offset_x) / width).clamp(0.0, 1.0);
            let time = fraction * duration;
            {
                let mut segments = cut_segments.borrow_mut();
                match drag_state.get() {
                    CutDrag::Start(id) => {
                        if let Some((_, start, end)) = segments
                            .iter_mut()
                            .find(|(segment_id, _, _)| *segment_id == id)
                        {
                            *start = time.min(*end - 0.1).max(0.0);
                        }
                    }
                    CutDrag::End(id) => {
                        if let Some((_, start, end)) = segments
                            .iter_mut()
                            .find(|(segment_id, _, _)| *segment_id == id)
                        {
                            *end = time.max(*start + 0.1).min(duration);
                        }
                    }
                    CutDrag::Body {
                        id,
                        anchor,
                        orig_start,
                        orig_end,
                    } => {
                        if let Some((_, start, end)) = segments
                            .iter_mut()
                            .find(|(segment_id, _, _)| *segment_id == id)
                        {
                            let len = (orig_end - orig_start).max(0.1);
                            let (mut new_start, mut new_end) =
                                (orig_start + (time - anchor), orig_end + (time - anchor));
                            if new_start < 0.0 {
                                new_start = 0.0;
                                new_end = len;
                            }
                            if new_end > duration {
                                new_end = duration;
                                new_start = (duration - len).max(0.0);
                            }
                            *start = new_start;
                            *end = new_end;
                        }
                    }
                    CutDrag::None => {}
                }
            }
            on_seek_fraction(fraction);
            area.queue_draw();
        });
    }
    {
        let drag_state = drag_state.clone();
        let cut_segments = cut_segments.clone();
        let on_segment_adjusted = on_segment_adjusted.clone();
        drag.connect_drag_end(move |gesture, _, _| {
            let Some(area) = gesture.widget() else {
                return;
            };
            let id = match drag_state.replace(CutDrag::None) {
                CutDrag::Start(id) | CutDrag::End(id) | CutDrag::Body { id, .. } => Some(id),
                CutDrag::None => None,
            };
            if let Some(id) = id
                && let Some((_, start, end)) = cut_segments
                    .borrow()
                    .iter()
                    .find(|(segment_id, _, _)| *segment_id == id)
            {
                on_segment_adjusted(CutSegmentAdjustment {
                    id,
                    start: *start,
                    end: *end,
                });
            }
            area.queue_draw();
        });
    }
    area.add_controller(drag);

    let motion = gtk::EventControllerMotion::new();
    {
        let cut_segments = cut_segments.clone();
        let duration_secs = duration_secs.clone();
        motion.connect_motion(move |controller, x, _| {
            let Some(area) = controller.widget() else {
                return;
            };
            let width = f64::from(area.width()).max(1.0);
            let duration = duration_secs.get().max(0.001);
            let segments = cut_segments.borrow();
            let cursor = if let Some((_, is_end)) =
                hit_handle(&segments, x, width, duration, EDGE_GRAB_PX)
            {
                if is_end { "e-resize" } else { "w-resize" }
            } else if hit_body(&segments, x, width, duration).is_some() {
                "move"
            } else {
                "default"
            };
            area.set_cursor_from_name(Some(cursor));
        });
    }
    area.add_controller(motion);
}

fn hit_handle(
    segments: &[(u64, f64, f64)],
    x: f64,
    width: f64,
    duration: f64,
    threshold: f64,
) -> Option<(u64, bool)> {
    let mut best: Option<(u64, bool, f64)> = None;
    for (id, start, end) in segments {
        for (is_end, time) in [(false, *start), (true, *end)] {
            let edge_x = (time / duration).clamp(0.0, 1.0) * width;
            let distance = (x - edge_x).abs();
            if distance <= threshold
                && best.is_none_or(|(_, _, best_distance)| distance < best_distance)
            {
                best = Some((*id, is_end, distance));
            }
        }
    }
    best.map(|(id, is_end, _)| (id, is_end))
}

fn hit_body(segments: &[(u64, f64, f64)], x: f64, width: f64, duration: f64) -> Option<u64> {
    segments.iter().find_map(|(id, start, end)| {
        let x0 = (start / duration).clamp(0.0, 1.0) * width;
        let x1 = (end / duration).clamp(0.0, 1.0) * width;
        (x > x0 && x < x1).then_some(*id)
    })
}

fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    use std::f64::consts::PI;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -PI / 2.0, 0.0);
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        PI / 2.0,
    );
    cr.arc(x + radius, y + height - radius, radius, PI / 2.0, PI);
    cr.arc(x + radius, y + radius, radius, PI, 1.5 * PI);
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_handle_picks_nearest_edge_inside_threshold() {
        let segments = [(7, 10.0, 20.0), (9, 30.0, 40.0)];

        assert_eq!(
            hit_handle(&segments, 101.0, 1000.0, 100.0, 8.0),
            Some((7, false))
        );
        assert_eq!(
            hit_handle(&segments, 198.0, 1000.0, 100.0, 8.0),
            Some((7, true))
        );
        assert_eq!(hit_handle(&segments, 500.0, 1000.0, 100.0, 8.0), None);
    }

    #[test]
    fn hit_body_matches_inside_only() {
        let segments = [(7, 10.0, 20.0)];

        assert_eq!(hit_body(&segments, 150.0, 1000.0, 100.0), Some(7));
        assert_eq!(hit_body(&segments, 100.0, 1000.0, 100.0), None);
        assert_eq!(hit_body(&segments, 200.0, 1000.0, 100.0), None);
    }
}
