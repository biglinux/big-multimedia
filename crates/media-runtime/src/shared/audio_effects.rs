//! Binds audio effect components to mpv/settings/noise-reduction runtime.

use std::rc::Rc;

use adw::prelude::*;
use big_audio_effects::noise_reduction::{MODELS, NoiseModel, any_model_available};

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
        nr_strength,
        nr_model,
        nr_mono: settings.get_bool(keys::NOISE_MONO),
        nr_voice_only: settings.noise_voice_only(),
        eq_bands: &eq,
    });
}

pub fn apply_voice_change(settings: &crate::settings::Settings, mpv: &MpvBackend) {
    let eq_str = settings.get_string(keys::EQ_BANDS);
    let gains = parse_eq_bands(&eq_str);
    rebuild_filters(settings, &gains, mpv);
}

/// The noise reduction's settings, shown beside the voice slider as in the
/// BigLinux converters: the model, mono, and keeping only speech. Each
/// change is saved, then `on_change` runs to rebuild the playing chain.
pub fn nr_settings_panel(
    settings: &crate::settings::Settings,
    on_change: impl Fn() + 'static,
) -> gtk::Box {
    let on_change: Rc<dyn Fn()> = Rc::new(on_change);
    let panel = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .width_request(300)
        .valign(gtk::Align::Center)
        .build();
    append_model_choice(&panel, settings, &on_change);
    panel.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

    let mono = switch_line(
        &i18n("Convert to mono before filtering"),
        settings.get_bool(keys::NOISE_MONO),
    );
    {
        let s = settings.clone();
        let on_change = on_change.clone();
        mono.1.connect_active_notify(move |switch| {
            let _ = s.put_bool(keys::NOISE_MONO, switch.is_active());
            on_change();
        });
    }
    panel.append(&mono.0);
    append_voice_only(&panel, settings, &on_change);
    panel
}

/// DeepFilterNet3 and DPDFNet-2 as two choices, and the package a missing
/// one needs.
fn append_model_choice(
    panel: &gtk::Box,
    settings: &crate::settings::Settings,
    on_change: &Rc<dyn Fn()>,
) {
    let quality = gtk::Label::builder()
        .label(i18n("Cleaning quality"))
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    panel.append(&quality);
    let (models, missing) = model_toggles();
    let stored = NoiseModel::from_i32_or_default(settings.get_int(keys::NOISE_MODEL));
    models.set_active_name(Some(&stored.index().to_string()));
    {
        let s = settings.clone();
        let on_change = on_change.clone();
        models.connect_active_name_notify(move |group| {
            let Some(index) = group
                .active_name()
                .and_then(|name| name.parse::<i32>().ok())
            else {
                return;
            };
            if s.get_int(keys::NOISE_MODEL) != index {
                let _ = s.put_int(keys::NOISE_MODEL, index);
                on_change();
            }
        });
    }
    panel.append(&models);
    for package in missing {
        panel.append(
            &gtk::Label::builder()
                .label(i18n("Unavailable: install {package}").replace("{package}", package))
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "warning"])
                .build(),
        );
    }
}

/// One toggle per model, a missing one disabled, and the packages missing.
fn model_toggles() -> (adw::ToggleGroup, Vec<&'static str>) {
    let models = adw::ToggleGroup::builder()
        .homogeneous(true)
        .hexpand(true)
        .build();
    models.update_property(&[gtk::accessible::Property::Label(&i18n("Cleaning quality"))]);
    let mut missing = Vec::new();
    for model in MODELS {
        // Named by what the choice changes.
        let label = match model {
            NoiseModel::DeepFilterNet3 => i18n("Faster"),
            NoiseModel::DpdfNet2 => i18n("Better, slower"),
        };
        if !model.plugin_available() {
            missing.push(model.package());
        }
        models.add(
            adw::Toggle::builder()
                .label(label)
                .name(model.index().to_string())
                .enabled(model.plugin_available())
                .build(),
        );
    }
    (models, missing)
}

