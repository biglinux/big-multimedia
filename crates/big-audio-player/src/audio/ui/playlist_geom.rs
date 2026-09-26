//! Playlist drop geometry — pure-ish hit-testing for the queue `ColumnView`.

use adw::prelude::*;
use big_relm4_components::list::signal_factory;
use gtk::pango;

/// Fallback only — actual pitch is measured at runtime.
pub const FALLBACK_ROW_HEIGHT: f64 = 44.0;

/// A plain left-aligned label factory for the grouping/age dropdowns.
pub fn dropdown_list_factory() -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let label = gtk::Label::new(None);
        label.set_xalign(0.0);
        label.set_margin_start(8);
        label.set_margin_end(8);
        label.set_margin_top(4);
        label.set_margin_bottom(4);
        let attrs = pango::AttrList::new();
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Normal));
        label.set_attributes(Some(&attrs));
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(|_, factory_callback_value| {
        let Some(list_item) = signal_factory::list_item_from_factory_value(factory_callback_value)
        else {
            return;
        };
        let Some(label) = signal_factory::child_as::<gtk::Label>(list_item) else {
            return;
        };
        if let Some(string_object) = signal_factory::bound_model_as::<gtk::StringObject>(list_item)
        {
            label.set_label(&string_object.string());
        }
    });
    factory
}

/// Derives row pitch from `vadjustment.upper() - header_h` divided by queue len.
/// Falls back when the result is implausible (mid-layout transient).
fn effective_row_height(total_content: f64, header_h: f64, queue_len: usize) -> f64 {
    if queue_len == 0 {
        return FALLBACK_ROW_HEIGHT;
    }
    let rh = (total_content - header_h) / queue_len as f64;
    if rh > 20.0 && rh < 100.0 {
        return rh;
    }
    FALLBACK_ROW_HEIGHT
}

/// Resolved drop slot: where to insert + where to draw the indicator line.
pub struct DropPosition {
    /// Queue index the drop would insert at (clamped to `0..=queue_len`).
    pub insert_index: usize,
    /// Indicator-line y, relative to the `ScrolledWindow` (overlay parent).
    pub indicator_y: f64,
}

fn drop_position_from_metrics(
    scroll_offset: f64,
    total_content: f64,
    header_h: f64,
    y: f64,
    queue_len: usize,
) -> DropPosition {
    let rh = effective_row_height(total_content, header_h, queue_len);
    let row_y = (scroll_offset + y) - header_h;
    let insert_index = ((row_y / rh).round() as isize).clamp(0, queue_len as isize) as usize;
    let indicator_y = (insert_index as f64 * rh) + header_h - scroll_offset;

    DropPosition {
        insert_index,
        indicator_y: indicator_y.max(0.0),
    }
}

fn reorder_drop_position_from_metrics(
    scroll_offset: f64,
    total_content: f64,
    header_h: f64,
    y: f64,
    queue_len: usize,
) -> DropPosition {
    let rh = effective_row_height(total_content, header_h, queue_len);
    let row_y = y - header_h;
    let insert_index = ((row_y / rh).round() as isize).clamp(0, queue_len as isize) as usize;
    let indicator_y = ((insert_index as f64 * rh) + header_h) - scroll_offset;

    DropPosition {
        insert_index,
        indicator_y: indicator_y.max(0.0),
    }
}

/// `y` in `ScrolledWindow` (viewport) coordinates — for the external-path
/// DropTarget mounted on the scroll area.
#[must_use]
pub fn find_drop_position(
    scroll: &gtk::ScrolledWindow,
    column_view: &gtk::ColumnView,
    y: f64,
    queue_len: usize,
) -> DropPosition {
    let scroll_offset = scroll.vadjustment().value();
    let total_content = scroll.vadjustment().upper();
    let header_h = column_view
        .first_child()
        .map_or(0.0, |h| f64::from(h.height()));
    drop_position_from_metrics(scroll_offset, total_content, header_h, y, queue_len)
}

/// `y` in `ColumnView` coordinates — for the internal row-reorder DropTarget.
#[must_use]
pub fn find_reorder_drop_position(
    scroll: &gtk::ScrolledWindow,
    column_view: &gtk::ColumnView,
    y: f64,
    queue_len: usize,
) -> DropPosition {
    let header_h = column_view
        .first_child()
        .map_or(0.0, |h| f64::from(h.height()));
    let scroll_offset = scroll.vadjustment().value();
    let total_content = scroll.vadjustment().upper();
    reorder_drop_position_from_metrics(scroll_offset, total_content, header_h, y, queue_len)
}

