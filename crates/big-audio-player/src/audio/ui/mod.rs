//! App-local Relm4 glue for the audio player (in-progress port).
//!
//! Reusable player panels live in the `big-player-ui` crate (lyrics, playback
//! bar, podcast, downloads, file browser, radio) — engine-free, so other media
//! apps can reuse them. Only genuinely app-specific wiring stays here (the
//! AudioShell root + library/playlist composition land here in R5/R6).

/// Relm4 root component (audio window + child panels + engine state).
pub mod audio_shell;
/// Cast (Chromecast) device-picker dialog (restored app-local).
#[cfg(feature = "chromecast")]
pub(crate) mod cast_dialog;
/// Detail `ColumnView` column builders for the library panel.
mod library_detail_columns;
/// Detail `ColumnView` interactions: drag, multi-select, context menu.
mod library_detail_interactions;
/// Shared app-local library dialogs (add-to-playlist picker).
mod library_dialogs;
/// Grouping / age / search controls bar.
mod library_grouping_ui;
/// Playlist import/export, save-as, folder-manage dialogs.
mod library_io;
/// App-local library panel (push-model; tree | detail; DB is shell-owned).
pub mod library_panel;
/// Tag editor dialog.
mod library_tag_editor;
/// Left-pane library tree (TreeListModel of BrowserNode; atomic preload).
/// `pub(crate)` so `library_query` can build the `LibraryTreeData` payload.
pub(crate) mod library_tree;
/// Queue-panel chrome: toolbar, empty states, column-config dialog.
mod playlist_chrome;
/// Queue `ColumnView` column builders (split out of `playlist_panel`).
mod playlist_columns;
/// Playlist drop geometry (relocated from the god-object for cutover survival).
mod playlist_geom;
/// Queue `ColumnView` interactions: drop, reorder, sort, context menu.
mod playlist_interactions;
/// Playlist manager dialog (save / load / rename / export / import / delete).
pub mod playlist_manager;
/// App-local playlist/queue panel (push-model; queue is shell-owned).
pub mod playlist_panel;
/// Keyboard-shortcuts viewer/rebind dialog (restored app-local).
pub(crate) mod shortcuts_dialog;
/// Smartphone web-remote dialog wiring (restored app-local).
pub(crate) mod smartphone_dialog;
/// Audio appearance application (palette + high-contrast CSS providers).
pub(crate) mod theming;
