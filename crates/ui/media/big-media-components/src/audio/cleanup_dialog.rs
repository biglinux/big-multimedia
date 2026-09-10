//! Audio cleanup and enhancement dialog.
//!
//! [`show_audio_cleanup_dialog`] presents a settings dialog with switchable
//! noise-reduction, gate, high-pass, normalize, compressor, and optional
//! equalizer cards. Callers wire pre-existing `gtk::Switch`, `gtk::DropDown`
//! and `gtk::Adjustment` widgets into [`BigAudioCleanupDialogControls`]; the
//! dialog's controls stay synchronized with those source widgets.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_audio_effects::equalizer::{EQ_BAND_FREQUENCIES_HZ, EQ_PRESET_BANDS, EQ_PRESET_LABELS};
use big_relm4_components::input::labeled_scale_row::{BigLabeledScaleRow, BigLabeledScaleRowSpec};
use big_relm4_components::input::switch::sync_switches;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use relm4::gtk;

use crate::audio::equalizer_controls::{
    BigAudioEqualizerControls, BigAudioEqualizerLabels, BigAudioEqualizerSpec,
};
use crate::converter::dialog_cards::{make_card, make_card_with_extra};

const DIALOG_CONTENT_WIDTH: i32 = 560;
const DIALOG_CONTENT_HEIGHT_720P: i32 = 620;
const DIALOG_SCROLLED_CONTENT_HEIGHT: i32 = 420;
const FIRST_SECTION_MARGIN_TOP: i32 = 0;
const FOLLOWING_SECTION_MARGIN_TOP: i32 = 8;
const SECTION_MARGIN_BOTTOM: i32 = 8;
const MODEL_HINT_SPACING: i32 = 2;
const MODEL_HINT_MARGIN_TOP: i32 = 4;
const CLEANUP_SCALE_TITLE_WIDTH_CHARS: i32 = 16;
const NOISE_REDUCTION_ILLUSTRATION: &str = "noise_reduction.svg";
const NOISE_GATE_ILLUSTRATION: &str = "noise_gate.svg";
const HIGH_PASS_ILLUSTRATION: &str = "hpf_rumble.svg";
const UNIFORM_VOLUME_ILLUSTRATION: &str = "uniform_volume.svg";
const COMPRESSOR_ILLUSTRATION: &str = "compressor.svg";
const EQUALIZER_ILLUSTRATION: &str = "equalizer.svg";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CleanupDialogPlan {
    content_width: i32,
    content_height: i32,
    scrolled_content_height: i32,
    show_missing_backend_banner: bool,
    include_equalizer: bool,
}

/// Translated strings displayed by the audio cleanup dialog.
///
/// Each field stores one already-translated UI message. Build instances
/// with [`BigAudioCleanupDialogText::translated`] to source the strings
/// through any callback (gettext, fluent, identity for tests).
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioCleanupDialogText {
    pub title: String,
    pub description: String,
    pub noise_section: String,
    pub missing_backend: String,
    pub remove_noise: String,
    pub remove_noise_description: String,
    pub strength: String,
    pub ai_model: String,
    pub dns3_hint: String,
    pub neural_hint: String,
    pub voice_recovery: String,
    pub gate_section: String,
    pub silence_low_sounds: String,
    pub silence_low_sounds_description: String,
    pub intensity: String,
    pub enhancement_section: String,
    pub remove_low_rumble: String,
    pub remove_low_rumble_description: String,
    pub frequency_hz: String,
    pub uniform_volume: String,
    pub uniform_volume_description: String,
    pub compressor: String,
    pub compressor_description: String,
    pub equalizer: String,
    pub equalizer_description: String,
    pub enable_equalizer: String,
    pub preset: String,
}