/// Walks up from the picked widget to read the row position stashed via
/// `set_data("vis-pos")` in the title factory bind (right-click hit-test).
#[must_use]
pub fn find_clicked_position(column_view: &gtk::Widget, x: f64, y: f64) -> Option<u32> {
    let picked = column_view.pick(x, y, gtk::PickFlags::DEFAULT)?;
    clicked_position_from_widget(Some(picked))
}

fn clicked_position_from_widget(widget: Option<gtk::Widget>) -> Option<u32> {
    clicked_position_from_ancestor_chain(
        widget,
        |w| {
            // SAFETY: matches set_data("vis-pos", position) in the title factory bind.
            let pos: Option<std::ptr::NonNull<u32>> = unsafe { w.data("vis-pos") };
            pos.map(|ptr| unsafe { *ptr.as_ref() })
        },
        |w| w.parent(),
    )
}

fn clicked_position_from_ancestor_chain<T>(
    mut current: Option<T>,
    mut visual_position: impl FnMut(&T) -> Option<u32>,
    mut parent: impl FnMut(T) -> Option<T>,
) -> Option<u32> {
    while let Some(node) = current {
        if let Some(position) = visual_position(&node) {
            return Some(position);
        }
        current = parent(node);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        FALLBACK_ROW_HEIGHT, clicked_position_from_ancestor_chain, drop_position_from_metrics,
        effective_row_height, reorder_drop_position_from_metrics,
    };

    #[test]
    fn effective_row_height_uses_measured_pitch() {
        assert_eq!(effective_row_height(330.0, 30.0, 5), 60.0);
    }

    #[test]
    fn effective_row_height_falls_back_without_rows() {
        assert_eq!(effective_row_height(330.0, 30.0, 0), FALLBACK_ROW_HEIGHT);
    }

    #[test]
    fn effective_row_height_falls_back_at_implausible_bounds() {
        assert_eq!(effective_row_height(130.0, 30.0, 5), FALLBACK_ROW_HEIGHT);
        assert_eq!(effective_row_height(530.0, 30.0, 5), FALLBACK_ROW_HEIGHT);
    }

    #[test]
    fn drop_position_uses_viewport_y_plus_scroll_offset() {
        let pos = drop_position_from_metrics(120.0, 330.0, 30.0, 88.0, 5);
        assert_eq!(pos.insert_index, 3);
        assert_eq!(pos.indicator_y, 90.0);
    }

    #[test]
    fn drop_position_clamps_before_first_row() {
        let pos = drop_position_from_metrics(0.0, 330.0, 30.0, 0.0, 5);
        assert_eq!(pos.insert_index, 0);
        assert_eq!(pos.indicator_y, 30.0);
    }

    #[test]
    fn drop_position_clamps_after_last_row() {
        let pos = drop_position_from_metrics(0.0, 330.0, 30.0, 500.0, 5);
        assert_eq!(pos.insert_index, 5);
        assert_eq!(pos.indicator_y, 330.0);
    }

    #[test]
    fn reorder_drop_position_uses_column_y_without_scroll_offset() {
        let pos = reorder_drop_position_from_metrics(120.0, 330.0, 30.0, 208.0, 5);
        assert_eq!(pos.insert_index, 3);
        assert_eq!(pos.indicator_y, 90.0);
    }

    #[test]
    fn reorder_drop_position_clamps_indicator_to_viewport() {
        let pos = reorder_drop_position_from_metrics(120.0, 330.0, 30.0, 0.0, 5);
        assert_eq!(pos.insert_index, 0);
        assert_eq!(pos.indicator_y, 0.0);
    }

    #[test]
    fn clicked_position_reads_ancestor_visual_position() {
        let parents = [None, Some(0), Some(1)];
        let positions = [None, Some(7), None];
        assert_eq!(
            clicked_position_from_ancestor_chain(
                Some(2_usize),
                |idx| positions[*idx],
                |idx| { parents[idx] }
            ),
            Some(7)
        );
    }

    #[test]
    fn clicked_position_returns_none_without_visual_position() {
        let parents = [None, Some(0), Some(1)];
        let positions = [None, None, None];
        assert_eq!(
            clicked_position_from_ancestor_chain(
                Some(2_usize),
                |idx| positions[*idx],
                |idx| { parents[idx] }
            ),
            None
        );
    }
}