/// The voice-only switch and its detection sensitivity, named from 1, very
/// sensitive, to 5, very strict.
fn append_voice_only(
    panel: &gtk::Box,
    settings: &crate::settings::Settings,
    on_change: &Rc<dyn Fn()>,
) {
    // The denoiser mutes what its voice detector does not take for a voice:
    // music, noise and the silence between words.
    let voice_only = switch_line(
        &i18n("Audio only when a voice is detected"),
        settings.get_bool(keys::NOISE_VOICE_ONLY),
    );
    panel.append(&voice_only.0);

    let names = [
        i18n("Very sensitive"),
        i18n("Sensitive"),
        i18n("Normal"),
        i18n("Strict"),
        i18n("Very strict"),
    ];
    let level = settings.get_int(keys::NOISE_VOICE_SENSITIVITY).clamp(1, 5);
    let (heading, level_name, scale) = detection_controls(level, &names);
    let detection = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .sensitive(voice_only.1.is_active())
        .build();
    detection.append(&heading);
    detection.append(&scale);
    panel.append(&detection);
    {
        let s = settings.clone();
        let on_change = on_change.clone();
        let detection = detection.downgrade();
        voice_only.1.connect_active_notify(move |switch| {
            let _ = s.put_bool(keys::NOISE_VOICE_ONLY, switch.is_active());
            if let Some(detection) = detection.upgrade() {
                detection.set_sensitive(switch.is_active());
            }
            on_change();
        });
    }
    {
        let s = settings.clone();
        let on_change = on_change.clone();
        let level_name = level_name.downgrade();
        scale.connect_value_changed(move |scale| {
            let level = scale.value().round().clamp(1.0, 5.0) as i32;
            if s.get_int(keys::NOISE_VOICE_SENSITIVITY) == level {
                return;
            }
            let _ = s.put_int(keys::NOISE_VOICE_SENSITIVITY, level);
            if let Some(level_name) = level_name.upgrade() {
                level_name.set_label(&names[(level - 1) as usize]);
            }
            on_change();
        });
    }
}

/// The detection heading, which names `level`, and its five-step scale.
fn detection_controls(level: i32, names: &[String; 5]) -> (gtk::Box, gtk::Label, gtk::Scale) {
    let level_name = gtk::Label::builder()
        .label(names[(level - 1) as usize].as_str())
        .css_classes(["dim-label"])
        .build();
    let heading = gtk::Box::builder().spacing(12).build();
    heading.append(
        &gtk::Label::builder()
            .label(i18n("Voice detection"))
            .xalign(0.0)
            .hexpand(true)
            .build(),
    );
    heading.append(&level_name);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 5.0, 1.0);
    scale.set_round_digits(0);
    scale.set_draw_value(false);
    scale.set_value(f64::from(level));
    for mark in 1..=5 {
        scale.add_mark(f64::from(mark), gtk::PositionType::Bottom, None);
    }
    scale.update_property(&[gtk::accessible::Property::Label(&i18n("Voice detection"))]);
    (heading, level_name, scale)
}

/// A wrapping label and its switch, the label naming the switch.
fn switch_line(label: &str, active: bool) -> (gtk::Box, gtk::Switch) {
    let line = gtk::Box::builder().spacing(12).build();
    line.append(
        &gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .wrap(true)
            .max_width_chars(26)
            .hexpand(true)
            .build(),
    );
    let switch = gtk::Switch::builder()
        .active(active)
        .valign(gtk::Align::Center)
        .build();
    switch.update_property(&[gtk::accessible::Property::Label(label)]);
    line.append(&switch);
    (line, switch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_gains_clamp_to_12_db() {
        let gains = parse_eq_bands("100.0;-100.0;3.5");
        assert_eq!(gains[..3], [12.0, -12.0, 3.5]);
        assert_eq!(gains[3..], [0.0; 7]);
    }
}
