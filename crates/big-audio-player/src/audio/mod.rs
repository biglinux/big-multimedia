//! Audio player UI — full music player interface with library, playlist,
//! radio, podcast, lyrics, downloads, and queue views, built on Relm4.

pub mod app;
/// Appearance dialog (custom palette / high-contrast).
pub mod appearance;
// Relocated to big-player-ui (engine-free UI crate); re-exported so the
// app-local `ui::library_tree` can resolve `crate::audio::browser_node::*`.
pub use big_player_ui::browser_node;
mod account_security;
mod column_builders;
pub mod keybindings;
pub mod library_grouping;
/// Pure library node → track resolution.
mod library_query;
/// Pure (GTK-free) library folder scanner.
mod library_scan_engine;
/// Pure (GTK-free) lyrics fetch (embedded tags + LRCLIB).
mod lyrics_fetch_engine;
/// Shared MPRIS playback-state snapshot.
mod mpris_state;
/// Pure (GTK-free) M3U/PLS import parser.
mod playlist_import;
/// Audio + Music preferences window.
pub mod preferences;
/// Pure (GTK-free) snapshot-based queue undo/redo.
mod queue_undo;
pub mod ui;
