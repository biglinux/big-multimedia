//! Crop interactions on the editor preview: 4-spinner apply pipeline,
//! reset button, rule-of-thirds grid toggle, double-click → fullscreen,
//! and edge/corner/center drag with 20 px hit radius and 2 px MIN_GAP so
//! opposite edges can't cross. Drag deltas are translated from widget
//! pixels to video pixels via `DragGeom`.

use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use super::actions::toggle_fullscreen_state;
use super::build::EditPageBuild;

pub(super) fn install_crop_interactions(build: &EditPageBuild) {
    install_crop_apply_callbacks(build);
    install_crop_reset_controller(build);
    install_crop_overlay_redraw(build);
    install_grid_toggle(build);
    install_fullscreen_double_click_controller(build);
    install_crop_drag_controller(build);
}

fn install_crop_apply_callbacks(build: &EditPageBuild) {
    let mpv = build.page.mpv.clone();
    let video_width = build.page.video_width.clone();
    let video_height = build.page.video_height.clone();
    let updating_crop = build.updating_crop.clone();
    // Weak: this closure is cloned onto EVERY crop spinner's value-changed —
    // strong captures of all four spinners form a 4-way mutual cycle that
    // made the whole crop row immortal (leaked on every window close).
    let crop_left = build.page.crop_left.downgrade();
    let crop_right = build.page.crop_right.downgrade();
    let crop_top = build.page.crop_top.downgrade();
    let crop_bottom = build.page.crop_bottom.downgrade();

    let apply_crop = move || {
        if updating_crop.get() {
            return;
        }
        let width = video_width.get();
        let height = video_height.get();
        if width <= 0 || height <= 0 {
            return;
        }
        let (Some(crop_left), Some(crop_right), Some(crop_top), Some(crop_bottom)) = (
            crop_left.upgrade(),
            crop_right.upgrade(),
            crop_top.upgrade(),
            crop_bottom.upgrade(),
        ) else {
            return;
        };
        let left = crop_left.value() as i64;
        let right = crop_right.value() as i64;
        let top = crop_top.value() as i64;
        let bottom = crop_bottom.value() as i64;
        let crop_width = (width - left - right).max(2);
        let crop_height = (height - top - bottom).max(2);
        let crop = format!("{crop_width}x{crop_height}+{left}+{top}");
        if let Some(ref mpv) = *mpv.borrow() {
            mpv.set_video_crop(&crop);
        }
    };

    let apply_left = apply_crop.clone();
    build
        .page
        .crop_left
        .connect_value_changed(move |_| apply_left());
    let apply_right = apply_crop.clone();
    build
        .page
        .crop_right
        .connect_value_changed(move |_| apply_right());
    let apply_top = apply_crop.clone();
    build
        .page
        .crop_top
        .connect_value_changed(move |_| apply_top());
    build
        .page
        .crop_bottom
        .connect_value_changed(move |_| apply_crop());
}

fn install_crop_reset_controller(build: &EditPageBuild) {
    let mpv = build.page.mpv.clone();
    let crop_left = build.page.crop_left.clone();
    let crop_right = build.page.crop_right.clone();
    let crop_top = build.page.crop_top.clone();
    let crop_bottom = build.page.crop_bottom.clone();
    let updating_crop = build.updating_crop.clone();
    let crop_overlay_area = build.page.crop_overlay_area.clone();

    build.crop_reset_button.connect_clicked(move |_| {
        updating_crop.set(true);
        crop_left.set_value(0.0);
        crop_right.set_value(0.0);
        crop_top.set_value(0.0);
        crop_bottom.set_value(0.0);
        updating_crop.set(false);
        if let Some(ref mpv) = *mpv.borrow() {
            mpv.set_video_crop("");
        }
        crop_overlay_area.queue_draw();
    });
}

