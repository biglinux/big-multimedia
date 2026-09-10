//! Media file-type knowledge (audio/video extensions + path predicates).
//!
//! Moved to `big_media_engines::media_files` (GTK-free parser leaf, shared with
//! `big-media-data`). Re-exported here so existing
//! `big_media_components::media::files::*` consumers keep working unchanged.

pub use big_media_engines::media_files::*;
