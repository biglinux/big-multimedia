//! Segment marking + timeline overlay: M places start/end pair, drag edges
//! with 10 px hit threshold (ew-resize cursor), tap empty area seeks.
//! Cairo overlay paints filled segment rects + standalone mark-point line.

use adw::prelude::*;
use big_relm4_components::list::bounded_list_box::clear_list_box_rows;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gtk::gdk;

use crate::player::mpv_backend::MpvBackend;

use super::build::EditPageBuild;
use super::segment_list::update_segments_listbox;
use super::{Callback, EditPage, TrimSegment, fire};

// Widget fields are WEAK: this aggregator is cloned into closures on
// buttons INSIDE its own listbox rows — strong widget refs here formed a
// rows⇄context⇄listbox cycle that leaked the whole segment UI (listbox,
// rows, overlay, timeline) on every window close. Use [`Self::widgets`].
#[derive(Clone)]
pub(super) struct SegListActions {
    pub(super) segments: Rc<RefCell<Vec<TrimSegment>>>,
    pub(super) mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    overlay: gtk::glib::WeakRef<gtk::DrawingArea>,
    pub(super) on_changed: Callback<Vec<TrimSegment>>,
    listbox: gtk::glib::WeakRef<gtk::ListBox>,
    placeholder: gtk::glib::WeakRef<adw::ActionRow>,
    list_row: gtk::glib::WeakRef<adw::ActionRow>,
    pub(super) duration: Rc<Cell<f64>>,
    timeline: gtk::glib::WeakRef<gtk::Scale>,
}

/// Strong view of the segment-list widgets, upgraded for one operation.
pub(super) struct SegListWidgets {
    pub(super) overlay: gtk::DrawingArea,
    pub(super) listbox: gtk::ListBox,
    pub(super) placeholder: adw::ActionRow,
    pub(super) list_row: adw::ActionRow,
    pub(super) timeline: gtk::Scale,
}

impl SegListActions {
    /// Upgrade every widget ref; `None` once the page is torn down.
    pub(super) fn widgets(&self) -> Option<SegListWidgets> {
        Some(SegListWidgets {
            overlay: self.overlay.upgrade()?,
            listbox: self.listbox.upgrade()?,
            placeholder: self.placeholder.upgrade()?,
            list_row: self.list_row.upgrade()?,
            timeline: self.timeline.upgrade()?,
        })
    }
}

pub(super) fn install_segment_overlay_behavior(build: &EditPageBuild) {
    install_segment_overlay_renderer(build);
    install_segment_overlay_drag_controller(build);
}

pub(super) fn handle_mark_action(
    mpv: &Rc<RefCell<Option<Arc<MpvBackend>>>>,
    mark_point: &Rc<Cell<Option<f64>>>,
    mark_button: &gtk::Button,
    context: &SegListActions,
) -> bool {
    let position = match *mpv.borrow() {
        Some(ref mpv) => mpv.get_position(),
        _ => {
            return false;
        }
    };

    if let Some(start) = mark_point.get() {
        let (start, end) = if start < position {
            (start, position)
        } else {
            (position, start)
        };
        context
            .segments
            .borrow_mut()
            .push(TrimSegment { start, end });
        mark_point.set(None);
        mark_button.remove_css_class("accent");
        if let Some(widgets) = context.widgets() {
            update_segments_listbox(
                &widgets.listbox,
                &context.segments.borrow(),
                &widgets.placeholder,
                &widgets.list_row,
                Some(context),
            );
            widgets.overlay.queue_draw();
        }
        fire(&context.on_changed, context.segments.borrow().clone());
    } else {
        mark_point.set(Some(position));
        mark_button.add_css_class("accent");
        if let Some(widgets) = context.widgets() {
            widgets.overlay.queue_draw();
        }
    }

    true
}

pub(super) fn clear_segments_state(
    context: &SegListActions,
    mark_point: &Rc<Cell<Option<f64>>>,
    mark_button: &gtk::Button,
) {
    context.segments.borrow_mut().clear();
    mark_point.set(None);
    mark_button.remove_css_class("accent");
    if let Some(widgets) = context.widgets() {
        clear_list_box_rows(&widgets.listbox);
        widgets.placeholder.set_visible(true);
        widgets.list_row.set_visible(false);
        widgets.overlay.queue_draw();
    }
    fire(&context.on_changed, Vec::new());
}

