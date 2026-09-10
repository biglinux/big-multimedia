//! Video-player color preferences page.

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_media_data::settings::{Settings, keys};
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::input::preference_cards::{
    BigSpinPreferenceCard, BigSpinRange, BigSwitchPreferenceCard,
};
use big_relm4_components::layout::didactic_card::BigDidacticCardSpec;
use std::rc::Rc;

use super::color_palette_controls::{
    CustomColorKeys, add_custom_color_group, color_source_group, colors_page,
};
use crate::illustration::{illustration_auto as illustration, illustrations_prefix};

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
        .subtitle(i18n(
            "Follow System uses your desktop theme - Video Colors extracts colors from the current video",
        ))
        .selected(idx),
    );
    let (source_row, source_dropdown) = source_dropdown_row.into_parts();
    source_row.add_prefix(&illustration("colors.svg"));
    source_group.add(&source_row);

    let opacity_card = BigSpinPreferenceCard::new(
        card_spec(
            "opacity.svg",
            i18n("Background Opacity"),
            i18n(
                "How transparent the player background is - lower values let your desktop show through",
            ),
        ),
        settings.get_double(keys::UI_BG_OPACITY),
        BigSpinRange::new(0.0, 1.0, 0.05, 0.1),
        2,
    );
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        opacity_card.spin().connect_value_changed(move |spin| {
            let _ = settings.put_double(keys::UI_BG_OPACITY, spin.value());
            on_changed();
        });
    }
    source_group.add(opacity_card.root());

    let menu_opacity_card = BigSpinPreferenceCard::new(
        card_spec(
            "opacity.svg",
            i18n("Menu Opacity"),
            i18n("How transparent menus and dialogs appear"),
        ),
        settings.get_double(keys::UI_MENU_OPACITY),
        BigSpinRange::new(0.0, 1.0, 0.05, 0.1),
        2,
    );
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        menu_opacity_card.spin().connect_value_changed(move |spin| {
            let _ = settings.put_double(keys::UI_MENU_OPACITY, spin.value());
            on_changed();
        });
    }
    source_group.add(menu_opacity_card.root());

    let shadow_card = BigSwitchPreferenceCard::new(
        card_spec(
            "shadow.svg",
            i18n("Icon and Text Shadow"),
            i18n("Makes controls easier to see when the background is transparent or very bright"),
        ),
        settings.get_bool(keys::UI_OSD_SHADOW),
    );
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        shadow_card.switch().connect_active_notify(move |switch| {
            let _ = settings.put_bool(keys::UI_OSD_SHADOW, switch.is_active());
            on_changed();
        });
    }
    source_group.add(shadow_card.root());
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
        Some("color_preset.svg"),
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

fn card_spec(
    illustration_name: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
) -> BigDidacticCardSpec {
    BigDidacticCardSpec::new(
        illustrations_prefix(),
        illustration_name,
        title,
        description,
    )
}
