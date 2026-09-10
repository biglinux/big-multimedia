//! Compat facade — implementations live in the concern modules
//! (spawn/illustration/play_button/column_persist/dialogs) or the shared
//! component crates;
//! kept so existing `ui_helpers::*` call sites keep working.

pub use super::column_persist::*;
pub use super::dialogs::*;
pub use super::illustration::*;
pub use super::play_button::*;
pub use super::spawn::*;
pub use big_media_components::playback::icons::{
    VOLUME_THRESHOLD_HIGH, VOLUME_THRESHOLD_LOW, playback_icon,
    sample_rate_khz_label as format_sample_rate,
    unmuted_volume_icon_name as volume_icon_name_unmute, volume_icon_name,
};
pub use big_media_components::time::{
    duration_hm as format_duration_hm, duration_hms as format_duration_hms,
    playback_time as format_time,
};
pub use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;
pub use big_relm4_components::input::scale_trough::{
    update_scale_trough, voice_trough_color, volume_trough_color,
};

#[cfg(test)]
mod tests {
    use super::*;

    // ── volume_trough_color ──

    #[test]
    fn volume_at_zero() {
        assert_eq!(volume_trough_color(0.0, 200.0), "@accent_bg_color");
    }

    #[test]
    fn volume_at_100() {
        assert_eq!(volume_trough_color(100.0, 200.0), "@accent_bg_color");
    }

    #[test]
    fn volume_midpoint_between_100_and_max() {
        // max=200, mid=150, v=150 → t=1.0
        let c = volume_trough_color(150.0, 200.0);
        assert!(
            c.contains("mix(@accent_bg_color, @warning_bg_color,"),
            "{c}"
        );
    }

    #[test]
    fn volume_above_midpoint() {
        // max=200, mid=150, v=175 → t=0.5
        let c = volume_trough_color(175.0, 200.0);
        assert!(c.contains("mix(@warning_bg_color, @error_bg_color,"), "{c}");
    }

    #[test]
    fn volume_at_max() {
        let c = volume_trough_color(200.0, 200.0);
        assert!(
            c.contains("mix(@warning_bg_color, @error_bg_color, 1.00)"),
            "{c}"
        );
    }

    // ── voice_trough_color ──

    #[test]
    fn voice_low_range() {
        assert_eq!(voice_trough_color(0.0), "@accent_bg_color");
        assert_eq!(voice_trough_color(50.0), "@accent_bg_color");
    }

    #[test]
    fn voice_mid_range() {
        let c = voice_trough_color(65.0);
        assert!(
            c.contains("mix(@accent_bg_color, @warning_bg_color, 0.50)"),
            "{c}"
        );
    }

    #[test]
    fn voice_high_range() {
        let c = voice_trough_color(105.0);
        assert!(
            c.contains("mix(@warning_bg_color, @error_bg_color, 0.50)"),
            "{c}"
        );
    }

    #[test]
    fn voice_at_max() {
        let c = voice_trough_color(130.0);
        assert!(
            c.contains("mix(@warning_bg_color, @error_bg_color, 1.00)"),
            "{c}"
        );
    }

    // ── format_duration_hm ──

    #[test]
    fn duration_hm_zero() {
        assert_eq!(format_duration_hm(0.0), "0 min");
    }

    #[test]
    fn duration_hm_minutes_only() {
        assert_eq!(format_duration_hm(300.0), "5 min");
    }

    #[test]
    fn duration_hm_with_hours() {
        assert_eq!(format_duration_hm(3900.0), "1h 05min");
    }

    // ── format_duration_hms ──

    #[test]
    fn duration_hms_zero() {
        assert_eq!(format_duration_hms(0.0), "");
    }

    #[test]
    fn duration_hms_minutes() {
        assert_eq!(format_duration_hms(185.0), "3:05");
    }

    #[test]
    fn duration_hms_with_hours() {
        assert_eq!(format_duration_hms(3661.0), "1:01:01");
    }

    #[test]
    fn duration_hms_nan() {
        assert_eq!(format_duration_hms(f64::NAN), "");
    }

    // ── format_time ──

    #[test]
    fn format_time_zero() {
        assert_eq!(format_time(0.0), "0:00");
    }

    #[test]
    fn format_time_seconds_only() {
        assert_eq!(format_time(45.0), "0:45");
    }

    #[test]
    fn format_time_minutes_and_seconds() {
        assert_eq!(format_time(125.0), "2:05");
    }

    #[test]
    fn format_time_exact_minute() {
        assert_eq!(format_time(60.0), "1:00");
    }

    #[test]
    fn format_time_large() {
        assert_eq!(format_time(3661.0), "1:01:01");
    }

    #[test]
    fn format_time_nan() {
        assert_eq!(format_time(f64::NAN), "0:00");
    }

    #[test]
    fn format_time_negative() {
        assert_eq!(format_time(-5.0), "0:00");
    }

    #[test]
    fn format_time_infinity() {
        assert_eq!(format_time(f64::INFINITY), "0:00");
    }

    // ── format_sample_rate ──

    #[test]
    fn sample_rate_44100() {
        assert_eq!(format_sample_rate(44100), "44.1");
    }

    #[test]
    fn sample_rate_48000() {
        assert_eq!(format_sample_rate(48000), "48");
    }

    #[test]
    fn sample_rate_96000() {
        assert_eq!(format_sample_rate(96000), "96");
    }

    #[test]
    fn sample_rate_192000() {
        assert_eq!(format_sample_rate(192_000), "192");
    }

    #[test]
    fn sample_rate_22050() {
        assert_eq!(format_sample_rate(22050), "22.1");
    }

    // Sidebar info bar builds "{count} track(s) · {duration}";
    // these lock the format expected by window_detail.rs.

    #[test]
    fn sidebar_status_single_track() {
        let count = 1;
        let total_secs: f64 = 240.0;
        let duration = format_duration_hm(total_secs);
        let status = format!(
            "{} {} · {}",
            count,
            if count == 1 { "track" } else { "tracks" },
            duration,
        );
        assert_eq!(status, "1 track · 4 min");
    }

    #[test]
    fn sidebar_status_multiple_tracks() {
        let count = 42;
        let total_secs: f64 = 7200.0; // 2 hours
        let duration = format_duration_hm(total_secs);
        let status = format!(
            "{} {} · {}",
            count,
            if count == 1 { "track" } else { "tracks" },
            duration,
        );
        assert_eq!(status, "42 tracks · 2h 00min");
    }

    #[test]
    fn sidebar_status_empty() {
        let count = 0;
        let total_secs: f64 = 0.0;
        let duration = format_duration_hm(total_secs);
        let status = format!(
            "{} {} · {}",
            count,
            if count == 1 { "track" } else { "tracks" },
            duration,
        );
        assert_eq!(status, "0 tracks · 0 min");
    }
}
