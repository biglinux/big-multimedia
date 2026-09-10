//! bigagents: app-local-adapter; binds audio effect components to mpv/settings/GTCRN runtime.

use adw::prelude::*;
use big_audio_effects::noise_reduction::{
    NoiseModel, REALTIME_LAVFI_MODEL_LABELS, REALTIME_LAVFI_MODELS,
    any_realtime_lavfi_model_available,
};
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};
use glib;
use log::{debug, warn};

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::keys;
use big_audio_effects::equalizer::EQ_BAND_FREQUENCIES_F64;
use big_audio_effects::noise_reduction::GtcrnControls;

pub use super::audio_effects_eq::create_equalizer_dialog;

/// 10 semicolon-separated dB gains, clamped to ±12 dB; missing → 0.
pub fn parse_eq_bands(eq_str: &str) -> [f64; 10] {
    big_media_components::audio::equalizer_controls::parse_clamped_band_gains::<10>(
        eq_str, -12.0, 12.0,
    )
}

pub(crate) use big_audio_effects::equalizer::EQ_BAND_LABELS as EQ_LABELS;

#[must_use]
pub fn noise_model_labels() -> Vec<String> {
    REALTIME_LAVFI_MODELS
        .iter()
        .zip(REALTIME_LAVFI_MODEL_LABELS)
        .map(|(model, label)| {
            let label = i18n(label);
            if model.plugin_available() {
                label
            } else {
                format!("{} ({})", label, i18n("not installed"))
            }
        })
        .collect()
}

#[must_use]
pub fn noise_model_index(model: i32) -> u32 {
    NoiseModel::from_i32_or_default(model).realtime_lavfi_index()
}

#[must_use]
pub fn has_noise_reduction_backend() -> bool {
    any_realtime_lavfi_model_available()
}

/// Live update for the running LADSPA plugin; v7 wire format = 24 bytes,
/// 6 LE f32: [strength, model_type, speech_strength, lookahead_ms, blend, voice_recovery].
pub fn write_gtcrn_controls_raw(
    strength: f64,
    model: i32,
    speech_strength: f64,
    model_blend: bool,
    voice_recovery: f64,
) {
    let path = glib::user_runtime_dir().join("gtcrn-ladspa-controls");
    let controls = GtcrnControls {
        strength,
        model,
        speech_strength,
        lookahead_ms: 50.0,
        model_blend,
        voice_recovery,
    };
    let buf = gtcrn_control_bytes(&controls);
    atomic_write(&path, &buf);
    debug!(
        "NR: write_gtcrn_controls_raw: strength={strength}, model={model}, voice_recovery={voice_recovery}"
    );
}
pub fn write_gtcrn_controls(settings: &crate::settings::Settings) {
    let strength = settings.get_double(keys::NOISE_STRENGTH).max(0.0) as f32;
    let model_type = settings.get_int(keys::NOISE_MODEL) as f32;
    let speech_strength = strength; // same as slider — user controls overall NR intensity
    let lookahead_ms = 50.0_f32;
    let model_blend = if settings.get_bool(keys::NOISE_MODEL_BLEND) {
        1.0_f32
    } else {
        0.0_f32
    };
    let voice_recovery = settings.get_double(keys::NOISE_VOICE_RECOVERY) as f32;

    let path = glib::user_runtime_dir().join("gtcrn-ladspa-controls");
    let controls = GtcrnControls {
        strength: f64::from(strength),
        model: model_type as i32,
        speech_strength: f64::from(speech_strength),
        lookahead_ms: f64::from(lookahead_ms),
        model_blend: model_blend > 0.0,
        voice_recovery: f64::from(voice_recovery),
    };
    let buf = gtcrn_control_bytes(&controls);
    atomic_write(&path, &buf);
    debug!(
        "NR: write_gtcrn_controls: strength={strength}, model={model_type}, blend={model_blend}, voice_recovery={voice_recovery}, path={}",
        path.display()
    );
}

fn gtcrn_control_bytes(controls: &GtcrnControls) -> [u8; 24] {
    let mut buf = [0_u8; 24];
    let [
        strength,
        model_type,
        speech_strength,
        lookahead_ms,
        model_blend,
        voice_recovery,
    ] = controls.control_values();
    buf[0..4].copy_from_slice(&strength.to_le_bytes());
    buf[4..8].copy_from_slice(&model_type.to_le_bytes());
    buf[8..12].copy_from_slice(&speech_strength.to_le_bytes());
    buf[12..16].copy_from_slice(&lookahead_ms.to_le_bytes());
    buf[16..20].copy_from_slice(&model_blend.to_le_bytes());
    buf[20..24].copy_from_slice(&voice_recovery.to_le_bytes());
    buf
}

