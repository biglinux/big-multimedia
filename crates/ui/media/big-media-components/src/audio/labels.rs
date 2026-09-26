// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared audio metadata label formatting.
//!
//! Format ffprobe-style audio metadata into a single line for queue rows
//! and headerbars. Empty or non-positive values are omitted; nothing
//! allocates beyond the returned `String`.

/// Format `bitrate` (bits per second) as a `"<value> kbps"` label.
#[must_use]
pub fn bitrate_kbps_label_from_bps(bitrate: i64) -> String {
    format!("{} kbps", bitrate / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_kbps_label_uses_decimal_bps() {
        assert_eq!(bitrate_kbps_label_from_bps(128_000), "128 kbps");
        assert_eq!(bitrate_kbps_label_from_bps(0), "0 kbps");
    }
}
