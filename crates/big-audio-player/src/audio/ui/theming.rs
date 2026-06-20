//! Audio-player appearance application. Reads the `audio-ui-*` settings and
//! pushes the resulting palette + high-contrast CSS into two app-owned
//! `CssProvider` slots. Mirrors the god-object's `apply_appearance`; kept app-
//! local because the palette keys are audio-specific (the video player keeps a
//! separate set). The provider slots live on the `AudioShell` so a re-apply
//! (from the Appearance dialog's `on_changed`) replaces, never stacks.

use big_relm4_components::theme::{apply_color_scheme, inject_palette_css, HIGH_CONTRAST_CSS};

use crate::settings::{keys, Settings};

/// Apply the saved colour source, custom palette, and high-contrast override.
/// `theme_slot`/`hc_slot` are the persistent provider slots (replaced in place).
pub(crate) fn apply_appearance(
    settings: &Settings,
    theme_slot: &mut Option<gtk::CssProvider>,
    hc_slot: &mut Option<gtk::CssProvider>,
) {
    let color_source = settings.get_string(keys::AUDIO_UI_COLOR_SOURCE);
    apply_color_scheme(&color_source);

    let css = crate::shared::theming::build_custom_palette_css(
        settings,
        &color_source,
        keys::AUDIO_UI_BG_COLOR,
        keys::AUDIO_UI_FONT_COLOR,
        keys::AUDIO_UI_ICON_COLOR,
        keys::AUDIO_UI_DIALOG_COLOR,
    )
    .unwrap_or_default();
    inject_palette_css(&css, theme_slot);

    // Empty string clears any prior high-contrast override from the provider.
    let hc_css = if settings.get_bool(keys::AUDIO_HIGH_CONTRAST) {
        HIGH_CONTRAST_CSS
    } else {
        ""
    };
    inject_palette_css(hc_css, hc_slot);
}
