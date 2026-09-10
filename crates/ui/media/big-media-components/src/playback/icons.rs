// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared media labels and symbolic icon choices.
//!
//! All helpers return `&'static str` icon names or short formatted strings;
//! the volume thresholds mirror the steps mpv uses in its OSC.

/// Volume percent below which [`volume_icon_name`] picks the "low" icon.
pub const VOLUME_THRESHOLD_LOW: f64 = 34.0;
/// Volume percent at or above which [`volume_icon_name`] picks the "high" icon.
pub const VOLUME_THRESHOLD_HIGH: f64 = 67.0;

/// Format an audio sample rate in kHz with one decimal for non-multiples of 1000.
#[must_use]
pub fn sample_rate_khz_label(rate: i32) -> String {
    if rate % 1000 == 0 {
        format!("{}", rate / 1000)
    } else {
        format!("{:.1}", f64::from(rate) / 1000.0)
    }
}

/// Symbolic icon name for the current volume percent, including `muted` at `0`.
#[must_use]
pub fn volume_icon_name(volume_percent: f64) -> &'static str {
    if volume_percent <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if volume_percent < VOLUME_THRESHOLD_LOW {
        "audio-volume-low-symbolic"
    } else if volume_percent < VOLUME_THRESHOLD_HIGH {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

/// Same as [`volume_icon_name`] but never returns the muted icon.
#[must_use]
pub fn unmuted_volume_icon_name(volume_percent: f64) -> &'static str {
    if volume_percent < VOLUME_THRESHOLD_LOW {
        "audio-volume-low-symbolic"
    } else if volume_percent < VOLUME_THRESHOLD_HIGH {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

/// Play or pause symbolic icon name to use given the `paused` state.
#[must_use]
pub fn playback_icon(paused: bool) -> &'static str {
    if paused {
        "media-playback-start-symbolic"
    } else {
        "media-playback-pause-symbolic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_label_uses_khz_without_unit() {
        assert_eq!(sample_rate_khz_label(44_100), "44.1");
        assert_eq!(sample_rate_khz_label(48_000), "48");
    }

    #[test]
    fn volume_icons_match_thresholds() {
        assert_eq!(volume_icon_name(0.0), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon_name(20.0), "audio-volume-low-symbolic");
        assert_eq!(volume_icon_name(50.0), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon_name(80.0), "audio-volume-high-symbolic");
    }

    #[test]
    fn playback_icon_reflects_paused_state() {
        assert_eq!(playback_icon(true), "media-playback-start-symbolic");
        assert_eq!(playback_icon(false), "media-playback-pause-symbolic");
    }
}