impl BigAudioCleanupDialogText {
    /// Translate every UI message through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("Audio Settings"),
            description: text(
                "Improve your audio by removing noise, controlling volume and enhancing voice clarity.",
            ),
            noise_section: text("AI Noise Cleaning"),
            missing_backend: text("Noise reduction LADSPA plugin not installed"),
            remove_noise: text("Remove Background Noise"),
            remove_noise_description: text(
                "Uses a neural network to separate voice from noise. Strength controls how aggressively noise is removed.",
            ),
            strength: text("Strength"),
            ai_model: text("AI Model"),
            dns3_hint: text(
                "DNS3 - General-purpose. Good at removing steady noise like fans, AC, and hum.",
            ),
            neural_hint: text(
                "DeepFilterNet3 / DPDFNet HR - Full-band 48 kHz models. Cleaner output when the optional LADSPA plugins are installed.",
            ),
            voice_recovery: text("Voice Recovery"),
            gate_section: text("Noise Gate"),
            silence_low_sounds: text("Silence Low Sounds"),
            silence_low_sounds_description: text(
                "Mutes audio below a threshold - great for removing faint hum or room noise between speech.",
            ),
            intensity: text("Intensity"),
            enhancement_section: text("Audio Enhancement"),
            remove_low_rumble: text("Remove Low Rumble"),
            remove_low_rumble_description: text(
                "Cuts low-frequency noise like wind, air conditioning or handling vibrations.",
            ),
            frequency_hz: text("Frequency (Hz)"),
            uniform_volume: text("Uniform Volume"),
            uniform_volume_description: text(
                "Normalizes loudness to broadcast standard (ITU-R BS.1770, -16 LUFS). Makes all files similar volume.",
            ),
            compressor: text("Compressor"),
            compressor_description: text(
                "Reduces the dynamic range - makes quiet parts louder and loud parts quieter.",
            ),
            equalizer: text("Equalizer"),
            equalizer_description: text(
                "Adjust frequency balance. Use presets or drag individual bands.",
            ),
            enable_equalizer: text("Enable equalizer"),
            preset: text("Preset"),
        }
    }
}

/// References to the source widgets the cleanup dialog mirrors.
///
/// Every field is a borrowed reference to a widget owned by the caller; the
/// dialog binds two-way listeners so that toggling a switch or moving an
/// adjustment in the dialog updates the source widget and vice-versa.
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub struct BigAudioCleanupDialogControls<'a> {
    pub noise_backend_available: bool,
    pub noise_enabled_switch: &'a gtk::Switch,
    pub noise_strength_adjustment: &'a gtk::Adjustment,
    pub noise_model_dropdown: &'a gtk::DropDown,
    pub noise_model_labels: Vec<String>,
    pub noise_recovery_adjustment: &'a gtk::Adjustment,
    pub gate_enabled_switch: &'a gtk::Switch,
    pub gate_intensity_adjustment: &'a gtk::Adjustment,
    pub high_pass_enabled_switch: &'a gtk::Switch,
    pub high_pass_frequency_adjustment: &'a gtk::Adjustment,
    pub normalize_switch: &'a gtk::Switch,
    pub compressor_enabled_switch: &'a gtk::Switch,
    pub compressor_intensity_adjustment: &'a gtk::Adjustment,
    pub equalizer_enabled_switch: &'a gtk::Switch,
    pub equalizer_preset_dropdown: &'a gtk::DropDown,
    pub equalizer_band_adjustments: &'a [gtk::Adjustment],
}

/// Present the audio cleanup dialog as a modal child of `parent`.
///
/// `include_equalizer` chooses whether to render the equalizer card at the
/// bottom of the dialog. Set it to `false` in apps that expose the
/// equalizer separately.
pub fn show_audio_cleanup_dialog(
    parent: &impl IsA<gtk::Widget>,
    text: BigAudioCleanupDialogText,
    controls: BigAudioCleanupDialogControls<'_>,
    include_equalizer: bool,
) {
    let plan = cleanup_dialog_plan(controls.noise_backend_available, include_equalizer);
    let dialog = adw::Dialog::builder()
        .title(&text.title)
        .content_width(plan.content_width)
        .content_height(plan.content_height)
        .build();

    let page = BigDialogPage::scrolled_clamped(plan.content_width, plan.scrolled_content_height);
    let content = page.content().clone();
    page.append_description(&text.description);

    append_section_label(&content, &text.noise_section, FIRST_SECTION_MARGIN_TOP);
    if plan.show_missing_backend_banner {
        let banner = adw::Banner::builder()
            .title(&text.missing_backend)
            .revealed(true)
            .build();
        banner.set_margin_bottom(8);
        content.append(&banner);
    }
    append_noise_card(&content, &text, &controls);

    append_section_label(&content, &text.gate_section, following_section_margin_top());
    append_gate_card(&content, &text, &controls);

    append_section_label(
        &content,
        &text.enhancement_section,
        following_section_margin_top(),
    );
    append_enhancement_cards(&content, &text, &controls);

    if plan.include_equalizer {
        append_equalizer_card(&content, &text, &controls);
    }

    let toolbar = page.into_root();
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}

