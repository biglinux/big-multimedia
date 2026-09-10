//! Compatibility adapter for the shared playback queue.

use crate::db::models::Track;

// `impl QueueEntry for Track` now lives in big-media-data (Track is local there;
// the queue primitive is in the big-media-engines leaf — orphan rule).
pub use big_media_components::playback::queue::{QueueEntry, RepeatMode};

pub type PlayQueue = big_media_components::playback::queue::PlayQueue<Track>;
