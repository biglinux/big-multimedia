//! Store, fuzzy/kind filter, and list/grid/queue view factories.
//!
//! The implementation is split by view kind so row factories, async thumbnail
//! workers, and store/filter plumbing stay reviewable.

mod grid_view;
mod list_view;
mod model;
mod queue_view;
mod shared;

pub(super) use grid_view::build_grid_view;
pub(super) use list_view::build_list_view;
pub(super) use queue_view::build_queue_view;