fn append_noise_card(
    content: &gtk::Box,
    text: &BigAudioCleanupDialogText,
    controls: &BigAudioCleanupDialogControls<'_>,
) {
    let noise_switch = gtk::Switch::builder()
        .active(controls.noise_enabled_switch.is_active())
        .sensitive(controls.noise_backend_available)
        .valign(gtk::Align::Center)
        .build();
    noise_switch.update_property(&[gtk::accessible::Property::Label(&text.remove_noise)]);
    sync_switches(&noise_switch, controls.noise_enabled_switch);

    let noise_sliders = gtk::Box::new(gtk::Orientation::Vertical, 8);
    noise_sliders.append(&make_synced_scale(
        &text.strength,
        controls.noise_strength_adjustment,
        "%",
        100.0,
    ));

    let model_label_refs: Vec<&str> = controls
        .noise_model_labels
        .iter()
        .map(String::as_str)
        .collect();
    let model_dropdown = gtk::DropDown::from_strings(&model_label_refs);
    model_dropdown.set_selected(controls.noise_model_dropdown.selected());
    sync_dropdowns(&model_dropdown, controls.noise_model_dropdown);

    let model_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    model_row.append(
        &gtk::Label::builder()
            .label(&text.ai_model)
            .hexpand(true)
            .halign(gtk::Align::Start)
            .build(),
    );
    model_row.append(&model_dropdown);
    noise_sliders.append(&model_row);

    let model_hints = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(model_hint_spacing())
        .margin_top(model_hint_margin_top())
        .build();
    append_hint(&model_hints, &text.dns3_hint);
    append_hint(&model_hints, &text.neural_hint);
    noise_sliders.append(&model_hints);

    noise_sliders.append(&make_synced_scale(
        &text.voice_recovery,
        controls.noise_recovery_adjustment,
        "%",
        100.0,
    ));

    content.append(&make_card_with_extra(
        noise_reduction_illustration(),
        &text.remove_noise,
        &text.remove_noise_description,
        &noise_switch,
        &noise_sliders,
    ));
}

