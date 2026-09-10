//! Compatibility re-export for shared playlist ordering helpers.

pub use big_media_components::playlist_order::{shuffled_order, sort_paths};

#[cfg(test)]
pub(crate) use big_media_components::playlist_order::apply_permutation;
