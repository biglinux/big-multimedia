//! Sidebar chrome builders split by responsibility.
//!
//! The public surface remains the small set of builders consumed by the sidebar
//! component and row factories.

mod context_menu;
mod empty_state;
mod header;
mod keyboard;
mod menu_actions;
mod metadata;

pub(super) use context_menu::{remove_from_queue, show_row_context_menu};
pub(super) use empty_state::{build_files_empty_state, build_playlist_empty_state};
pub(super) use header::build_header;
pub(super) use keyboard::add_key_controller;
pub(super) use metadata::{build_breadcrumb, build_chips, build_info, build_search};
