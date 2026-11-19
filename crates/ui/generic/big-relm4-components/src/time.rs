// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared media and dialog time formatting.
//!
//! Pure helpers used by transports, queue rows, file-info dialogs, and editor
//! controls. All formatters tolerate `NaN`, infinities, and negative input by
//! returning a stable placeholder instead of panicking.

/// Format `total_secs` for a media library row, e.g. `"1h 05min"`, `"5 min"`
/// or `"40 s"`.
///
/// Floors to minute precision, or to seconds under a minute, and omits the
/// hours field when the duration is under an hour.
///
/// # Examples
///
/// ```
/// use big_relm4_components::time::duration_hm;
///
/// assert_eq!(duration_hm(40.0), "40 s");
/// assert_eq!(duration_hm(300.0), "5 min");
/// assert_eq!(duration_hm(3900.0), "1h 05min");
/// ```
#[must_use]
pub fn duration_hm(total_secs: f64) -> String {
    let total_mins = (total_secs / 60.0) as u64;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours > 0 {
        format!("{hours}h {mins:02}min")
    } else if mins > 0 {
        format!("{mins} min")
    } else {
        format!("{} s", total_secs.max(0.0) as u64)
    }
}

/// Format `total_secs` as `H:MM:SS` or `M:SS`; returns an empty string for invalid input.
///
/// Use when the empty string should hide the label entirely.
#[must_use]
pub fn duration_hms(total_secs: f64) -> String {
    if total_secs <= 0.0 || !total_secs.is_finite() {
        return String::new();
    }
    hms(total_secs as u64)
}

/// Format current playback position; falls back to `"0:00"` for invalid input.
#[must_use]
pub fn playback_time(secs: f64) -> String {
    if secs < 0.0 || !secs.is_finite() {
        return "0:00".to_string();
    }
    hms(secs as u64)
}

/// Parse a user-entered time string into seconds.
///
/// Accepts plain decimal seconds (`"123.5"`), `M:SS` form (`"2:30"`), and
/// `H:MM:SS` form (`"1:02:03"`). Negative plain seconds clamp to zero. Clock
/// fields take digits only, with seconds (and minutes after hours) below 60;
/// non-finite numbers and any other shape return [`None`].
#[must_use]
pub fn parse_playback_time(input: &str) -> Option<f64> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(secs) = input.parse::<f64>() {
        return secs.is_finite().then(|| secs.max(0.0));
    }
    let parts: Vec<&str> = input.split(':').collect();
    match parts.as_slice() {
        [minutes, seconds] => {
            Some(f64::from(clock_count(minutes)?) * 60.0 + clock_seconds(seconds)?)
        }
        [hours, minutes, seconds] => {
            let minutes = clock_count(minutes).filter(|minutes| *minutes < 60)?;
            Some(
                f64::from(clock_count(hours)?) * 3600.0
                    + f64::from(minutes) * 60.0
                    + clock_seconds(seconds)?,
            )
        }
        _ => None,
    }
}

fn clock_count(field: &str) -> Option<u32> {
    if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    field.parse().ok()
}

fn clock_seconds(field: &str) -> Option<f64> {
    if !field
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    field.parse::<f64>().ok().filter(|seconds| *seconds < 60.0)
}

/// Format a countdown label as zero-padded `MM:SS`.
#[must_use]
pub fn countdown_mm_ss(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn hms(total: u64) -> String {
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_hm_formats_library_totals() {
        assert_eq!(duration_hm(0.0), "0 s");
        assert_eq!(duration_hm(40.0), "40 s");
        assert_eq!(duration_hm(60.0), "1 min");
        assert_eq!(duration_hm(300.0), "5 min");
        assert_eq!(duration_hm(3900.0), "1h 05min");
    }

    #[test]
    fn duration_hms_hides_invalid_values() {
        assert_eq!(duration_hms(0.0), "");
        assert_eq!(duration_hms(185.0), "3:05");
        assert_eq!(duration_hms(3661.0), "1:01:01");
        assert_eq!(duration_hms(f64::NAN), "");
    }

    #[test]
    fn playback_time_has_visible_zero_fallback() {
        assert_eq!(playback_time(0.0), "0:00");
        assert_eq!(playback_time(125.0), "2:05");
        assert_eq!(playback_time(3661.0), "1:01:01");
        assert_eq!(playback_time(f64::INFINITY), "0:00");
    }

    #[test]
    fn parses_seconds_minutes_and_hours() {
        assert_eq!(parse_playback_time("123.5"), Some(123.5));
        assert_eq!(parse_playback_time("2:30"), Some(150.0));
        assert_eq!(parse_playback_time("1:02:03"), Some(3723.0));
        assert_eq!(parse_playback_time("-5"), Some(0.0));
        assert_eq!(parse_playback_time("abc:def"), None);
        assert_eq!(parse_playback_time(""), None);
        assert_eq!(parse_playback_time(" 2:30 "), Some(150.0));
        assert_eq!(parse_playback_time("   "), None);
    }

    /// Oracle: `playback_time` is the inverse for whole seconds up to 100 hours.
    #[test]
    fn parse_inverts_playback_time() {
        for secs in 0..360_000_u32 {
            let text = playback_time(f64::from(secs));
            assert_eq!(parse_playback_time(&text), Some(f64::from(secs)), "{text}");
        }
    }

    #[test]
    fn rejects_malformed_times() {
        for input in [
            "inf", "-inf", "1e400", "NaN", "1:99", "1.5:30", "1:60:00", "-1:30", "1:-30", "1:3e1",
            "1::30", ":30", "1:60",
        ] {
            assert_eq!(parse_playback_time(input), None, "{input}");
        }
    }

    #[test]
    fn countdown_zero_pads_both_fields() {
        assert_eq!(countdown_mm_ss(0), "00:00");
        assert_eq!(countdown_mm_ss(65), "01:05");
        assert_eq!(countdown_mm_ss(3600), "60:00");
    }
}
