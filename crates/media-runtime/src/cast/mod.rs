//! Playing on a TV: the client of BigNetScreen's Chromecast/DLNA daemon,
//! which owns discovery, streaming and any conversion.

#[cfg(feature = "ui-core")]
pub mod daemon;
/// The "Play on a TV" page, dialog and card both players show.
#[cfg(feature = "ui-core")]
pub mod tv;
