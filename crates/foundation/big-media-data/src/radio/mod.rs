//! Radio data layer shared by media apps: Radio-Browser API client (server
//! discovery + fetchers + JSON parse). GTK-free.
//!
//! The favicon LRU cache (`gdk::Texture`) and the radio UI panel are GTK and
//! live in `big-player-ui` (`radio_panel`); the now-playing panel stays
//! app-side (it reparents shell-owned playlist widgets).
//!
//! `RadioExploreEntry` tuple shape: `(name, url, country, tags, favicon)`.

pub type RadioExploreEntry = (String, String, String, String, String);

pub mod api;