/// Atomic write: temp file + rename avoids truncation races.
fn atomic_write(path: &std::path::Path, payload_bytes: &[u8]) {
    let temporary_path = path.with_extension("tmp");
    match std::fs::write(&temporary_path, payload_bytes) {
        Ok(()) => {
            if let Err(e) = std::fs::rename(&temporary_path, path) {
                warn!(
                    "NR: rename {} → {} failed: {e}",
                    temporary_path.display(),
                    path.display()
                );
                let _ = std::fs::write(path, payload_bytes);
            }
        }
        Err(e) => {
            warn!("NR: write {} failed: {e}", temporary_path.display());
        }
    }
}

pub fn rebuild_filters(
    settings: &crate::settings::Settings,
    eq_gains: &[f64; 10],
    mpv: &MpvBackend,
) {
    let nr_strength = settings.get_double(keys::NOISE_STRENGTH).max(0.0);
    let nr_model = settings.get_int(keys::NOISE_MODEL);
    let nr_model_blend = settings.get_bool(keys::NOISE_MODEL_BLEND);
    let nr_voice_recovery = settings.get_double(keys::NOISE_VOICE_RECOVERY);

    write_gtcrn_controls(settings);

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
        nr_model_blend,
        nr_voice_recovery,
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

pub fn create_nr_settings_dialog(settings: &crate::settings::Settings) -> adw::Window {
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
            "Powered by GTCRN — a neural network that separates voice from \
             background noise in real time. The default settings work well \
             for most cases.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    intro.add_css_class("dim-label");
    intro.set_margin_bottom(4);
    content.append(&intro);

    let (model_card, model_dropdown) = build_model_card(settings);
    content.append(&model_card);

    let (vr_card, vr_scale) = build_voice_recovery_card(settings);
    content.append(&vr_card);

    // LADSPA plugin re-reads control file each audio frame; no mpv reload needed.
    {
        let s = settings.clone();
        model_dropdown.connect_selected_notify(move |dd| {
            let _ = s.put_int(keys::NOISE_MODEL, dd.selected() as i32);
            let _ = s.put_bool(keys::NOISE_MODEL_BLEND, false);
            write_gtcrn_controls(&s);
        });
    }
    {
        let s = settings.clone();
        vr_scale.connect_value_changed(move |scale| {
            let _ = s.put_double(keys::NOISE_VOICE_RECOVERY, scale.value());
            write_gtcrn_controls(&s);
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

    let dns3_desc = gtk::Label::builder()
        .label(i18n(
            "• DNS3 — General-purpose. Good at removing steady noises like \
             fans, air conditioners, and hum.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    dns3_desc.add_css_class("dim-label");
    options_box.append(&dns3_desc);

    let vctk_desc = gtk::Label::builder()
        .label(i18n(
            "• VCTK — Trained on clear voices. Preserves more vocal \
             detail and sounds more natural.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    vctk_desc.add_css_class("dim-label");
    options_box.append(&vctk_desc);

    let deepfilter_desc = gtk::Label::builder()
        .label(i18n(
            "• DeepFilterNet3 / DPDFNet HR — Full-band 48 kHz models. \
             Cleaner output when the optional LADSPA plugins are installed.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    deepfilter_desc.add_css_class("dim-label");
    options_box.append(&deepfilter_desc);

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

fn build_voice_recovery_card(settings: &crate::settings::Settings) -> (gtk::Box, gtk::Scale) {
    let vr_card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    vr_card.add_css_class("card");

    let vr_inner = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let vr_header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .build();
    let vr_icon = gtk::Image::from_icon_name("audio-speakers-symbolic");
    vr_icon.set_pixel_size(24);
    vr_icon.set_valign(gtk::Align::Center);
    vr_header.append(&vr_icon);
    let vr_title = gtk::Label::builder()
        .label(i18n("Voice Recovery"))
        .hexpand(true)
        .xalign(0.0)
        .build();
    vr_title.add_css_class("heading");
    vr_header.append(&vr_title);
    vr_inner.append(&vr_header);

    let vr_desc = gtk::Label::builder()
        .label(i18n(
            "Noise reduction can sometimes muffle high-pitched parts of the \
             voice. This slider restores those frequencies, making the voice \
             sound more natural.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    vr_desc.add_css_class("dim-label");
    vr_inner.append(&vr_desc);

    let vr_hint = gtk::Label::builder()
        .label(i18n(
            "Higher values recover more voice detail but may let a small \
             amount of noise through. The default (0.75) is a good balance.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    vr_hint.add_css_class("dim-label");
    vr_inner.append(&vr_hint);

    let stored_vr = settings.get_double(keys::NOISE_VOICE_RECOVERY);
    let vr_scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 1.0, 0.05);
    vr_scale.set_hexpand(true);
    vr_scale.set_value(stored_vr);
    vr_scale.set_draw_value(true);
    vr_scale.set_value_pos(gtk::PositionType::Right);
    vr_scale.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Voice recovery level",
    ))]);
    vr_scale.set_margin_top(4);
    vr_inner.append(&vr_scale);

    vr_card.append(&vr_inner);

    (vr_card, vr_scale)
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
