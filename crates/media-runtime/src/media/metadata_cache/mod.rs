//! Video metadata probing and caching layer.
//!
//! Wraps `ffprobe` invocations behind an in-memory + on-disk cache so that
//! repeated queries for the same file are essentially free. A native
//! container parser (`native_duration`) supplies the duration when ffprobe
//! reports none. An atomic semaphore limits concurrent `ffprobe` child processes.

mod cache;
mod display;
mod probe;

pub use display::{meta_summary, resolution_label};
pub use probe::{probe_metadata, probe_metadata_cached};

pub(in crate::media) use cache::{acquire_slot, meta_cache, release_slot};

#[cfg(test)]
mod tests;