fn install_segment_overlay_renderer(build: &EditPageBuild) {
    let segments = build.page.segments.clone();
    let duration = build.page.duration.clone();
    let mark_point = build.page.mark_point.clone();

    build
        .page
        .segment_overlay
        .set_draw_func(move |_area, cairo, width, height| {
            let duration = duration.get();
            if duration <= 0.0 {
                return;
            }
            let width = width as f64;
            let height = height as f64;

            for segment in segments.borrow().iter() {
                let x0 = (segment.start / duration).clamp(0.0, 1.0) * width;
                let x1 = (segment.end / duration).clamp(0.0, 1.0) * width;

                cairo.set_source_rgba(0.2, 0.6, 1.0, 0.4);
                cairo.rectangle(x0, 0.0, (x1 - x0).max(2.0), height);
                let _ = cairo.fill();

                cairo.set_source_rgba(0.2, 0.6, 1.0, 0.8);
                cairo.set_line_width(2.0);
                cairo.move_to(x0, 0.0);
                cairo.line_to(x0, height);
                let _ = cairo.stroke();
                cairo.move_to(x1, 0.0);
                cairo.line_to(x1, height);
                let _ = cairo.stroke();
            }

            if let Some(point) = mark_point.get() {
                let x = (point / duration).clamp(0.0, 1.0) * width;
                cairo.set_source_rgba(1.0, 0.5, 0.0, 0.9);
                cairo.set_line_width(3.0);
                cairo.move_to(x, 0.0);
                cairo.line_to(x, height);
                let _ = cairo.stroke();
            }
        });
}

