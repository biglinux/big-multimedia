// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Scale trough CSS helpers.

use relm4::gtk;

/// Pick the trough fill colour for a volume slider that may exceed
/// 100 %. Normal range stays accent-coloured; the slider warms toward
/// warning then error as the value approaches `max_value`.
#[must_use]
pub fn volume_trough_color(value: f64, max_value: f64) -> String {
    tiered_trough_color(value, 100.0, f64::midpoint(100.0, max_value), max_value)
}

/// Pick the trough fill colour for a voice-volume slider where 50 %
/// is the recommended ceiling. Warmer colours warn the user above
/// 50 % and reach the error colour at `max_value`.
#[must_use]
pub fn voice_trough_color(value: f64, max_value: f64) -> String {
    tiered_trough_color(value, 50.0, 80.0, max_value)
}

/// Accent up to `safe`, blending to warning at `warning`, then to the full
/// error colour at `max_value`.
fn tiered_trough_color(value: f64, safe: f64, warning: f64, max_value: f64) -> String {
    if value <= safe {
        "@accent_bg_color".to_string()
    } else if value <= warning {
        let t = (value - safe) / (warning - safe);
        format!("mix(@accent_bg_color, @warning_bg_color, {t:.2})")
    } else {
        let t = ((value - warning) / (max_value - warning)).min(1.0);
        format!("mix(@warning_bg_color, @error_bg_color, {t:.2})")
    }
}

/// Push a new scale trough value through the live state.
pub fn update_scale_trough(provider: &gtk::CssProvider, css_class: &str, color: &str) {
    let css = format!(".{css_class} trough highlight {{ background: {color}; }}");
    provider.load_from_string(&css);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_color_uses_accent_at_normal_volume() {
        assert_eq!(volume_trough_color(0.0, 200.0), "@accent_bg_color");
        assert_eq!(volume_trough_color(100.0, 200.0), "@accent_bg_color");
    }

    #[test]
    fn volume_color_blends_to_warning_then_error() {
        assert!(
            volume_trough_color(150.0, 200.0).contains("mix(@accent_bg_color, @warning_bg_color,")
        );
        assert!(
            volume_trough_color(175.0, 200.0).contains("mix(@warning_bg_color, @error_bg_color,")
        );
        assert!(
            volume_trough_color(200.0, 200.0)
                .contains("mix(@warning_bg_color, @error_bg_color, 1.00)")
        );
    }

    #[test]
    fn voice_color_blends_by_known_ranges() {
        assert_eq!(voice_trough_color(50.0, 100.0), "@accent_bg_color");
        assert!(
            voice_trough_color(65.0, 100.0)
                .contains("mix(@accent_bg_color, @warning_bg_color, 0.50)")
        );
        assert!(
            voice_trough_color(90.0, 100.0)
                .contains("mix(@warning_bg_color, @error_bg_color, 0.50)")
        );
        assert!(
            voice_trough_color(100.0, 100.0)
                .contains("mix(@warning_bg_color, @error_bg_color, 1.00)")
        );
    }
}
