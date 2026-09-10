//! Generic playback queue (ordered / shuffled / repeated media lists).
//!
//! The pure primitive (`PlayQueue<T>`, `RepeatMode`, `QueueEntry`) moved to the
//! GTK-free leaf `big_media_engines::queue` so the data layer (`big-media-data`)
//! can implement `QueueEntry` for its own types. Re-exported here so existing
//! `big_media_components::playback::queue::*` consumers keep working unchanged.

pub use big_media_engines::queue::*;