#[allow(clippy::too_many_lines)] // GTK overlay drag-controller wiring — long by nature
fn install_segment_overlay_drag_controller(build: &EditPageBuild) {
    let segments = build.page.segments.clone();
    let duration = build.page.duration.clone();
    // Weak: these closures live ON the segment overlay — strong overlay
    // (self) or timeline captures pinned the overlay + timeline past close.
    let timeline = build.page.timeline.downgrade();
    let overlay = build.page.segment_overlay.downgrade();
    let context = build.page.seg_list_actions();
    let mpv = build.page.mpv.clone();
    let dragging_edge = Rc::new(Cell::new(None::<(usize, bool)>));
    let drag_start_x = Rc::new(Cell::new(0.0));

    let drag = gtk::GestureDrag::new();
    drag.set_button(1);

    {
        let segments = segments.clone();
        let duration = duration.clone();
        let dragging_edge = dragging_edge.clone();
        let drag_start_x = drag_start_x.clone();
        let overlay = overlay.clone();
        let mpv = mpv.clone();
        let timeline = timeline.clone();
        drag.connect_drag_begin(move |_gesture, x, _y| {
            drag_start_x.set(x);
            let duration = duration.get();
            let (Some(overlay), Some(timeline)) = (overlay.upgrade(), timeline.upgrade()) else {
                return;
            };
            let _ = &overlay;
            let width = overlay.width() as f64;
            if duration <= 0.0 || width <= 0.0 {
                dragging_edge.set(None);
                return;
            }
            let threshold = 10.0;
            let mut found = None;
            for (idx, segment) in segments.borrow().iter().enumerate() {
                let start_x = (segment.start / duration).clamp(0.0, 1.0) * width;
                let end_x = (segment.end / duration).clamp(0.0, 1.0) * width;
                if (x - start_x).abs() <= threshold {
                    found = Some((idx, true));
                    break;
                }
                if (x - end_x).abs() <= threshold {
                    found = Some((idx, false));
                    break;
                }
            }
            dragging_edge.set(found);
            if found.is_none() {
                let position = ((x / width) * duration).clamp(0.0, duration);
                timeline.set_value(position);
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.seek_absolute(position);
                }
            }
        });
    }

    {
        let segments = segments.clone();
        let duration = duration.clone();
        let dragging_edge = dragging_edge.clone();
        let drag_start_x = drag_start_x.clone();
        let overlay = overlay.clone();
        let mpv = mpv.clone();
        let timeline = timeline.clone();
        let context = context.clone();
        drag.connect_drag_update(move |_gesture, offset_x, _offset_y| {
            let duration = duration.get();
            let (Some(overlay), Some(timeline)) = (overlay.upgrade(), timeline.upgrade()) else {
                return;
            };
            let width = overlay.width() as f64;
            if duration <= 0.0 || width <= 0.0 {
                return;
            }
            let x = (drag_start_x.get() + offset_x).clamp(0.0, width);
            let position = ((x / width) * duration).clamp(0.0, duration);

            if let Some((idx, is_start)) = dragging_edge.get() {
                let mut segments = segments.borrow_mut();
                if let Some(segment) = segments.get_mut(idx) {
                    if is_start && position < segment.end {
                        segment.start = position;
                    } else if !is_start && position > segment.start {
                        segment.end = position;
                    }
                }
                drop(segments);
                timeline.set_value(position);
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.seek_absolute(position);
                }
                overlay.queue_draw();
                fire(&context.on_changed, context.segments.borrow().clone());
            } else {
                timeline.set_value(position);
                if let Some(ref mpv) = *mpv.borrow() {
                    mpv.seek_absolute(position);
                }
            }
        });
    }

    {
        let dragging_edge = dragging_edge.clone();
        let segments = segments.clone();
        let context = context.clone();
        drag.connect_drag_end(move |_gesture, _offset_x, _offset_y| {
            if dragging_edge.take().is_none() {
                return;
            }
            if let Some(widgets) = context.widgets() {
                update_segments_listbox(
                    &widgets.listbox,
                    &segments.borrow(),
                    &widgets.placeholder,
                    &widgets.list_row,
                    Some(&context),
                );
            }
        });
    }

    build.page.segment_overlay.add_controller(drag);

    {
        let segments = segments.clone();
        let duration = duration.clone();
        let overlay = overlay.clone();
        let motion = gtk::EventControllerMotion::new();
        let overlay_motion = overlay.clone();
        motion.connect_motion(move |_, x, _y| {
            let duration = duration.get();
            let Some(overlay_motion) = overlay_motion.upgrade() else {
                return;
            };
            let width = overlay_motion.width() as f64;
            if duration <= 0.0 || width <= 0.0 {
                return;
            }
            let threshold = 10.0;
            let near_edge = segments.borrow().iter().any(|segment| {
                let start_x = (segment.start / duration).clamp(0.0, 1.0) * width;
                let end_x = (segment.end / duration).clamp(0.0, 1.0) * width;
                (x - start_x).abs() <= threshold || (x - end_x).abs() <= threshold
            });
            if near_edge {
                overlay_motion.set_cursor_from_name(Some("ew-resize"));
            } else {
                overlay_motion.set_cursor(gdk::Cursor::from_name("default", None).as_ref());
            }
        });
        let overlay_leave = overlay.clone();
        motion.connect_leave(move |_| {
            if let Some(overlay_leave) = overlay_leave.upgrade() {
                overlay_leave.set_cursor(gdk::Cursor::from_name("default", None).as_ref());
            }
        });
        build.page.segment_overlay.add_controller(motion);
    }
}

impl EditPage {
    /// Clone of the in-memory trim segment list.
    #[must_use]
    pub fn segments(&self) -> Vec<TrimSegment> {
        self.segments.borrow().clone()
    }

    pub(super) fn seg_list_actions(&self) -> SegListActions {
        SegListActions {
            segments: self.segments.clone(),
            mpv: self.mpv.clone(),
            overlay: self.segment_overlay.downgrade(),
            on_changed: self.on_segments_changed.clone(),
            listbox: self.segments_listbox.downgrade(),
            placeholder: self.segments_placeholder.downgrade(),
            list_row: self.segments_list_row.downgrade(),
            duration: self.duration.clone(),
            timeline: self.timeline.downgrade(),
        }
    }

    /// Remove every trim segment, clear the timeline mark point, and
    /// drop the "mark active" accent on the mark button.
    pub fn clear_segments(&self) {
        let context = self.seg_list_actions();
        clear_segments_state(&context, &self.mark_point, &self.mark_button);
    }

    /// Append a new trim segment spanning `start..=end` seconds.
    pub fn add_segment(&self, start: f64, end: f64) {
        self.segments.borrow_mut().push(TrimSegment { start, end });
        let context = self.seg_list_actions();
        update_segments_listbox(
            &self.segments_listbox,
            &self.segments.borrow(),
            &self.segments_placeholder,
            &self.segments_list_row,
            Some(&context),
        );
        fire(&self.on_segments_changed, self.segments.borrow().clone());
        self.segment_overlay.queue_draw();
    }
}
