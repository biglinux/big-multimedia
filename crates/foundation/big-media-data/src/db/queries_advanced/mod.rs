//! Advanced queries: smart playlists, queue, statistics, radio stations, library browser.

mod library;
mod metadata;
mod queue;
mod radio;
mod smart_playlists;

pub use library::{
    get_compilation_albums, get_library_groups, get_library_track_count, get_library_tracks,
};
pub use metadata::{
    TrackMetadataUpdate, update_track_metadata, update_track_metadata_from_mb, update_track_rating,
};
pub use queue::{load_queue, save_queue};
pub use radio::{
    delete_radio_station, get_all_radio_stations, insert_radio_station, migrate_radio_from_json,
    update_radio_station,
};
pub use smart_playlists::{
    create_smart_playlist, delete_smart_playlist, execute_smart_playlist_rules,
    get_duplicate_tracks, get_most_played, get_never_played, get_recently_added,
    get_smart_playlists,
};

// Advanced query tests exercise rusqlite against an in-memory SQLite database.
// Miri cannot execute libsqlite3 FFI, so the ordinary test gate owns this coverage.
#[cfg(all(test, not(miri)))]
mod tests;
