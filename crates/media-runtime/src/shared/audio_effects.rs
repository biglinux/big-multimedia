//! bigagents: app-local-adapter; binds audio effect components to mpv/settings/noise-reduction runtime.

use std::sync::{Arc, Weak};

use adw::prelude::*;
use big_audio_effects::noise_reduction::{MODELS, NoiseModel, any_model_available};
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::keys;
use big_audio_effects::equalizer::EQ_BAND_FREQUENCIES_F64;

pub use super::audio_effects_eq::{
    create_equalizer_content, create_equalizer_dialog, equalizer_preset_matching,
    equalizer_preset_names,
};

/// 10 semicolon-separated dB gains, clamped to ±12 dB; missing → 0.
pub fn parse_eq_bands(eq_str: &str) -> [f64; 10] {
    big_media_components::audio::equalizer_controls::parse_clamped_band_gains::<10>(
        eq_str, -12.0, 12.0,
    )
}

pub(crate) use big_audio_effects::equalizer::EQ_BAND_LABELS as EQ_LABELS;

/// Dropdown labels, naming the package a missing model needs.
#[must_use]
pub fn noise_model_labels() -> Vec<String> {
    MODELS
        .iter()
        .map(|model| {
            let label = match model {
                NoiseModel::DeepFilterNet3 => i18n("Light — DeepFilterNet3"),
                NoiseModel::DpdfNet2 => i18n("Higher quality — DPDFNet-2 48 kHz"),
            };
            if model.plugin_available() {
                label
            } else {
                format!(
                    "{label} ({})",
                    i18n("Unavailable: install {package}").replace("{package}", model.package())
                )
            }
        })
        .collect()
}

#[must_use]
pub fn noise_model_index(model: i32) -> u32 {
    NoiseModel::from_i32_or_default(model).index()
}

#[must_use]
pub fn has_noise_reduction_backend() -> bool {
    any_model_available()
}

pub fn rebuild_filters(
    settings: &crate::settings::Settings,
    eq_gains: &[f64; 10],
    mpv: &MpvBackend,
) {
    let nr_strength = settings.get_double(keys::NOISE_STRENGTH).max(0.0);
    let nr_model = settings.get_int(keys::NOISE_MODEL);

    let eq: Vec<(f64, f64)> = EQ_BAND_FREQUENCIES_F64
        .iter()
        .zip(eq_gains.iter())
        .map(|(&f, &g)| (f, g))
        .collect();
    mpv.apply_audio_effects(&crate::player::mpv_backend::AudioEffects {
        normalize: settings.get_bool(keys::AUDIO_NORMALIZE),
        nr_enabled: nr_strength > 0.0,
        nr_strength,
        nr_model,
        nr_mono: settings.get_bool(keys::NOISE_MONO),
        gate_enabled: false,
        gate_intensity: 0.0,
        hpf_enabled: false,
        hpf_frequency: 0.0,
        eq_bands: &eq,
        compressor_enabled: false,
        compressor_intensity: 0.0,
    });
}

pub fn apply_voice_change(settings: &crate::settings::Settings, mpv: &MpvBackend) {
    let eq_str = settings.get_string(keys::EQ_BANDS);
    let gains = parse_eq_bands(&eq_str);
    rebuild_filters(settings, &gains, mpv);
}

