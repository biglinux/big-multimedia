//! App settings adapter for shared libadwaita theme helpers.
//! bigagents: app-local-adapter; binds shared theme helpers to app settings keys.

pub use big_relm4_components::theme::{
    ColorPalette, HIGH_CONTRAST_CSS, apply_color_scheme, build_adwaita_color_overrides,
    inject_palette_css, parse_hex_color, parse_hex_to_rgba,
};

/// `Some(css)` only when `color_source == "custom"`; callers fall back on `None`.
pub fn build_custom_palette_css(
    settings: &crate::settings::Settings,
    color_source: &str,
    bg_key: crate::settings::keys::StringKey,
    fg_key: crate::settings::keys::StringKey,
    icon_key: crate::settings::keys::StringKey,
    dialog_key: crate::settings::keys::StringKey,
) -> Option<String> {
    if color_source != "custom" {
        return None;
    }
    let palette = ColorPalette {
        bg: parse_hex_color(&settings.get_string(bg_key)),
        fg: parse_hex_color(&settings.get_string(fg_key)),
        icon: parse_hex_color(&settings.get_string(icon_key)),
        dialog: parse_hex_color(&settings.get_string(dialog_key)),
    };
    Some(build_adwaita_color_overrides(&palette))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_custom_palette_returns_none_for_gtk_default() {
        let settings = crate::settings::Settings::new("test");
        let result = build_custom_palette_css(
            &settings,
            "gtk-default",
            crate::settings::keys::AUDIO_UI_BG_COLOR,
            crate::settings::keys::AUDIO_UI_FONT_COLOR,
            crate::settings::keys::AUDIO_UI_ICON_COLOR,
            crate::settings::keys::AUDIO_UI_DIALOG_COLOR,
        );
        assert!(result.is_none());
    }
}
