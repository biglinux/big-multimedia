//! Shared color-page helpers for audio and video appearance dialogs.

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_media_data::settings::Settings;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::theme::parse_hex_to_rgba;
use gtk::gdk;
use std::rc::Rc;

pub(super) struct ColorPreset {
    pub(super) bg: &'static str,
    pub(super) font: &'static str,
    pub(super) icon: &'static str,
    pub(super) dialog: &'static str,
}

pub(super) const PRESET_LABELS: [&str; 12] = [
    "Custom",
    "Light",
    "Dark",
    "Solarized Light",
    "Solarized Dark",
    "Monokai",
    "Dracula",
    "Nord",
    "Tokyo Night",
    "Catppuccin Mocha",
    "Rose Pine",
    "Pink Light",
];

pub(super) const PRESETS: [ColorPreset; 11] = [
    ColorPreset {
        bg: "#FFFFFF",
        font: "#000000",
        icon: "#000000",
        dialog: "#F0F0F0",
    },
    ColorPreset {
        bg: "#1C1C1C",
        font: "#FFFFFF",
        icon: "#FFFFFF",
        dialog: "#2A2A2A",
    },
    ColorPreset {
        bg: "#FDF6E3",
        font: "#073642",
        icon: "#0B6E4F",
        dialog: "#EEE8D5",
    },
    ColorPreset {
        bg: "#002B36",
        font: "#93A1A1",
        icon: "#268BD2",
        dialog: "#073642",
    },
    ColorPreset {
        bg: "#272822",
        font: "#F8F8F2",
        icon: "#66D9EF",
        dialog: "#3A3B34",
    },
    ColorPreset {
        bg: "#282A36",
        font: "#F8F8F2",
        icon: "#BD93F9",
        dialog: "#3A3C4E",
    },
    ColorPreset {
        bg: "#2E3440",
        font: "#D8DEE9",
        icon: "#88C0D0",
        dialog: "#3B4252",
    },
    ColorPreset {
        bg: "#1A1B26",
        font: "#C0CAF5",
        icon: "#7AA2F7",
        dialog: "#1F2335",
    },
    ColorPreset {
        bg: "#1E1E2E",
        font: "#F5E0DC",
        icon: "#89B4FA",
        dialog: "#181825",
    },
    ColorPreset {
        bg: "#191724",
        font: "#E0DEF4",
        icon: "#EBBCBA",
        dialog: "#1F1D2E",
    },
    ColorPreset {
        bg: "#FFE4EC",
        font: "#4A2040",
        icon: "#D63384",
        dialog: "#FFD0DD",
    },
];

pub(super) fn colors_page() -> adw::PreferencesPage {
    adw::PreferencesPage::builder()
        .title(i18n("Colors"))
        .icon_name("preferences-color-symbolic")
        .build()
}

/// Untitled: its first row is named "Color Source" already.
pub(super) fn color_source_group() -> adw::PreferencesGroup {
    adw::PreferencesGroup::new()
}

pub(super) struct CustomColorKeys {
    pub(super) background: &'static str,
    pub(super) font: &'static str,
    pub(super) icon: &'static str,
    pub(super) dialog: &'static str,
}

pub(super) fn add_custom_color_group(
    page: &adw::PreferencesPage,
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
    keys: CustomColorKeys,
    current_source: &str,
) -> adw::PreferencesGroup {
    let custom_group = BigPreferencesGroupSpec::new(i18n("Custom Colors")).build();

    let preset_labels: Vec<String> = PRESET_LABELS.iter().map(|label| i18n(label)).collect();
    let preset_dropdown_row = BigDropdownRow::new(
        BigDropdownRowSpec::new(i18n("Color Preset"), preset_labels).selected(0),
    );
    let (preset_row, preset_dropdown) = preset_dropdown_row.into_parts();
    custom_group.add(&preset_row);

    let bg_button = add_color_row(
        &custom_group,
        &i18n("Background Color"),
        keys.background,
        settings,
        on_changed,
    );
    let fg_button = add_color_row(
        &custom_group,
        &i18n("Font Color"),
        keys.font,
        settings,
        on_changed,
    );
    let icon_button = add_color_row(
        &custom_group,
        &i18n("Icon Color"),
        keys.icon,
        settings,
        on_changed,
    );
    let dialog_button = add_color_row(
        &custom_group,
        &i18n("Dialogs and Menus Color"),
        keys.dialog,
        settings,
        on_changed,
    );

    preset_dropdown.connect_selected_notify(move |dropdown| {
        let idx = dropdown.selected() as usize;
        if idx == 0 {
            return;
        }
        if let Some(preset) = PRESETS.get(idx - 1) {
            bg_button.set_rgba(&parse_hex_to_rgba(preset.bg));
            fg_button.set_rgba(&parse_hex_to_rgba(preset.font));
            icon_button.set_rgba(&parse_hex_to_rgba(preset.icon));
            dialog_button.set_rgba(&parse_hex_to_rgba(preset.dialog));
        }
    });

    custom_group.set_visible(current_source == "custom");
    page.add(&custom_group);
    custom_group
}

fn rgba_to_hex(rgba: gdk::RGBA) -> String {
    let red = (rgba.red() * 255.0) as u8;
    let green = (rgba.green() * 255.0) as u8;
    let blue = (rgba.blue() * 255.0) as u8;
    format!("#{red:02X}{green:02X}{blue:02X}")
}

fn add_color_row(
    group: &adw::PreferencesGroup,
    title: &str,
    key: &str,
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> gtk::ColorDialogButton {
    let button = gtk::ColorDialogButton::builder()
        .valign(gtk::Align::Center)
        .build();
    button.set_dialog(&gtk::ColorDialog::new());
    button.set_rgba(&parse_hex_to_rgba(&settings.string(key)));
    let row_builder = BigButtonStripRow::new(BigButtonStripRowSpec::new(title));
    row_builder.append(&button);
    row_builder.root().set_activatable_widget(Some(&button));
    let settings = settings.clone();
    let key = key.to_string();
    let on_changed = on_changed.clone();
    button.connect_rgba_notify(move |button| {
        let _ = settings.set_string(&key, &rgba_to_hex(button.rgba()));
        on_changed();
    });
    let row = row_builder.into_root();
    group.add(&row);
    button
}

#[cfg(test)]
mod tests {
    use super::{PRESET_LABELS, PRESETS, rgba_to_hex};

    #[test]
    fn hex_drops_alpha() {
        let rgba = gtk::gdk::RGBA::new(0.5, 0.25, 1.0, 0.1);
        assert_eq!(rgba_to_hex(rgba), "#7F3FFF");
    }

    #[test]
    fn preset_arrays_stay_aligned() {
        assert_eq!(PRESET_LABELS.len(), PRESETS.len() + 1);
    }
}
