//! Equalizer settings dialog.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_audio_effects::equalizer::{EQ_BAND_FREQUENCIES_HZ, EQ_PRESET_BANDS, EQ_PRESET_LABELS};
use big_relm4_components::input::switch::sync_switches;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::audio::equalizer_controls::{
    BigAudioEqualizerControls, BigAudioEqualizerLabels, BigAudioEqualizerSpec,
};
use crate::converter::dialog_cards::load_svg;

const DIALOG_CONTENT_WIDTH: i32 = 620;
const DIALOG_CONTENT_HEIGHT: i32 = 700;
const DIALOG_SCROLLED_CONTENT_WIDTH: i32 = 580;
const DIALOG_SCROLLED_CONTENT_HEIGHT: i32 = 420;
const SEPARATOR_MARGIN_BOTTOM: i32 = 6;
const HEADING_ROW_SPACING: i32 = 18;
const HEADING_MARGIN_BOTTOM: i32 = 8;
const HEADING_ILLUSTRATION_WIDTH: i32 = 102;
const HEADING_ILLUSTRATION_HEIGHT: i32 = 72;
const HEADING_TEXT_SPACING: i32 = 6;
const HEADING_DESCRIPTION_MAX_WIDTH_CHARS: i32 = 50;
const EQUALIZER_SECTION_SPACING: i32 = 12;
const EQUALIZER_SECTION_MARGIN_TOP: i32 = 2;
const EQUALIZER_SECTION_MARGIN_BOTTOM: i32 = 4;
const EQUALIZER_SECTION_MARGIN_START: i32 = 2;
const EQUALIZER_SECTION_MARGIN_END: i32 = 2;
const EQUALIZER_PRESET_ROW_SPACING: i32 = 16;
const EQUALIZER_PRESET_ROW_MARGIN_BOTTOM: i32 = 6;
const EQUALIZER_BAND_TITLE_WIDTH_CHARS: i32 = 9;
const EQUALIZER_BAND_VALUE_WIDTH_CHARS: i32 = 9;
const EQUALIZER_BAND_SPACING: i32 = 12;
const EQUALIZER_BAND_HEIGHT_REQUEST: i32 = 26;
const EQUALIZER_BAND_VERTICAL_MARGIN: i32 = 1;
const EQUALIZER_ILLUSTRATION: &str = "equalizer.svg";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EqualizerDialogPlan {
    content_width: i32,
    content_height: i32,
    scrolled_content_width: i32,
    scrolled_content_height: i32,
    separator_margin_bottom: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EqualizerDialogHeadingSpec {
    illustration_name: &'static str,
    row_spacing: i32,
    margin_bottom: i32,
    illustration_width: i32,
    illustration_height: i32,
    text_spacing: i32,
    description: String,
    description_max_width_chars: i32,
    enable_label: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct EqualizerDialogSectionLayout {
    spacing: i32,
    margin_top: i32,
    margin_bottom: i32,
    margin_start: i32,
    margin_end: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EqualizerControlsDialogSpec {
    preset_label: String,
    preset_row_spacing: i32,
    preset_row_margin_bottom: i32,
    band_title_width_chars: i32,
    band_value_width_chars: i32,
    band_spacing: i32,
    band_height_request: i32,
    band_vertical_margin: i32,
}

/// Translated strings for the standalone equalizer dialog.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioEqualizerDialogText {
    pub title: String,
    pub description: String,
    pub enable_label: String,
    pub preset_label: String,
}

impl BigAudioEqualizerDialogText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Equalizer"),
            description: text("Adjust frequency balance. Use presets or drag individual bands."),
            enable_label: text("Enable equalizer"),
            preset_label: text("Preset"),
        }
    }
}

/// References to the source widgets the equalizer dialog mirrors.
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct BigAudioEqualizerDialogControls<'a> {
    pub enabled_switch: &'a gtk::Switch,
    pub preset_dropdown: &'a gtk::DropDown,
    pub band_adjustments: &'a [gtk::Adjustment],
}