fn append_gate_card(
    content: &gtk::Box,
    text: &BigAudioCleanupDialogText,
    controls: &BigAudioCleanupDialogControls<'_>,
) {
    let gate_switch = gtk::Switch::builder()
        .active(controls.gate_enabled_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    gate_switch.update_property(&[gtk::accessible::Property::Label(&text.silence_low_sounds)]);
    sync_switches(&gate_switch, controls.gate_enabled_switch);

    let gate_slider = make_synced_scale(
        &text.intensity,
        controls.gate_intensity_adjustment,
        "%",
        1.0,
    );
    content.append(&make_card_with_extra(
        noise_gate_illustration(),
        &text.silence_low_sounds,
        &text.silence_low_sounds_description,
        &gate_switch,
        &gate_slider,
    ));
}

fn append_enhancement_cards(
    content: &gtk::Box,
    text: &BigAudioCleanupDialogText,
    controls: &BigAudioCleanupDialogControls<'_>,
) {
    let high_pass_switch = gtk::Switch::builder()
        .active(controls.high_pass_enabled_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    high_pass_switch.update_property(&[gtk::accessible::Property::Label(&text.remove_low_rumble)]);
    sync_switches(&high_pass_switch, controls.high_pass_enabled_switch);
    let high_pass_slider = make_synced_scale(
        &text.frequency_hz,
        controls.high_pass_frequency_adjustment,
        " Hz",
        1.0,
    );
    content.append(&make_card_with_extra(
        high_pass_illustration(),
        &text.remove_low_rumble,
        &text.remove_low_rumble_description,
        &high_pass_switch,
        &high_pass_slider,
    ));

    let normalize_switch = gtk::Switch::builder()
        .active(controls.normalize_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    normalize_switch.update_property(&[gtk::accessible::Property::Label(&text.uniform_volume)]);
    sync_switches(&normalize_switch, controls.normalize_switch);
    content.append(&make_card(
        uniform_volume_illustration(),
        &text.uniform_volume,
        &text.uniform_volume_description,
        &normalize_switch,
    ));

    let compressor_switch = gtk::Switch::builder()
        .active(controls.compressor_enabled_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    compressor_switch.update_property(&[gtk::accessible::Property::Label(&text.compressor)]);
    sync_switches(&compressor_switch, controls.compressor_enabled_switch);
    let compressor_slider = make_synced_scale(
        &text.intensity,
        controls.compressor_intensity_adjustment,
        "%",
        100.0,
    );
    content.append(&make_card_with_extra(
        compressor_illustration(),
        &text.compressor,
        &text.compressor_description,
        &compressor_switch,
        &compressor_slider,
    ));
}

fn append_equalizer_card(
    content: &gtk::Box,
    text: &BigAudioCleanupDialogText,
    controls: &BigAudioCleanupDialogControls<'_>,
) {
    let equalizer_switch = gtk::Switch::builder()
        .active(controls.equalizer_enabled_switch.is_active())
        .valign(gtk::Align::Center)
        .build();
    equalizer_switch.update_property(&[gtk::accessible::Property::Label(&text.enable_equalizer)]);
    sync_switches(&equalizer_switch, controls.equalizer_enabled_switch);

    let equalizer_box = BigAudioEqualizerControls::new(
        BigAudioEqualizerSpec::new(
            BigAudioEqualizerLabels {
                preset: text.preset.clone(),
            },
            EQ_BAND_FREQUENCIES_HZ.iter().copied(),
            EQ_PRESET_LABELS,
            EQ_PRESET_BANDS,
        ),
        controls.equalizer_preset_dropdown,
        controls.equalizer_band_adjustments,
    )
    .into_root();

    content.append(&make_card_with_extra(
        equalizer_illustration(),
        &text.equalizer,
        &text.equalizer_description,
        &equalizer_switch,
        &equalizer_box,
    ));
}

fn append_section_label(content: &gtk::Box, text: &str, margin_top: i32) {
    let label = gtk::Label::builder()
        .label(text)
        .halign(gtk::Align::Start)
        .margin_top(margin_top)
        .margin_bottom(section_margin_bottom())
        .build();
    label.add_css_class("title-3");
    content.append(&label);
}

fn append_hint(content: &gtk::Box, text: &str) {
    let label = gtk::Label::builder()
        .label(text)
        .wrap(true)
        .xalign(0.0)
        .build();
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    content.append(&label);
}

fn sync_dropdowns(dialog_dropdown: &gtk::DropDown, source_dropdown: &gtk::DropDown) {
    // Delegate to the shared weak-bind helper — a local two-way strong bind is a
    // mutual reference cycle that leaks both dropdowns when the dialog closes.
    big_relm4_components::input::dropdown::sync_dropdowns(dialog_dropdown, source_dropdown);
}

fn make_synced_scale(
    title: &str,
    adjustment: &gtk::Adjustment,
    suffix: &str,
    display_multiplier: f64,
) -> gtk::Box {
    let labeled = BigLabeledScaleRow::new(
        cleanup_labeled_scale_spec(title, suffix, display_multiplier),
        adjustment,
    );
    let (_, row, _) = labeled.into_parts();
    row
}

fn cleanup_dialog_plan(
    noise_backend_available: bool,
    include_equalizer: bool,
) -> CleanupDialogPlan {
    CleanupDialogPlan {
        content_width: DIALOG_CONTENT_WIDTH,
        content_height: DIALOG_CONTENT_HEIGHT_720P,
        scrolled_content_height: DIALOG_SCROLLED_CONTENT_HEIGHT,
        show_missing_backend_banner: should_show_missing_backend_banner(noise_backend_available),
        include_equalizer,
    }
}

fn should_show_missing_backend_banner(noise_backend_available: bool) -> bool {
    !noise_backend_available
}

fn following_section_margin_top() -> i32 {
    FOLLOWING_SECTION_MARGIN_TOP
}

fn section_margin_bottom() -> i32 {
    SECTION_MARGIN_BOTTOM
}

fn model_hint_spacing() -> i32 {
    MODEL_HINT_SPACING
}

fn model_hint_margin_top() -> i32 {
    MODEL_HINT_MARGIN_TOP
}

fn cleanup_labeled_scale_spec(
    title: &str,
    suffix: &str,
    display_multiplier: f64,
) -> BigLabeledScaleRowSpec {
    BigLabeledScaleRowSpec::new(title)
        .title_width_chars(CLEANUP_SCALE_TITLE_WIDTH_CHARS)
        .display(display_multiplier, suffix, 0)
}

fn noise_reduction_illustration() -> &'static str {
    NOISE_REDUCTION_ILLUSTRATION
}

fn noise_gate_illustration() -> &'static str {
    NOISE_GATE_ILLUSTRATION
}

fn high_pass_illustration() -> &'static str {
    HIGH_PASS_ILLUSTRATION
}

fn uniform_volume_illustration() -> &'static str {
    UNIFORM_VOLUME_ILLUSTRATION
}

