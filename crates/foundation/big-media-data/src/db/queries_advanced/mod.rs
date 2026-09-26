//! Advanced queries: smart playlists, queue, statistics, radio stations, library browser.

mod library;
mod metadata;
mod queue;
mod radio;
mod smart_playlists;

pub use library::{
    get_compilation_albums, get_library_groups, get_library_track_count, get_library_tracks,
};
pub use metadata::{TrackMetadataUpdate, update_track_metadata, update_track_rating};
pub use queue::{QueueSession, SavedQueue, load_queue, load_queue_session, save_queue_session};
pub use radio::{
    delete_radio_station, get_all_radio_stations, insert_radio_station, migrate_radio_from_json,
};
pub use smart_playlists::{get_most_played, get_never_played, get_recently_added};

// Advanced query tests exercise rusqlite against an in-memory SQLite database.
// Miri cannot execute libsqlite3 FFI, so the ordinary test gate owns this coverage.
#[cfg(all(test, not(miri)))]
mod tests;
