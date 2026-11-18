//! Playing on a TV: the client of BigNetScreen's Chromecast/DLNA daemon,
//! which owns discovery, streaming and any conversion (`daemon`), and the
//! page, dialog and card both players show (`tv`).

#[cfg(feature = "ui-core")]
pub mod daemon;
#[cfg(feature = "ui-core")]
pub mod tv;
