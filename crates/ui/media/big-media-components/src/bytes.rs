// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared byte-size formatting for media queues and download progress.
//!
//! All helpers are pure and round to display-only precision; they never
//! report the exact byte count. Use the variant that matches the UI:
//! `decimal_compact` for queue rows, `decimal_detailed` for metadata
//! panels, and `binary_transfer` for download progress.

const KIB: u64 = 1024;
const MIB: u64 = KIB * 1024;
const GIB: u64 = MIB * 1024;

/// Format `bytes` as a compact decimal-prefixed string (`"1.0 GB"`, `"512 B"`).
///
/// Uses base-1024 thresholds with SI-style suffixes (`B`, `KB`, `MB`, `GB`)
/// to match the converter queue style. Sub-KB values are rendered as raw
/// bytes; KB values drop the fractional digit.
///
/// # Examples
///
/// ```
/// use big_media_components::bytes::decimal_compact;
///
/// assert_eq!(decimal_compact(512), "512 B");
/// assert_eq!(decimal_compact(2048), "2 KB");
/// assert_eq!(decimal_compact(1_073_741_824), "1.0 GB");
/// ```
#[must_use]
pub fn decimal_compact(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format `bytes` as a decimal-prefixed string with one or two fractional digits.
///
/// Matches the file-info metadata style: KB shows one decimal, MB and GB show
/// two. Companion of [`decimal_compact`] for use in dialogs where extra
/// precision is wanted.
#[must_use]
pub fn decimal_detailed(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.2} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Format `bytes` using IEC binary suffixes (`B`, `KiB`, `MiB`, `GiB`).
///
/// Use for download-progress widgets where the IEC prefixes (`KiB`, `MiB`,
/// `GiB`) communicate exactness better than the decimal `KB`/`MB`/`GB`
/// rendered by [`decimal_compact`].
#[must_use]
pub fn binary_transfer(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_compact_matches_converter_queue_style() {
        assert_eq!(decimal_compact(0), "0 B");
        assert_eq!(decimal_compact(1024), "1 KB");
        assert_eq!(decimal_compact(2048), "2 KB");
        assert_eq!(decimal_compact(1_048_576), "1.0 MB");
        assert_eq!(decimal_compact(1_073_741_824), "1.0 GB");
    }

    #[test]
    fn decimal_detailed_matches_browser_metadata_style() {
        assert_eq!(decimal_detailed(512), "512 B");
        assert_eq!(decimal_detailed(1024), "1.0 KB");
        assert_eq!(decimal_detailed(5 * MIB), "5.00 MB");
        assert_eq!(decimal_detailed(2 * GIB), "2.00 GB");
    }

    #[test]
    fn binary_transfer_matches_download_progress_style() {
        assert_eq!(binary_transfer(0), "0 B");
        assert_eq!(binary_transfer(1023), "1023 B");
        assert_eq!(binary_transfer(1024), "1 KiB");
        assert_eq!(binary_transfer(1536), "2 KiB");
        assert_eq!(binary_transfer(5 * MIB + 512 * KIB), "5.5 MiB");
        assert_eq!(binary_transfer(2 * GIB + 512 * MIB), "2.5 GiB");
    }
}
