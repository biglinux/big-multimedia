//! Edit-page timestamp formatting (seconds → `H:MM:SS.mmm`).

use big_media_components::time::precise_time_ms;

#[must_use]
pub(super) fn format_time_ms(secs: f64) -> String {
    precise_time_ms(secs)
}

#[cfg(test)]
mod tests {
    use super::format_time_ms;

    #[test]
    fn format_time_ms_handles_hour_minute_second_samples() {
        assert_eq!(format_time_ms(0.0), "0:00:00.000");
        assert_eq!(format_time_ms(61.234), "0:01:01.234");
        assert_eq!(format_time_ms(3661.999), "1:01:01.999");
    }

    #[test]
    fn format_time_ms_sanitizes_invalid_input() {
        assert_eq!(format_time_ms(f64::NAN), "0:00:00.000");
        assert_eq!(format_time_ms(-1.0), "0:00:00.000");
    }
}
