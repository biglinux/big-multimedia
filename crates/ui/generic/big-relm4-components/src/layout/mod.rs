//! Shared layout widgets.

pub mod advanced_disclosure;
pub mod column_chrome;
pub mod column_persist;
pub mod dialog_footer;
pub mod dialog_page;
pub mod didactic_card;
pub mod hamburger_menu;
pub mod illustration_card;
pub mod media_window;
pub mod preferences;
pub mod preferences_dialog;
pub mod sidebar_dialog;
pub mod sidebar_header;
pub mod toolbar_pane;
pub mod widget_container;
/// Runtime applier that reparents live controls into their zone containers per
/// a [`zone_layout`](crate::input::zone_layout) value (leak-safe, generic).
pub mod zone_applier;
