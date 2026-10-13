//! The shared playback queue, holding library tracks.

use crate::db::models::Track;

// `impl QueueEntry for Track` lives in big-media-data, where Track is local;
// the queue primitive is in the big-media-engines leaf (orphan rule).
pub use big_media_components::playback::queue::{QueueEntry, RepeatMode};

pub type PlayQueue = big_media_components::playback::queue::PlayQueue<Track>;
