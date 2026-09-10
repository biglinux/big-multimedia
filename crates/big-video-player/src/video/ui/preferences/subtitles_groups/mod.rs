//! Subtitles preferences page: 5 PreferencesGroup builders. Font + colors +
//! language stay here; position + special-formats live in [`position`] /
//! [`special`] to keep this module under the 400-LOC budget. Advanced rows
//! tracked via `adv_widgets` → toggled by the page-level "Show advanced"
//! switch.

mod position;
mod special;

pub(in crate::video::ui::preferences) use position::build_position_rendering_group;
pub(in crate::video::ui::preferences) use special::build_special_formats_group;

// Drop unused parent re-exports below; the warning list above covers it.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{BigPreferencesGroupSpec, track_widget_visibility};
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use gtk::pango;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::AdvancedWidgets;
use super::{
    add_color_row, add_combo_row, add_entry_row, add_spin_double_row, add_spin_int_row,
    add_switch_row,
};

use crate::shared::ui_helpers::illustration_auto as illustration;

pub(super) fn build_font_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Font"))
        .description(i18n("Typeface and size for subtitle text"))
        .build();

    let font_dialog = gtk::FontDialog::new();
    let font_picker_button = gtk::FontDialogButton::new(Some(font_dialog));
    let current_font = settings.get_string(keys::SUB_FONT);
    if !current_font.is_empty() {
        let desc = pango::FontDescription::from_string(&current_font);
        font_picker_button.set_font_desc(&desc);
    }
    let font_row_builder = BigButtonStripRow::new(
        BigButtonStripRowSpec::new(i18n("Font Family"))
            .subtitle(i18n("Choose a specific typeface for subtitles")),
    );
    font_row_builder
        .root()
        .add_prefix(&illustration("font.svg"));
    font_row_builder.append(&font_picker_button);
    font_row_builder
        .root()
        .set_activatable_widget(Some(&font_picker_button));
    {
        let settings = settings.clone();
        let mpv = mpv.cloned();
        font_picker_button.connect_font_desc_notify(move |button| {
            if let Some(description) = button.font_desc() {
                let family = description
                    .family()
                    .map(|font_family| font_family.to_string())
                    .unwrap_or_default();
                let _ = settings.put_string(keys::SUB_FONT, &family);
                if let Some(ref mpv) = mpv {
                    mpv.set_sub_font(&family);
                }
            }
        });
    }
    let font_row = font_row_builder.into_root();
    group.add(&font_row);
    track_widget_visibility(&font_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let size_row = add_spin_int_row(
        &group,
        &i18n("Font Size"),
        &i18n("Larger values make subtitles easier to read from a distance"),
        "subtitle-font-size",
        10.0,
        100.0,
        1.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_font_size(v);
            }
        },
    );
    size_row.add_prefix(&illustration("font.svg"));

    let m = mpv.cloned();
    let bold_row = add_switch_row(
        &group,
        &i18n("Bold"),
        &i18n("Makes subtitle text thicker and easier to read"),
        "sub-bold",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_bold(v);
            }
        },
    );
    bold_row.add_prefix(&illustration("bold.svg"));

    group
}

pub(super) fn build_colors_outline_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Colors and Outline"))
        .description(i18n(
            "Customize how subtitles look for better readability on any background",
        ))
        .build();

    let m = mpv.cloned();
    let text_color_row = add_color_row(
        &group,
        &i18n("Text Color"),
        &i18n("Main color of subtitle text"),
        "sub-color",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_color(v);
            }
        },
    );
    text_color_row.add_prefix(&illustration("colors.svg"));

    let m = mpv.cloned();
    let outline_size_row = add_spin_double_row(
        &group,
        &i18n("Outline Thickness"),
        &i18n("Border around text — thicker makes it readable on bright backgrounds"),
        "sub-border-size",
        0.0,
        10.0,
        0.5,
        1,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_border_size(v);
            }
        },
    );
    outline_size_row.add_prefix(&illustration("outline.svg"));

    let m = mpv.cloned();
    let outline_color_row = add_color_row(
        &group,
        &i18n("Outline Color"),
        &i18n("Color of the border around subtitle text"),
        "sub-border-color",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_border_color(v);
            }
        },
    );
    outline_color_row.add_prefix(&illustration("outline.svg"));
    track_widget_visibility(&outline_color_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let shadow_color_row = add_color_row(
        &group,
        &i18n("Shadow Color"),
        &i18n("Shadow behind text — adjust alpha for transparency"),
        "sub-shadow-color",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_shadow_color(v);
            }
        },
    );
    shadow_color_row.add_prefix(&illustration("outline.svg"));
    track_widget_visibility(&shadow_color_row, is_advanced, adv_widgets);

    let m = mpv.cloned();
    let shadow_offset_row = add_spin_double_row(
        &group,
        &i18n("Shadow Offset"),
        &i18n("Distance of shadow from text"),
        "sub-shadow-offset",
        0.0,
        10.0,
        0.5,
        1,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_shadow_offset(v);
            }
        },
    );
    shadow_offset_row.add_prefix(&illustration("outline.svg"));
    track_widget_visibility(&shadow_offset_row, is_advanced, adv_widgets);

    group
}

pub(super) fn build_language_loading_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Language and Loading"))
        .description(i18n(
            "Language preferences and how external subtitle files are found",
        ))
        .build();

    let m = mpv.cloned();
    let lang_row = add_entry_row(
        &group,
        &i18n("Preferred Language"),
        &i18n("Auto-selects your language when available (e.g. pt, en, ja)"),
        "preferred-sub-lang",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_preferred_sub_lang(v);
            }
        },
    );
    lang_row.add_prefix(&illustration("language.svg"));

    let autoload_labels = &[
        i18n("Approximate match"),
        i18n("Exact filename match"),
        i18n("Load all found"),
        i18n("Don't auto-load"),
    ];
    let m = mpv.cloned();
    let autoload_row = add_combo_row(
        &group,
        &i18n("Auto-load Mode"),
        &i18n("How to find external subtitle files next to the video"),
        &autoload_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["fuzzy", "exact", "all", "no"],
        "sub-auto",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_auto(v);
            }
        },
    );
    autoload_row.add_prefix(&illustration("sub_loading.svg"));
    track_widget_visibility(&autoload_row, is_advanced, adv_widgets);

    group
}