fn compressor_illustration() -> &'static str {
    COMPRESSOR_ILLUSTRATION
}

fn equalizer_illustration() -> &'static str {
    EQUALIZER_ILLUSTRATION
}

#[cfg(test)]
mod tests {
    use super::{
        BigAudioCleanupDialogText, CLEANUP_SCALE_TITLE_WIDTH_CHARS, DIALOG_CONTENT_HEIGHT_720P,
        DIALOG_CONTENT_WIDTH, DIALOG_SCROLLED_CONTENT_HEIGHT, FIRST_SECTION_MARGIN_TOP,
        cleanup_dialog_plan, cleanup_labeled_scale_spec, compressor_illustration,
        equalizer_illustration, following_section_margin_top, high_pass_illustration,
        model_hint_margin_top, model_hint_spacing, noise_gate_illustration,
        noise_reduction_illustration, section_margin_bottom, should_show_missing_backend_banner,
        uniform_volume_illustration,
    };

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let mut translated_messages = Vec::new();
        let text = BigAudioCleanupDialogText::translated(|message| {
            translated_messages.push(message.to_string());
            format!("translated:{message}")
        });

        assert_eq!(text.title, "translated:Audio Settings");
        assert_eq!(text.noise_section, "translated:AI Noise Cleaning");
        assert_eq!(
            text.missing_backend,
            "translated:Noise reduction LADSPA plugin not installed"
        );
        assert_eq!(text.equalizer, "translated:Equalizer");
        assert_eq!(translated_messages.len(), 27);
        assert!(translated_messages.contains(&"Voice Recovery".to_string()));
        assert!(translated_messages.contains(&"Preset".to_string()));
    }

    #[test]
    fn cleanup_dialog_plan_exposes_banner_and_equalizer_decisions() {
        let missing_backend_plan = cleanup_dialog_plan(false, true);
        assert_eq!(missing_backend_plan.content_width, DIALOG_CONTENT_WIDTH);
        assert_eq!(
            missing_backend_plan.content_height,
            DIALOG_CONTENT_HEIGHT_720P
        );
        assert_eq!(
            missing_backend_plan.scrolled_content_height,
            DIALOG_SCROLLED_CONTENT_HEIGHT
        );
        assert!(missing_backend_plan.show_missing_backend_banner);
        assert!(missing_backend_plan.include_equalizer);

        let installed_backend_plan = cleanup_dialog_plan(true, false);
        assert!(!installed_backend_plan.show_missing_backend_banner);
        assert!(!installed_backend_plan.include_equalizer);
        assert!(should_show_missing_backend_banner(false));
        assert!(!should_show_missing_backend_banner(true));
    }

    #[test]
    fn cleanup_dialog_layout_constants_match_section_spacing() {
        assert_eq!(FIRST_SECTION_MARGIN_TOP, 0);
        assert_eq!(following_section_margin_top(), 8);
        assert_eq!(section_margin_bottom(), 8);
        assert_eq!(model_hint_spacing(), 2);
        assert_eq!(model_hint_margin_top(), 4);
    }

    #[test]
    fn cleanup_labeled_scale_spec_matches_slider_display_contract() {
        let percent_spec = cleanup_labeled_scale_spec("Strength", "%", 100.0);
        assert_eq!(percent_spec.title, "Strength");
        assert_eq!(
            percent_spec.title_width_chars,
            CLEANUP_SCALE_TITLE_WIDTH_CHARS
        );
        assert_eq!(percent_spec.display_multiplier, 100.0);
        assert_eq!(percent_spec.suffix, "%");
        assert_eq!(percent_spec.digits, 0);
        assert_eq!(percent_spec.value_text(0.42), "42%");

        let frequency_spec = cleanup_labeled_scale_spec("Frequency (Hz)", " Hz", 1.0);
        assert_eq!(frequency_spec.value_text(120.0), "120 Hz");
    }

    #[test]
    fn cleanup_card_illustrations_cover_every_card() {
        assert_eq!(noise_reduction_illustration(), "noise_reduction.svg");
        assert_eq!(noise_gate_illustration(), "noise_gate.svg");
        assert_eq!(high_pass_illustration(), "hpf_rumble.svg");
        assert_eq!(uniform_volume_illustration(), "uniform_volume.svg");
        assert_eq!(compressor_illustration(), "compressor.svg");
        assert_eq!(equalizer_illustration(), "equalizer.svg");
    }
}
