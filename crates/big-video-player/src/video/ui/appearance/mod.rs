//! Appearance preferences for the video player window: layout pages and
//! dialog entry points.

mod dialogs;
pub(crate) mod layout;
mod theming;

pub use dialogs::show_appearance_dialog;
pub(in crate::video) use theming::apply_appearance;
