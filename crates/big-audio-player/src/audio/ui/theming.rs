//! Audio-player appearance application. Reads the `audio-ui-*` settings and
//! pushes the resulting palette + high-contrast CSS into the shell's two
//! [`CssSlot`]s. Kept app-local because the palette keys are audio-specific (the
//! video player keeps a separate set).

use big_relm4_components::theme::{CssSlot, HIGH_CONTRAST_CSS};

use crate::settings::{Settings, keys};

/// Apply the saved color source, custom palette, and high-contrast override.
pub(crate) fn apply_appearance(settings: &Settings, theme_slot: &CssSlot, hc_slot: &CssSlot) {
    let color_source = settings.get_string(keys::AUDIO_UI_COLOR_SOURCE);
    big_media_runtime::theming::apply_color_scheme(
        settings,
        &color_source,
        keys::AUDIO_UI_BG_COLOR,
    );

    theme_slot.set(&palette_css(settings, &color_source));

    // Empty string clears any prior high-contrast override from the provider.
    let hc_css = if settings.get_bool(keys::AUDIO_HIGH_CONTRAST) {
        HIGH_CONTRAST_CSS
    } else {
        ""
    };
    hc_slot.set(hc_css);
}

/// The palette for `color_source`, translucent when the setting asks for it.
fn palette_css(settings: &Settings, color_source: &str) -> String {
    let css = big_media_runtime::theming::build_palette_css(
        settings,
        color_source,
        keys::AUDIO_UI_BG_COLOR,
        keys::AUDIO_UI_FONT_COLOR,
        keys::AUDIO_UI_ICON_COLOR,
        keys::AUDIO_UI_DIALOG_COLOR,
    )
    .unwrap_or_default();
    if settings.get_bool(keys::AUDIO_UI_TRANSLUCENT) {
        css + &translucent_css(settings.get_double(keys::AUDIO_UI_BG_OPACITY))
    } else {
        css
    }
}

/// Lowest translucent background opacity; below it text loses its ground.
const MIN_TRANSLUCENT_OPACITY: f64 = 0.3;

/// The window's background at `opacity` over what is behind it, blurred by the
/// compositor: GTK asks for the blur behind a surface whose backdrop filter is
/// 20 px or more. The panes on it draw no background of their own, so the
/// window is translucent once, not once per layer.
fn translucent_css(opacity: f64) -> String {
    let percent = (opacity.clamp(MIN_TRANSLUCENT_OPACITY, 1.0) * 100.0).round();
    format!(
        ":root {{
  --window-bg-color: color-mix(in srgb, @window_bg_color {percent}%, transparent);
  --view-bg-color: transparent;
  --headerbar-bg-color: transparent;
  --headerbar-backdrop-color: transparent;
  --sidebar-bg-color: color-mix(in srgb, @window_fg_color 4%, transparent);
  --sidebar-backdrop-color: color-mix(in srgb, @window_fg_color 4%, transparent);
  --secondary-sidebar-bg-color: color-mix(in srgb, @window_fg_color 4%, transparent);
  --secondary-sidebar-backdrop-color: color-mix(in srgb, @window_fg_color 4%, transparent);
}}
window.background {{ backdrop-filter: blur(24px); }}
"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucency_follows_its_settings() {
        let settings = Settings::memory_only();
        assert!(!palette_css(&settings, "gtk-default").contains("backdrop-filter"));
        settings.put_bool(keys::AUDIO_UI_TRANSLUCENT, true);
        settings.put_double(keys::AUDIO_UI_BG_OPACITY, 0.6);
        let css = palette_css(&settings, "gtk-default");
        assert!(css.contains("@window_bg_color 60%"), "{css}");
        assert!(css.contains("backdrop-filter: blur(24px)"));
        settings.put_double(keys::AUDIO_UI_BG_OPACITY, 0.1);
        assert!(palette_css(&settings, "custom").contains("@window_bg_color 30%"));
    }
}
