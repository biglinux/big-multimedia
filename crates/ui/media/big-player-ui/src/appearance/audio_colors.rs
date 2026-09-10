//! Audio-player color preferences page.

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use std::rc::Rc;

use super::color_palette_controls::{
    CustomColorKeys, add_custom_color_group, color_source_group, colors_page,
};
use crate::illustration::illustration_auto as illustration;

/// Build the audio-player Appearance → Colors page.
pub fn create_audio_colors_page(
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let page = colors_page();
    let source_group = color_source_group();

    let source_values: [&str; 4] = ["gtk-default", "gtk-light", "gtk-dark", "custom"];
    let current_source = settings.get_string(keys::AUDIO_UI_COLOR_SOURCE);
    let idx = source_values
        .iter()
        .position(|&value| value == current_source)
        .unwrap_or(0) as u32;
    let source_dropdown_row = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Color Source"),
            [
                i18n("Follow System"),
                i18n("Light Theme"),
                i18n("Dark Theme"),
                i18n("Custom Colors"),
            ],
        )
        .subtitle(i18n(
            "Follow System uses your desktop theme - Custom Colors lets you pick your own palette",
        ))
        .selected(idx),
    );
    let (source_row, source_dropdown) = source_dropdown_row.into_parts();
    source_row.add_prefix(&illustration("colors.svg"));
    source_group.add(&source_row);
    page.add(&source_group);

    let custom_group = add_custom_color_group(
        &page,
        settings,
        on_changed,
        CustomColorKeys {
            background: "audio-ui-bg-color",
            font: "audio-ui-font-color",
            icon: "audio-ui-icon-color",
            dialog: "audio-ui-dialog-color",
        },
        &current_source,
        None,
    );

    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        source_dropdown.connect_selected_notify(move |dropdown| {
            let selected = dropdown.selected() as usize;
            let value = source_values.get(selected).copied().unwrap_or("custom");
            let _ = settings.put_string(keys::AUDIO_UI_COLOR_SOURCE, value);
            custom_group.set_visible(value == "custom");
            on_changed();
        });
    }

    let a11y_group = BigPreferencesGroupSpec::new(i18n("Accessibility"))
        .description(i18n("Improve readability of secondary text and labels"))
        .build();

    let high_contrast_row = adw::SwitchRow::builder()
        .title(i18n("High Contrast Text"))
        .subtitle(i18n(
            "Makes dim labels, subtitles and secondary text easier to read",
        ))
        .active(settings.get_bool(keys::AUDIO_HIGH_CONTRAST))
        .build();
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        high_contrast_row.connect_active_notify(move |row| {
            let _ = settings.put_bool(keys::AUDIO_HIGH_CONTRAST, row.is_active());
            on_changed();
        });
    }
    a11y_group.add(&high_contrast_row);
    page.add(&a11y_group);

    page
}
