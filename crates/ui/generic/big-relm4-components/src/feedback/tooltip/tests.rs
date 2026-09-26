use super::*;

#[test]
fn config_defaults_match_biglinux_policy() {
    assert_eq!(
        BigTooltipConfig::default(),
        BigTooltipConfig {
            show_delay_ms: 150,
            fade_out_ms: 300,
            css_fade_ms: 200,
            max_width_chars: 80,
        }
    );
}

#[test]
fn adjust_dark_background_lightens() {
    assert_eq!(adjust_tooltip_background("#1a1a1a"), "#424242");
}

#[test]
fn adjust_light_background_darkens() {
    assert_eq!(adjust_tooltip_background("#ffffff"), "#ebebeb");
}

#[test]
fn adjust_clamps_lighten_overflow() {
    assert_eq!(adjust_tooltip_background("#ff0000"), "#ff2828");
}

#[test]
fn adjust_handles_hash_prefix() {
    assert_eq!(
        adjust_tooltip_background("#1a1a1a"),
        adjust_tooltip_background("1a1a1a")
    );
}

#[test]
fn adjust_returns_original_on_invalid_input() {
    assert_eq!(adjust_tooltip_background("#abc"), "#abc");
    assert_eq!(adjust_tooltip_background("#gggggg"), "#gggggg");
}

#[test]
fn dark_color_classification_is_conservative() {
    assert!(is_dark_color("#000000"));
    assert!(is_dark_color("#1a1a1a"));
    assert!(is_dark_color("#abc"));
    assert!(!is_dark_color("#ffffff"));
    assert!(!is_dark_color("#fafafa"));
}
