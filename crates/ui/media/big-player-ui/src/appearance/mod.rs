//! Appearance pages shared by BigLinux audio and video players.

mod audio_colors;
mod color_palette_controls;
mod video_colors;
mod window;

pub use crate::illustration::illustration_auto as illustration;
pub use audio_colors::create_audio_colors_page;
pub use video_colors::create_video_colors_page;
pub use window::{build_preferences_window, show_appearance_window};
