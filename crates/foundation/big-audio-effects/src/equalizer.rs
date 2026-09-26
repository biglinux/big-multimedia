// SPDX-License-Identifier: MIT

//! Canonical 10-band equalizer layout: the number of bands, their center
//! frequencies and the labels rendered under each slider.
//!
//! Saved band gains are stored per band index, so reordering or resizing
//! these constants is a breaking change to user data.

/// Number of bands in the canonical equalizer.
pub const EQ_BAND_COUNT: usize = 10;

/// Canonical 10-band EQ center frequencies as `f64`.
///
/// Provided so DSP code that already works in double precision does not
/// need to convert each time.
pub const EQ_BAND_FREQUENCIES_F64: [f64; EQ_BAND_COUNT] = [
    31.0, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Short labels rendered under each equalizer slider in the UI.
pub const EQ_BAND_LABELS: [&str; EQ_BAND_COUNT] = [
    "31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k",
];

#[cfg(test)]
mod tests {}