/// Model choice for the voice noise reduction. A change rebuilds the running
/// chain; the dialog holds the player weakly, so it never keeps it alive.
pub fn create_nr_settings_dialog(
    settings: &crate::settings::Settings,
    mpv: Option<&Arc<MpvBackend>>,
) -> adw::Window {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(16)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();

    let intro = gtk::Label::builder()
        .label(i18n(
            "A neural network separates voice from background noise while \
             the video or music plays. The strength slider sets how much \
             noise is removed.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    intro.add_css_class("dim-label");
    intro.set_margin_bottom(4);
    content.append(&intro);

    let (model_card, model_dropdown) = build_model_card(settings);
    content.append(&model_card);

    {
        let s = settings.clone();
        let mpv: Weak<MpvBackend> = mpv.map(Arc::downgrade).unwrap_or_default();
        model_dropdown.connect_selected_notify(move |dd| {
            let _ = s.put_int(keys::NOISE_MODEL, dd.selected() as i32);
            if let Some(mpv) = mpv.upgrade()
                && s.get_double(keys::NOISE_STRENGTH) > 0.0
            {
                apply_voice_change(&s, &mpv);
            }
        });
    }

    let mono_row = adw::SwitchRow::builder()
        .title(i18n("Convert to mono before filtering"))
        .subtitle(i18n(
            "Mixes all channels into one before removing noise: half the \
             processing, and every speaker plays the same cleaned voice. \
             The stereo placement of sounds is lost.",
        ))
        .active(settings.get_bool(keys::NOISE_MONO))
        .build();
    let mono_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    mono_list.add_css_class("boxed-list");
    mono_list.append(&mono_row);
    content.append(&mono_list);
    {
        let s = settings.clone();
        let mpv: Weak<MpvBackend> = mpv.map(Arc::downgrade).unwrap_or_default();
        mono_row.connect_active_notify(move |row| {
            let _ = s.put_bool(keys::NOISE_MONO, row.is_active());
            if let Some(mpv) = mpv.upgrade()
                && s.get_double(keys::NOISE_STRENGTH) > 0.0
            {
                apply_voice_change(&s, &mpv);
            }
        });
    }

    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .build();
    scroll.set_child(Some(&content));

    let dialog = adw::Window::builder()
        .title(i18n("Noise reduction settings"))
        .default_width(840)
        .default_height(580)
        .modal(false)
        .build();

    let toolbar_pane = BigToolbarPane::new(BigToolbarPaneSpec::new());
    let header = BigToolbarHeader::new(BigToolbarHeaderSpec::new());
    toolbar_pane.add_top_bar(header.root());
    toolbar_pane.set_content(&scroll);
    let toolbar_view = toolbar_pane.into_root();
    dialog.set_content(Some(&toolbar_view));

    crate::settings::saving::bind_write_drain(&dialog);
    dialog
}

fn build_model_card(settings: &crate::settings::Settings) -> (gtk::Box, gtk::DropDown) {
    let model_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    model_card.add_css_class("card");

    let model_inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let model_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let model_icon = gtk::Image::from_icon_name("audio-input-microphone-symbolic");
    model_icon.set_pixel_size(24);
    model_icon.set_valign(gtk::Align::Center);
    model_header.append(&model_icon);
    let model_title = gtk::Label::builder()
        .label(i18n("Noise Reduction Model"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    model_title.add_css_class("heading");
    model_header.append(&model_title);
    model_inner.append(&model_header);

    let model_desc = gtk::Label::builder()
        .label(i18n(
            "Selects which AI model separates your voice from background noise.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    model_desc.add_css_class("dim-label");
    model_inner.append(&model_desc);

    let options_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(4)
        .build();
    for text in [
        i18n("• DeepFilterNet3 — For speech. Uses less processing."),
        i18n("• DPDFNet-2 48 kHz — For speech. Higher quality, more processing."),
    ] {
        let label = gtk::Label::builder()
            .label(text)
            .wrap(true)
            .xalign(0.0)
            .build();
        label.add_css_class("dim-label");
        options_box.append(&label);
    }
    model_inner.append(&options_box);

    let labels = noise_model_labels();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model_dropdown = gtk::DropDown::from_strings(&label_refs);
    model_dropdown.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Noise reduction model",
    ))]);
    let stored_model = settings.get_int(keys::NOISE_MODEL);
    model_dropdown.set_selected(noise_model_index(stored_model));
    model_dropdown.set_margin_top(4);
    model_inner.append(&model_dropdown);

    model_card.append(&model_inner);

    (model_card, model_dropdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_eq_empty_string() {
        let gains = parse_eq_bands("");
        assert_eq!(gains, [0.0; 10]);
    }

    #[test]
    fn parse_eq_full_10_bands() {
        let gains = parse_eq_bands("1.0;2.0;3.0;4.0;5.0;6.0;7.0;8.0;9.0;10.0");
        assert_eq!(gains, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]);
    }

    #[test]
    fn parse_eq_fewer_bands_pads_zeros() {
        let gains = parse_eq_bands("3.5;-2.0");
        assert!((gains[0] - 3.5).abs() < f64::EPSILON);
        assert!((gains[1] - -2.0).abs() < f64::EPSILON);
        for &g in &gains[2..] {
            assert!((g).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_eq_extra_bands_ignored() {
        let gains = parse_eq_bands("1;2;3;4;5;6;7;8;9;10;11;12");
        // Only first 10 are used
        assert!((gains[9] - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_eq_invalid_entries_skipped() {
        let gains = parse_eq_bands("3.0;bad;5.0");
        // "bad" is skipped, so index 0=3.0, index 1=5.0 (shifted)
        assert!((gains[0] - 3.0).abs() < f64::EPSILON);
        assert!((gains[1] - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_eq_negative_gains() {
        let gains = parse_eq_bands("-6.0;0;6.0");
        assert!((gains[0] - -6.0).abs() < f64::EPSILON);
        assert!((gains[1]).abs() < f64::EPSILON);
        assert!((gains[2] - 6.0).abs() < f64::EPSILON);
    }

    #[test]
    fn eq_labels_count_matches_freqs() {
        assert_eq!(EQ_LABELS.len(), 10);
        assert_eq!(EQ_BAND_FREQUENCIES_F64.len(), 10);
    }

    #[test]
    fn parse_eq_semicolons_only() {
        // All empty strings after split → all skipped by filter_map
        let gains = parse_eq_bands(";;;");
        assert_eq!(gains, [0.0; 10]);
    }

    #[test]
    fn parse_eq_trailing_semicolon() {
        let gains = parse_eq_bands("5.0;");
        assert!((gains[0] - 5.0).abs() < f64::EPSILON);
        for &g in &gains[1..] {
            assert!(g.abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_eq_whitespace_in_value_skipped() {
        // Leading whitespace causes parse failure → value skipped
        let gains = parse_eq_bands(" 3.0;5.0");
        // " 3.0" fails to parse, "5.0" succeeds → index 0 = 5.0
        assert!((gains[0] - 5.0).abs() < f64::EPSILON);
        for &g in &gains[1..] {
            assert!(g.abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_eq_single_value() {
        let gains = parse_eq_bands("7.5");
        assert!((gains[0] - 7.5).abs() < f64::EPSILON);
        for &g in &gains[1..] {
            assert!(g.abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_eq_large_gain_values() {
        let gains = parse_eq_bands("100.0;-100.0");
        assert!((gains[0] - 12.0).abs() < f64::EPSILON);
        assert!((gains[1] - -12.0).abs() < f64::EPSILON);
    }
}
