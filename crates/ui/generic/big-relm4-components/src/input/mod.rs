//! Input widgets shared by BigLinux Relm4/libadwaita apps.

pub mod adjust_panel;
pub mod button_content;
pub mod debounce;
pub mod dropdown_row;
pub mod illustrated_slider_row;
pub mod inline_spin_set;
pub mod preference_cards;
pub mod scale_popover;
pub mod scale_trough;
pub mod shortcut_capture;
pub mod shortcut_dialog;
pub mod spin_row;
/// Display-free zone-layout model, re-exported from `big-app-kit-core` so the
/// spec layer owns it GTK-free while every `big_relm4_components::input::zone_layout::*`
/// consumer path keeps working. The GTK editor ([`zone_layout_editor`]) and the
/// runtime applier ([`crate::layout::zone_applier`]) build on it.
#[doc(inline)]
pub use big_app_kit_core::zone_layout;
pub mod password_row;
pub mod zone_layout_editor;
