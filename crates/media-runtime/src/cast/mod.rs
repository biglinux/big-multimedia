//! Cast pipeline: GPU probing, transcode runtime, and optional Chromecast control.

#[cfg(feature = "chromecast")]
pub mod chromecast;
#[cfg(feature = "ui-core")]
pub mod daemon;
pub mod gpu;
#[cfg(feature = "ui-core")]
pub mod probe;
#[cfg(feature = "ui-core")]
pub(crate) mod sandbox;
#[cfg(feature = "ui-core")]
pub mod transcode;