/// Present the equalizer dialog as a modal child of `parent`.
pub fn show_audio_equalizer_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigAudioEqualizerDialogText,
    controls: BigAudioEqualizerDialogControls<'_>,
) {
    let dialog_plan = audio_equalizer_dialog_plan();
    let heading_spec = audio_equalizer_heading_spec(&text);
    let section_layout = audio_equalizer_section_layout();

    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(dialog_plan.content_width)
        .content_height(dialog_plan.content_height)
        .build();

    let page = BigDialogPage::scrolled_clamped(
        dialog_plan.scrolled_content_width,
        dialog_plan.scrolled_content_height,
    );
    let content = page.content().clone();

    let heading_row = gtk::Box::new(gtk::Orientation::Horizontal, heading_spec.row_spacing);
    heading_row.set_margin_bottom(heading_spec.margin_bottom);

    let heading_icon = load_svg(heading_spec.illustration_name);
    heading_icon.set_size_request(
        heading_spec.illustration_width,
        heading_spec.illustration_height,
    );
    heading_icon.set_valign(gtk::Align::Center);
    heading_row.append(&heading_icon);

    let heading_text = gtk::Box::new(gtk::Orientation::Vertical, heading_spec.text_spacing);
    heading_text.set_hexpand(true);
    heading_text.set_valign(gtk::Align::Center);
    let heading_description = gtk::Label::builder()
        .label(&heading_spec.description)
        .halign(gtk::Align::Start)
        .wrap(true)
        .max_width_chars(heading_spec.description_max_width_chars)
        .build();
    heading_description.add_css_class("dim-label");
    heading_text.append(&heading_description);
    heading_row.append(&heading_text);

    let equalizer_switch = gtk::Switch::builder()
        .active(controls.enabled_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    equalizer_switch
        .update_property(&[gtk::accessible::Property::Label(&heading_spec.enable_label)]);
    sync_switches(&equalizer_switch, controls.enabled_switch);
    heading_row.append(&equalizer_switch);
    content.append(&heading_row);

    let separator = gtk::Separator::new(gtk::Orientation::Horizontal);
    separator.add_css_class("spacer");
    separator.set_margin_bottom(dialog_plan.separator_margin_bottom);
    content.append(&separator);

    let equalizer_section = gtk::Box::new(gtk::Orientation::Vertical, section_layout.spacing);
    equalizer_section.set_margin_top(section_layout.margin_top);
    equalizer_section.set_margin_bottom(section_layout.margin_bottom);
    equalizer_section.set_margin_start(section_layout.margin_start);
    equalizer_section.set_margin_end(section_layout.margin_end);

    let equalizer_control_spec = audio_equalizer_controls_spec(&text);
    let equalizer_box = BigAudioEqualizerControls::new(
        equalizer_control_spec.into_audio_equalizer_spec(),
        controls.preset_dropdown,
        controls.band_adjustments,
    )
    .into_root();

    equalizer_section.append(&equalizer_box);
    content.append(&equalizer_section);

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

fn audio_equalizer_dialog_plan() -> EqualizerDialogPlan {
    EqualizerDialogPlan {
        content_width: DIALOG_CONTENT_WIDTH,
        content_height: DIALOG_CONTENT_HEIGHT,
        scrolled_content_width: DIALOG_SCROLLED_CONTENT_WIDTH,
        scrolled_content_height: DIALOG_SCROLLED_CONTENT_HEIGHT,
        separator_margin_bottom: SEPARATOR_MARGIN_BOTTOM,
    }
}

fn audio_equalizer_heading_spec(text: &BigAudioEqualizerDialogText) -> EqualizerDialogHeadingSpec {
    EqualizerDialogHeadingSpec {
        illustration_name: EQUALIZER_ILLUSTRATION,
        row_spacing: HEADING_ROW_SPACING,
        margin_bottom: HEADING_MARGIN_BOTTOM,
        illustration_width: HEADING_ILLUSTRATION_WIDTH,
        illustration_height: HEADING_ILLUSTRATION_HEIGHT,
        text_spacing: HEADING_TEXT_SPACING,
        description: text.description.clone(),
        description_max_width_chars: HEADING_DESCRIPTION_MAX_WIDTH_CHARS,
        enable_label: text.enable_label.clone(),
    }
}

fn audio_equalizer_section_layout() -> EqualizerDialogSectionLayout {
    EqualizerDialogSectionLayout {
        spacing: EQUALIZER_SECTION_SPACING,
        margin_top: EQUALIZER_SECTION_MARGIN_TOP,
        margin_bottom: EQUALIZER_SECTION_MARGIN_BOTTOM,
        margin_start: EQUALIZER_SECTION_MARGIN_START,
        margin_end: EQUALIZER_SECTION_MARGIN_END,
    }
}

fn audio_equalizer_controls_spec(
    text: &BigAudioEqualizerDialogText,
) -> EqualizerControlsDialogSpec {
    EqualizerControlsDialogSpec {
        preset_label: text.preset_label.clone(),
        preset_row_spacing: EQUALIZER_PRESET_ROW_SPACING,
        preset_row_margin_bottom: EQUALIZER_PRESET_ROW_MARGIN_BOTTOM,
        band_title_width_chars: EQUALIZER_BAND_TITLE_WIDTH_CHARS,
        band_value_width_chars: EQUALIZER_BAND_VALUE_WIDTH_CHARS,
        band_spacing: EQUALIZER_BAND_SPACING,
        band_height_request: EQUALIZER_BAND_HEIGHT_REQUEST,
        band_vertical_margin: EQUALIZER_BAND_VERTICAL_MARGIN,
    }
}

impl EqualizerControlsDialogSpec {
    fn into_audio_equalizer_spec(self) -> BigAudioEqualizerSpec {
        BigAudioEqualizerSpec::new(
            BigAudioEqualizerLabels {
                preset: self.preset_label,
            },
            EQ_BAND_FREQUENCIES_HZ.iter().copied(),
            EQ_PRESET_LABELS,
            EQ_PRESET_BANDS,
        )
        .preset_layout(self.preset_row_spacing, self.preset_row_margin_bottom)
        .band_layout(
            self.band_title_width_chars,
            self.band_value_width_chars,
            self.band_spacing,
            self.band_height_request,
            self.band_vertical_margin,
        )
    }
}

#[cfg(test)]
mod tests {
    use big_audio_effects::equalizer::{EQ_BAND_FREQUENCIES_HZ, EQ_PRESET_LABELS};

    use super::{
        BigAudioEqualizerDialogText, DIALOG_CONTENT_HEIGHT, DIALOG_CONTENT_WIDTH,
        DIALOG_SCROLLED_CONTENT_HEIGHT, DIALOG_SCROLLED_CONTENT_WIDTH,
        EQUALIZER_BAND_HEIGHT_REQUEST, EQUALIZER_BAND_SPACING, EQUALIZER_BAND_TITLE_WIDTH_CHARS,
        EQUALIZER_BAND_VALUE_WIDTH_CHARS, EQUALIZER_BAND_VERTICAL_MARGIN, EQUALIZER_ILLUSTRATION,
        EQUALIZER_PRESET_ROW_MARGIN_BOTTOM, EQUALIZER_PRESET_ROW_SPACING,
        EQUALIZER_SECTION_MARGIN_BOTTOM, EQUALIZER_SECTION_MARGIN_END,
        EQUALIZER_SECTION_MARGIN_START, EQUALIZER_SECTION_MARGIN_TOP, EQUALIZER_SECTION_SPACING,
        HEADING_DESCRIPTION_MAX_WIDTH_CHARS, HEADING_ILLUSTRATION_HEIGHT,
        HEADING_ILLUSTRATION_WIDTH, HEADING_MARGIN_BOTTOM, HEADING_ROW_SPACING,
        HEADING_TEXT_SPACING, SEPARATOR_MARGIN_BOTTOM, audio_equalizer_controls_spec,
        audio_equalizer_dialog_plan, audio_equalizer_heading_spec, audio_equalizer_section_layout,
    };

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let mut translated_messages = Vec::new();
        let text = BigAudioEqualizerDialogText::translated(|message| {
            translated_messages.push(message.to_string());
            format!("translated:{message}")
        });

        assert_eq!(text.title, "translated:Equalizer");
        assert_eq!(
            text.description,
            "translated:Adjust frequency balance. Use presets or drag individual bands."
        );
        assert_eq!(text.enable_label, "translated:Enable equalizer");
        assert_eq!(text.preset_label, "translated:Preset");
        assert_eq!(translated_messages.len(), 4);
        assert!(translated_messages.contains(&"Equalizer".to_string()));
        assert!(translated_messages.contains(&"Enable equalizer".to_string()));
    }

    #[test]
    fn dialog_plan_matches_equalizer_surface_size() {
        let plan = audio_equalizer_dialog_plan();

        assert_eq!(plan.content_width, DIALOG_CONTENT_WIDTH);
        assert_eq!(plan.content_height, DIALOG_CONTENT_HEIGHT);
        assert_eq!(plan.scrolled_content_width, DIALOG_SCROLLED_CONTENT_WIDTH);
        assert_eq!(plan.scrolled_content_height, DIALOG_SCROLLED_CONTENT_HEIGHT);
        assert_eq!(plan.separator_margin_bottom, SEPARATOR_MARGIN_BOTTOM);
        assert_eq!(DIALOG_CONTENT_WIDTH, 620);
        assert_eq!(DIALOG_CONTENT_HEIGHT, 700);
        assert_eq!(DIALOG_SCROLLED_CONTENT_WIDTH, 580);
        assert_eq!(DIALOG_SCROLLED_CONTENT_HEIGHT, 420);
    }

    #[test]
    fn heading_spec_keeps_description_icon_and_accessible_switch_label() {
        let text =
            BigAudioEqualizerDialogText::translated(|message| format!("translated:{message}"));
        let heading = audio_equalizer_heading_spec(&text);

        assert_eq!(heading.illustration_name, EQUALIZER_ILLUSTRATION);
        assert_eq!(heading.illustration_width, HEADING_ILLUSTRATION_WIDTH);
        assert_eq!(heading.illustration_height, HEADING_ILLUSTRATION_HEIGHT);
        assert_eq!(heading.row_spacing, HEADING_ROW_SPACING);
        assert_eq!(heading.margin_bottom, HEADING_MARGIN_BOTTOM);
        assert_eq!(heading.text_spacing, HEADING_TEXT_SPACING);
        assert_eq!(
            heading.description,
            "translated:Adjust frequency balance. Use presets or drag individual bands."
        );
        assert_eq!(
            heading.description_max_width_chars,
            HEADING_DESCRIPTION_MAX_WIDTH_CHARS
        );
        assert_eq!(heading.enable_label, "translated:Enable equalizer");
    }

    #[test]
    fn section_layout_keeps_compact_equalizer_margins() {
        let layout = audio_equalizer_section_layout();

        assert_eq!(layout.spacing, EQUALIZER_SECTION_SPACING);
        assert_eq!(layout.margin_top, EQUALIZER_SECTION_MARGIN_TOP);
        assert_eq!(layout.margin_bottom, EQUALIZER_SECTION_MARGIN_BOTTOM);
        assert_eq!(layout.margin_start, EQUALIZER_SECTION_MARGIN_START);
        assert_eq!(layout.margin_end, EQUALIZER_SECTION_MARGIN_END);
    }

    #[test]
    fn equalizer_controls_spec_uses_canonical_tables_and_dialog_layout() {
        let text = BigAudioEqualizerDialogText::translated(|message| message.to_string());
        let spec = audio_equalizer_controls_spec(&text);
        let equalizer_spec = spec.clone().into_audio_equalizer_spec();
        let resolved = equalizer_spec.resolved();

        assert_eq!(spec.preset_label, "Preset");
        assert_eq!(resolved.band_count, EQ_BAND_FREQUENCIES_HZ.len());
        assert_eq!(resolved.preset_count, EQ_PRESET_LABELS.len());
        assert!(resolved.preset_rows_match_bands);
        assert_eq!(spec.preset_row_spacing, EQUALIZER_PRESET_ROW_SPACING);
        assert_eq!(
            spec.preset_row_margin_bottom,
            EQUALIZER_PRESET_ROW_MARGIN_BOTTOM
        );
        assert_eq!(
            spec.band_title_width_chars,
            EQUALIZER_BAND_TITLE_WIDTH_CHARS
        );
        assert_eq!(
            spec.band_value_width_chars,
            EQUALIZER_BAND_VALUE_WIDTH_CHARS
        );
        assert_eq!(spec.band_spacing, EQUALIZER_BAND_SPACING);
        assert_eq!(spec.band_height_request, EQUALIZER_BAND_HEIGHT_REQUEST);
        assert_eq!(spec.band_vertical_margin, EQUALIZER_BAND_VERTICAL_MARGIN);
        assert_eq!(equalizer_spec.labels.preset, "Preset");
        assert_eq!(
            equalizer_spec.band_title_width_chars,
            Some(EQUALIZER_BAND_TITLE_WIDTH_CHARS)
        );
        assert_eq!(
            equalizer_spec.band_height_request,
            Some(EQUALIZER_BAND_HEIGHT_REQUEST)
        );
    }
}
