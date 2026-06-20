//! Audio player UI — full music player interface with library, playlist,
//! radio, podcast, lyrics, downloads, and queue views, built on Relm4.

pub mod app;
/// Appearance dialog (custom palette / high-contrast) — restored app-local.
pub mod appearance;
// Relocated to big-player-ui (engine-free UI crate); re-exported so the
// app-local `ui::library_tree` can resolve `crate::audio::browser_node::*`.
pub use big_player_ui::browser_node;
mod column_builders;
mod converter_launcher;
pub mod keybindings;
pub mod library_grouping;
/// Pure library node → track resolution (survives the window/ cutover).
mod library_query;
/// Pure (GTK-free) library folder scanner (survives the window/ cutover).
mod library_scan_engine;
/// Pure (GTK-free) lyrics fetch (embedded tags + LRCLIB; survives the cutover).
mod lyrics_fetch_engine;
/// Shared MPRIS playback-state snapshot (survives the window/ cutover).
mod mpris_state;
/// Pure (GTK-free) M3U/PLS import parser (survives the window/ cutover).
mod playlist_import;
/// Audio + Music preferences window — restored app-local.
pub mod preferences;
/// Pure (GTK-free) snapshot-based queue undo/redo (survives the cutover).
mod queue_undo;
pub mod ui;
