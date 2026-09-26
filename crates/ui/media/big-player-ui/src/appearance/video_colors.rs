//! Video-player color preferences page.

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use std::rc::Rc;

use super::color_palette_controls::{
    CustomColorKeys, add_custom_color_group, color_source_group, colors_page,
};

/// Build the video-player Appearance → Colors page.
pub fn create_video_colors_page(
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let page = colors_page();
    let source_group = color_source_group();

    let source_values: [&str; 5] = ["gtk-default", "gtk-light", "gtk-dark", "gradient", "custom"];
    let current_source = settings.get_string(keys::UI_COLOR_SOURCE);
    let idx = source_values
        .iter()
        .position(|&value| value == current_source)
        .unwrap_or(3) as u32;
    let source_dropdown_row = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Color Source"),
            [
                i18n("Follow System"),
                i18n("Light Theme"),
                i18n("Dark Theme"),
                i18n("Video Colors"),
                i18n("Custom Colors"),
            ],
        )
        .selected(idx),
    );
    let (source_row, source_dropdown) = source_dropdown_row.into_parts();
    source_group.add(&source_row);

    for (title, key) in [
        (i18n("Background Opacity"), keys::UI_BG_OPACITY),
        (i18n("Menu Opacity"), keys::UI_MENU_OPACITY),
    ] {
        let row = adw::SpinRow::builder()
            .title(title)
            .digits(2)
            .adjustment(&gtk::Adjustment::new(
                settings.get_double(key),
                0.0,
                1.0,
                0.05,
                0.1,
                0.0,
            ))
            .build();
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        row.connect_value_notify(move |row| {
            let _ = settings.put_double(key, row.value());
            on_changed();
        });
        source_group.add(&row);
    }

    let shadow_row = adw::SwitchRow::builder()
        .title(i18n("Icon and Text Shadow"))
        .active(settings.get_bool(keys::UI_OSD_SHADOW))
        .build();
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        shadow_row.connect_active_notify(move |row| {
            let _ = settings.put_bool(keys::UI_OSD_SHADOW, row.is_active());
            on_changed();
        });
    }
    source_group.add(&shadow_row);
    page.add(&source_group);

    let custom_group = add_custom_color_group(
        &page,
        settings,
        on_changed,
        CustomColorKeys {
            background: "ui-bg-color",
            font: "ui-font-color",
            icon: "ui-icon-color",
            dialog: "ui-dialog-color",
        },
        &current_source,
    );

    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        source_dropdown.connect_selected_notify(move |dropdown| {
            let selected = dropdown.selected() as usize;
            let value = source_values.get(selected).copied().unwrap_or("custom");
            let _ = settings.put_string(keys::UI_COLOR_SOURCE, value);
            custom_group.set_visible(value == "custom");
            on_changed();
        });
    }

    page
}
