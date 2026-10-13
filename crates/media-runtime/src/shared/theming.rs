//! App settings adapter for shared libadwaita theme helpers.
//! bigagents: app-local-adapter; binds shared theme helpers to app settings keys.

pub use big_relm4_components::theme::{
    ColorPalette, HIGH_CONTRAST_CSS, apply_color_scheme, build_adwaita_color_overrides,
    inject_palette_css, parse_hex_color, parse_hex_to_rgba,
};

/// libadwaita's palette for the scheme in force. libadwaita derives these
/// variables from named colours that follow the scheme; a desktop theme that
/// pins the variables as user CSS (BigStyle writes the system's light ones)
/// would leave light-theme colours on a forced dark scheme.
const SCHEME_PALETTE_CSS: &str = "\
:root {
  --window-bg-color: @window_bg_color;
  --window-fg-color: @window_fg_color;
  --view-bg-color: @view_bg_color;
  --view-fg-color: @view_fg_color;
  --headerbar-bg-color: @headerbar_bg_color;
  --headerbar-fg-color: @headerbar_fg_color;
  --headerbar-border-color: @headerbar_border_color;
  --headerbar-backdrop-color: @headerbar_backdrop_color;
  --headerbar-shade-color: @headerbar_shade_color;
  --headerbar-darker-shade-color: @headerbar_darker_shade_color;
  --sidebar-bg-color: @sidebar_bg_color;
  --sidebar-fg-color: @sidebar_fg_color;
  --sidebar-backdrop-color: @sidebar_backdrop_color;
  --sidebar-border-color: @sidebar_border_color;
  --sidebar-shade-color: @sidebar_shade_color;
  --secondary-sidebar-bg-color: @secondary_sidebar_bg_color;
  --secondary-sidebar-fg-color: @secondary_sidebar_fg_color;
  --secondary-sidebar-backdrop-color: @secondary_sidebar_backdrop_color;
  --secondary-sidebar-border-color: @secondary_sidebar_border_color;
  --secondary-sidebar-shade-color: @secondary_sidebar_shade_color;
  --card-bg-color: @card_bg_color;
  --card-fg-color: @card_fg_color;
  --card-shade-color: @card_shade_color;
  --dialog-bg-color: @dialog_bg_color;
  --dialog-fg-color: @dialog_fg_color;
  --popover-bg-color: @popover_bg_color;
  --popover-fg-color: @popover_fg_color;
  --popover-shade-color: @popover_shade_color;
  --thumbnail-bg-color: @thumbnail_bg_color;
  --thumbnail-fg-color: @thumbnail_fg_color;
  --shade-color: @shade_color;
  --scrollbar-outline-color: @scrollbar_outline_color;
}
@define-color player_icon_color @window_fg_color;
";

/// The palette CSS for `color_source`: the user's colours for `"custom"`,
/// libadwaita's own for a forced light or dark scheme, `None` to follow the
/// system.
pub fn build_palette_css(
    settings: &crate::settings::Settings,
    color_source: &str,
    bg_key: crate::settings::keys::StringKey,
    fg_key: crate::settings::keys::StringKey,
    icon_key: crate::settings::keys::StringKey,
    dialog_key: crate::settings::keys::StringKey,
) -> Option<String> {
    match color_source {
        "custom" => Some(build_adwaita_color_overrides(&ColorPalette {
            bg: parse_hex_color(&settings.get_string(bg_key)),
            fg: parse_hex_color(&settings.get_string(fg_key)),
            icon: parse_hex_color(&settings.get_string(icon_key)),
            dialog: parse_hex_color(&settings.get_string(dialog_key)),
        })),
        "gtk-light" | "gtk-dark" | "gradient" => Some(SCHEME_PALETTE_CSS.to_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_colors_bring_no_palette() {
        let settings = crate::settings::Settings::new("test");
        let result = build_palette_css(
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
