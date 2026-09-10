//! Audio filter chain for the mpv backend: builder, runtime control, effect
//! definitions, and music-specific tuning.

mod chain;
mod control;
mod effects;
mod music;

pub use chain::build_audio_filter_chain;
