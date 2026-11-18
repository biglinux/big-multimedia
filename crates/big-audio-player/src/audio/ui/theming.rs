//! Audio-player appearance application. Reads the `audio-ui-*` settings and
//! pushes the resulting palette + high-contrast CSS into the shell's two
//! [`CssSlot`]s. Kept app-local because the palette keys are audio-specific (the
//! video player keeps a separate set).

use big_relm4_components::theme::{CssSlot, HIGH_CONTRAST_CSS, apply_color_scheme};

use crate::settings::{Settings, keys};

/// Apply the saved color source, custom palette, and high-contrast override.
pub(crate) fn apply_appearance(settings: &Settings, theme_slot: &CssSlot, hc_slot: &CssSlot) {
    let color_source = settings.get_string(keys::AUDIO_UI_COLOR_SOURCE);
    apply_color_scheme(&color_source);

    let css = crate::shared::theming::build_palette_css(
        settings,
        &color_source,
        keys::AUDIO_UI_BG_COLOR,
        keys::AUDIO_UI_FONT_COLOR,
        keys::AUDIO_UI_ICON_COLOR,
        keys::AUDIO_UI_DIALOG_COLOR,
    )
    .unwrap_or_default();
    theme_slot.set(&css);

    // Empty string clears any prior high-contrast override from the provider.
    let hc_css = if settings.get_bool(keys::AUDIO_HIGH_CONTRAST) {
        HIGH_CONTRAST_CSS
    } else {
        ""
    };
    hc_slot.set(hc_css);
}
