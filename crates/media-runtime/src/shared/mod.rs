//! Cross-cutting helpers consumed by the audio and video players.

/// Shared application helpers (file path extraction, color scheme, about dialog).
pub mod app_base;
/// Audio effects: equalizer, noise reduction, and filter chain management.
pub mod audio_effects;
mod audio_effects_eq;
/// Hand files (and marked cuts) to the external converter GUIs.
pub mod converter_handoff;
/// Illustration image builders backed by GResource icons.
pub mod illustration;
/// Playlist file save picker and result alert.
pub mod playlist_export;
/// Shared hover-to-open popover infrastructure.
pub mod popovers;
/// Shared shortcut capture dialog.
pub mod shortcut_capture;
/// Shared sleep timer labels, actions, and settings persistence.
pub mod sleep_timer;
/// Off-thread DB query helper (`spawn_db_query`).
pub mod spawn;
/// Color parsing and Adwaita CSS override generation for custom themes.
pub mod theming;
pub mod tv_cast;
