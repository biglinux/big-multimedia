//! Library services: cover art retrieval, MusicBrainz
//! metadata, LRC lyrics, podcast RSS parsing, and filesystem scanning.

pub mod accounts;
pub mod coverart;
pub mod lrclib;
#[cfg(feature = "db")]
pub mod metadata;
pub mod musicbrainz;
pub mod podcast;
#[cfg(feature = "db")]
pub mod scanner;