fn install_crop_overlay_redraw(build: &EditPageBuild) {
    let crop_overlay_area = build.page.crop_overlay_area.clone();
    let redraw_right = crop_overlay_area.clone();
    let redraw_top = crop_overlay_area.clone();
    let redraw_bottom = crop_overlay_area.clone();
    build
        .page
        .crop_left
        .connect_value_changed(move |_| crop_overlay_area.queue_draw());
    build
        .page
        .crop_right
        .connect_value_changed(move |_| redraw_right.queue_draw());
    build
        .page
        .crop_top
        .connect_value_changed(move |_| redraw_top.queue_draw());
    build
        .page
        .crop_bottom
        .connect_value_changed(move |_| redraw_bottom.queue_draw());
}

fn install_grid_toggle(build: &EditPageBuild) {
    let show_grid = build.page.show_grid.clone();
    let crop_overlay_area = build.page.crop_overlay_area.clone();
    build.page.grid_button.connect_toggled(move |button| {
        show_grid.set(button.is_active());
        crop_overlay_area.queue_draw();
    });
}

fn install_fullscreen_double_click_controller(build: &EditPageBuild) {
    let is_fullscreen = build.page.is_fullscreen.clone();
    let fullscreen_button = build.page.fullscreen_button.clone();
    let on_fullscreen = build.page.on_fullscreen.clone();

    let click = gtk::GestureClick::new();
    click.set_button(1);
    click.connect_released(move |gesture, n_press, _x, _y| {
        if n_press == 2 {
            toggle_fullscreen_state(&is_fullscreen, &fullscreen_button, &on_fullscreen);
            gesture.set_state(gtk::EventSequenceState::Claimed);
        }
    });
    build.page.crop_overlay_area.add_controller(click);
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum DragEdge {
    #[default]
    None,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

#[derive(Clone, Copy, Default)]
struct CropStart {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

struct EdgeBounds {
    left_x: f64,
    right_x: f64,
    top_y: f64,
    bottom_y: f64,
}

fn classify_drag_edge(x: f64, y: f64, b: &EdgeBounds) -> DragEdge {
    const HIT_RADIUS: f64 = 20.0;
    let near_left = (x - b.left_x).abs() < HIT_RADIUS;
    let near_right = (x - b.right_x).abs() < HIT_RADIUS;
    let near_top = (y - b.top_y).abs() < HIT_RADIUS;
    let near_bottom = (y - b.bottom_y).abs() < HIT_RADIUS;

    if near_left && near_top {
        DragEdge::TopLeft
    } else if near_right && near_top {
        DragEdge::TopRight
    } else if near_left && near_bottom {
        DragEdge::BottomLeft
    } else if near_right && near_bottom {
        DragEdge::BottomRight
    } else if near_left && y >= b.top_y && y <= b.bottom_y {
        DragEdge::Left
    } else if near_right && y >= b.top_y && y <= b.bottom_y {
        DragEdge::Right
    } else if near_top && x >= b.left_x && x <= b.right_x {
        DragEdge::Top
    } else if near_bottom && x >= b.left_x && x <= b.right_x {
        DragEdge::Bottom
    } else if x > b.left_x && x < b.right_x && y > b.top_y && y < b.bottom_y {
        DragEdge::Center
    } else {
        DragEdge::None
    }
}

#[derive(Clone)]
struct CropScales {
    left: gtk::SpinButton,
    right: gtk::SpinButton,
    top: gtk::SpinButton,
    bottom: gtk::SpinButton,
}

/// Weak handle stored inside the crop-drag closures. The gesture lives ON
/// the crop overlay while the spinners' redraw handlers hold the overlay
/// strong — strong spinners here closed an overlay⇄spinners mutual cycle
/// that leaked the whole crop UI on window close.
#[derive(Clone)]
struct CropScalesWeak {
    left: glib::WeakRef<gtk::SpinButton>,
    right: glib::WeakRef<gtk::SpinButton>,
    top: glib::WeakRef<gtk::SpinButton>,
    bottom: glib::WeakRef<gtk::SpinButton>,
}

impl CropScalesWeak {
    fn from_build(build: &EditPageBuild) -> Self {
        Self {
            left: build.page.crop_left.downgrade(),
            right: build.page.crop_right.downgrade(),
            top: build.page.crop_top.downgrade(),
            bottom: build.page.crop_bottom.downgrade(),
        }
    }

    fn upgrade(&self) -> Option<CropScales> {
        Some(CropScales {
            left: self.left.upgrade()?,
            right: self.right.upgrade()?,
            top: self.top.upgrade()?,
            bottom: self.bottom.upgrade()?,
        })
    }
}

impl CropScales {
    fn snapshot(&self) -> CropStart {
        CropStart {
            left: self.left.value(),
            right: self.right.value(),
            top: self.top.value(),
            bottom: self.bottom.value(),
        }
    }
}

struct DragGeom {
    dx: f64,
    dy: f64,
    w: f64,
    h: f64,
    vw: f64,
    vh: f64,
}

fn apply_drag(edge: DragEdge, scales: &CropScales, start: &CropStart, g: &DragGeom) {
    const MIN_GAP: f64 = 2.0;
    let dxw = (g.dx / g.w) * g.vw;
    let dyh = (g.dy / g.h) * g.vh;
    let (vw, vh) = (g.vw, g.vh);

    let clamp_left = |v: f64, right_ref: f64| v.clamp(0.0, vw - right_ref - MIN_GAP);
    let clamp_right = |v: f64, left_ref: f64| v.clamp(0.0, vw - left_ref - MIN_GAP);
    let clamp_top = |v: f64, bottom_ref: f64| v.clamp(0.0, vh - bottom_ref - MIN_GAP);
    let clamp_bottom = |v: f64, top_ref: f64| v.clamp(0.0, vh - top_ref - MIN_GAP);

    match edge {
        DragEdge::Left => scales
            .left
            .set_value(clamp_left(start.left + dxw, scales.right.value())),
        DragEdge::Right => scales
            .right
            .set_value(clamp_right(start.right - dxw, scales.left.value())),
        DragEdge::Top => scales
            .top
            .set_value(clamp_top(start.top + dyh, scales.bottom.value())),
        DragEdge::Bottom => scales
            .bottom
            .set_value(clamp_bottom(start.bottom - dyh, scales.top.value())),
        DragEdge::TopLeft => {
            scales
                .left
                .set_value(clamp_left(start.left + dxw, scales.right.value()));
            scales
                .top
                .set_value(clamp_top(start.top + dyh, scales.bottom.value()));
        }
        DragEdge::TopRight => {
            scales
                .right
                .set_value(clamp_right(start.right - dxw, scales.left.value()));
            scales
                .top
                .set_value(clamp_top(start.top + dyh, scales.bottom.value()));
        }
        DragEdge::BottomLeft => {
            scales
                .left
                .set_value(clamp_left(start.left + dxw, scales.right.value()));
            scales
                .bottom
                .set_value(clamp_bottom(start.bottom - dyh, scales.top.value()));
        }
        DragEdge::BottomRight => {
            scales
                .right
                .set_value(clamp_right(start.right - dxw, scales.left.value()));
            scales
                .bottom
                .set_value(clamp_bottom(start.bottom - dyh, scales.top.value()));
        }
        DragEdge::Center => {
            // Mirror against starts, not live values — second set_value
            // would otherwise see a stale bound from the first.
            scales
                .left
                .set_value((start.left + dxw).clamp(0.0, vw - start.right - MIN_GAP));
            scales
                .right
                .set_value((start.right - dxw).clamp(0.0, vw - start.left - MIN_GAP));
            scales
                .top
                .set_value((start.top + dyh).clamp(0.0, vh - start.bottom - MIN_GAP));
            scales
                .bottom
                .set_value((start.bottom - dyh).clamp(0.0, vh - start.top - MIN_GAP));
        }
        DragEdge::None => {}
    }
}

fn install_crop_drag_controller(build: &EditPageBuild) {
    let drag = gtk::GestureDrag::new();
    let scales = CropScalesWeak::from_build(build);
    let video_width = build.page.video_width.clone();
    let video_height = build.page.video_height.clone();
    let mpv = build.page.mpv.clone();
    let updating_crop = build.updating_crop.clone();

    let drag_edge = Rc::new(Cell::new(DragEdge::None));
    let start_vals = Rc::new(RefCell::new(CropStart::default()));

    install_drag_begin(
        &drag,
        &scales,
        &video_width,
        &video_height,
        &drag_edge,
        &start_vals,
    );
    install_drag_update(
        &drag,
        scales.clone(),
        video_width.clone(),
        video_height.clone(),
        drag_edge.clone(),
        start_vals.clone(),
        updating_crop,
    );
    install_drag_end(&drag, scales, video_width, video_height, mpv);

    build.page.crop_overlay_area.add_controller(drag);
}

fn install_drag_begin(
    drag: &gtk::GestureDrag,
    scales: &CropScalesWeak,
    video_width: &Rc<Cell<i64>>,
    video_height: &Rc<Cell<i64>>,
    drag_edge: &Rc<Cell<DragEdge>>,
    start_vals: &Rc<RefCell<CropStart>>,
) {
    let scales = scales.clone();
    let video_width = video_width.clone();
    let video_height = video_height.clone();
    let drag_edge = drag_edge.clone();
    let start_vals = start_vals.clone();
    drag.connect_drag_begin(move |gesture, x, y| {
        let Some(widget) = gesture.widget() else {
            return;
        };
        let w = widget.width() as f64;
        let h = widget.height() as f64;
        let vw = video_width.get() as f64;
        let vh = video_height.get() as f64;
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let Some(scales) = scales.upgrade() else {
            return;
        };
        let bounds = EdgeBounds {
            left_x: (scales.left.value() / vw) * w,
            right_x: w - (scales.right.value() / vw) * w,
            top_y: (scales.top.value() / vh) * h,
            bottom_y: h - (scales.bottom.value() / vh) * h,
        };
        *start_vals.borrow_mut() = scales.snapshot();
        drag_edge.set(classify_drag_edge(x, y, &bounds));
    });
}

fn install_drag_update(
    drag: &gtk::GestureDrag,
    scales: CropScalesWeak,
    video_width: Rc<Cell<i64>>,
    video_height: Rc<Cell<i64>>,
    drag_edge: Rc<Cell<DragEdge>>,
    start_vals: Rc<RefCell<CropStart>>,
    updating_crop: Rc<Cell<bool>>,
) {
    drag.connect_drag_update(move |gesture, dx, dy| {
        let edge = drag_edge.get();
        if edge == DragEdge::None {
            return;
        }
        let Some(widget) = gesture.widget() else {
            return;
        };
        let w = widget.width() as f64;
        let h = widget.height() as f64;
        let vw = video_width.get() as f64;
        let vh = video_height.get() as f64;
        if vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let start = *start_vals.borrow();
        let geom = DragGeom {
            dx,
            dy,
            w,
            h,
            vw,
            vh,
        };
        let Some(scales) = scales.upgrade() else {
            return;
        };
        updating_crop.set(true);
        apply_drag(edge, &scales, &start, &geom);
        updating_crop.set(false);
    });
}

fn install_drag_end(
    drag: &gtk::GestureDrag,
    scales: CropScalesWeak,
    video_width: Rc<Cell<i64>>,
    video_height: Rc<Cell<i64>>,
    mpv: Rc<RefCell<Option<Arc<crate::player::mpv_backend::MpvBackend>>>>,
) {
    drag.connect_drag_end(move |_, _, _| {
        let w = video_width.get();
        let h = video_height.get();
        if w <= 0 || h <= 0 {
            return;
        }
        let Some(scales) = scales.upgrade() else {
            return;
        };
        let left = scales.left.value() as i64;
        let right = scales.right.value() as i64;
        let top = scales.top.value() as i64;
        let bottom = scales.bottom.value() as i64;
        let crop_width = (w - left - right).max(2);
        let crop_height = (h - top - bottom).max(2);
        let crop = format!("{crop_width}x{crop_height}+{left}+{top}");
        if let Some(ref mpv) = *mpv.borrow() {
            mpv.set_video_crop(&crop);
        }
    });
}
