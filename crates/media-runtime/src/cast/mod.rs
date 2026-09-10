//! Cast pipeline: GPU probing, transcode runtime, and optional Chromecast control.

#[cfg(feature = "chromecast")]
pub mod chromecast;
pub mod gpu;
#[cfg(feature = "ui")]
pub mod probe;
#[cfg(feature = "ui")]
pub(crate) mod sandbox;
#[cfg(feature = "ui")]
pub mod transcode;
