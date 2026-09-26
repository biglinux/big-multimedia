//! GPU-property and shader filter groups for the filters dialog: GPU
//! processing (mpv render properties), GPU upscaling shaders (FSR/CAS), neural
//! upscaling (RAVU), and Anime4K presets, driving an `Arc<MpvBackend>` +
//! `Settings`. Shader percentage
//! math lives in [`super::filter_specs`].
//!
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};

use super::filter_specs::{
    cas_shader_to_ui_pct, cas_ui_pct_to_shader, fsr_shader_to_ui_pct, fsr_ui_pct_to_shader,
};
use crate::config;
use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::player::mpv_backend::shader_gen::{self, ShaderParam};
use crate::settings::{Settings, keys};
use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;

/// GPU processing: sharpen (debounced), deband, sigmoid-upscaling,
/// correct-downscaling — all live mpv render properties.
pub(super) fn build_gpu_group(mpv: &Arc<MpvBackend>) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("GPU Processing")).build();

    group.add(&build_gpu_sharpen_row(mpv));
    group.add(&build_gpu_bool_row(
        mpv,
        "deband",
        i18n("Deband"),
        i18n("Remove color banding artifacts"),
        MpvBackend::set_deband,
    ));
    group.add(&build_gpu_bool_row(
        mpv,
        "sigmoid-upscaling",
        i18n("Sigmoid Upscaling"),
        i18n("Reduces ringing when scaling video up"),
        MpvBackend::set_sigmoid_upscaling,
    ));
    group.add(&build_gpu_bool_row(
        mpv,
        "correct-downscaling",
        i18n("Correct Downscaling"),
        i18n("Higher quality when scaling video down"),
        MpvBackend::set_correct_downscaling,
    ));
    group
}

/// GPU sharpen expander (0..5, debounced 100 ms).
fn build_gpu_sharpen_row(mpv: &Arc<MpvBackend>) -> adw::ExpanderRow {
    let current = mpv.get_property_f64("sharpen");
    let is_active = current.abs() > 0.001;
    let expander = adw::ExpanderRow::builder()
        .title(i18n("Sharpen"))
        .subtitle(i18n("GPU sharpening (unsharp mask)"))
        .show_enable_switch(true)
        .enable_expansion(is_active)
        .build();

    let adjustment = gtk::Adjustment::new(
        if is_active { current } else { 0.5 },
        0.0,
        5.0,
        0.1,
        0.5,
        0.0,
    );
    expander.add_row(
        &adw::SpinRow::builder()
            .title(i18n("Amount"))
            .adjustment(&adjustment)
            .digits(1)
            .build(),
    );

    let mpv_toggle = Arc::clone(mpv);
    let adjustment_toggle = adjustment.clone();
    expander.connect_enable_expansion_notify(move |row| {
        mpv_toggle.set_sharpen(if row.enables_expansion() {
            adjustment_toggle.value()
        } else {
            0.0
        });
    });

    let debounce = DebouncedAction::new(Duration::from_millis(100));
    // Weak back-ref: the expander owns the SpinRow (→ adjustment), so a strong
    // capture here would form a mutual reference cycle (both leak on close).
    let expander_weak = expander.downgrade();
    let mpv = Arc::clone(mpv);
    adjustment.connect_value_changed(move |adjustment| {
        let Some(expander_ref) = expander_weak.upgrade() else {
            return;
        };
        if !expander_ref.enables_expansion() {
            return;
        }
        let value = adjustment.value();
        let mpv = Arc::clone(&mpv);
        debounce.run(move || {
            mpv.set_sharpen(value);
        });
    });

    expander
}

/// A GPU boolean property toggle row seeded from the live mpv property.
fn build_gpu_bool_row(
    mpv: &Arc<MpvBackend>,
    property: &str,
    title: String,
    subtitle: String,
    setter: fn(&MpvBackend, bool),
) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(mpv.get_property_bool(property))
        .build();
    let mpv = Arc::clone(mpv);
    row.connect_active_notify(move |row| setter(&mpv, row.is_active()));
    row
}

/// GPU upscaling shaders: None / FSR / CAS with a sharpening-intensity spin.
pub(super) fn build_shaders_group(
    mpv: &Arc<MpvBackend>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("GPU Upscaling Shaders")).build();

    if !std::path::Path::new(config::SHADERS_DIR).is_dir() {
        group.add(&not_installed_row());
        return group;
    }

    let current = settings.get_string(keys::BUILTIN_UPSCALER);
    let fsr_value = settings.get_double(keys::SHADER_FSR_SHARPNESS);
    let cas_value = settings.get_double(keys::SHADER_CAS_SHARPENING);

    let ids = ["", "fsr", "cas"];
    let (upscaling_shader_row, combo) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Upscaling Shader"),
            [
                i18n("None"),
                i18n("FidelityFX Super Resolution (FSR)"),
                i18n("CAS Upscaling"),
            ],
        )
        .subtitle(i18n(
            "Enhances image quality when the video resolution is lower than your display",
        )),
    )
    .into_parts();
    combo.set_selected(ids.iter().position(|id| *id == current).unwrap_or(0) as u32);
    group.add(&upscaling_shader_row);

    let fsr_default = if fsr_value == 0.0 { 0.2 } else { fsr_value };
    let intensity = gtk::Adjustment::new(
        match current.as_str() {
            "fsr" => fsr_shader_to_ui_pct(fsr_default),
            "cas" => cas_shader_to_ui_pct(cas_value),
            _ => 50.0,
        },
        0.0,
        100.0,
        1.0,
        10.0,
        0.0,
    );
    let intensity_row = adw::SpinRow::builder()
        .title(i18n("Sharpening Intensity"))
        .subtitle(i18n(
            "Higher values produce a more visible sharpening effect",
        ))
        .adjustment(&intensity)
        .numeric(true)
        .build();
    intensity_row.set_visible(!current.is_empty());
    group.add(&intensity_row);

    let mpv_combo = Arc::clone(mpv);
    let settings_combo = settings.clone();
    let intensity_for_combo = intensity.clone();
    combo.connect_selected_notify(move |combo| {
        let idx = combo.selected() as usize;
        let key = ids.get(idx).copied().unwrap_or("");
        intensity_row.set_visible(!key.is_empty());

        intensity_for_combo.set_value(saved_upscaler_percent(&settings_combo, key));
        apply_builtin_upscaler(&mpv_combo, key, intensity_for_combo.value());
        settings_combo.put_string(keys::BUILTIN_UPSCALER, key);
    });

    let debounce = DebouncedAction::new(Duration::from_millis(300));
    let settings_spin = settings.clone();
    // Weak back-ref: `combo`'s own handler strong-holds this intensity row, so a
    // strong capture here would form a mutual reference cycle (both leak).
    let combo_weak = combo.downgrade();
    let mpv_spin = Arc::clone(mpv);
    intensity.connect_value_changed(move |intensity| {
        let Some(combo_for_spin) = combo_weak.upgrade() else {
            return;
        };
        let idx = combo_for_spin.selected() as usize;
        let key = ids.get(idx).copied().unwrap_or("").to_string();
        let pct = intensity.value();
        match key.as_str() {
            "fsr" => {
                settings_spin.put_double(keys::SHADER_FSR_SHARPNESS, fsr_ui_pct_to_shader(pct));
            }
            "cas" => {
                settings_spin.put_double(keys::SHADER_CAS_SHARPENING, cas_ui_pct_to_shader(pct));
            }
            _ => {}
        }
        let mpv = Arc::clone(&mpv_spin);
        debounce.run(move || {
            apply_builtin_upscaler(&mpv, &key, pct);
        });
    });

    group
}

/// The intensity (UI percent) saved for upscaler `key`.
fn saved_upscaler_percent(settings: &Settings, key: &str) -> f64 {
    match key {
        "fsr" => {
            let v = settings.get_double(keys::SHADER_FSR_SHARPNESS);
            fsr_shader_to_ui_pct(if v == 0.0 { 0.2 } else { v })
        }
        "cas" => cas_shader_to_ui_pct(settings.get_double(keys::SHADER_CAS_SHARPENING)),
        _ => 50.0,
    }
}

/// Swap the FSR/CAS upscaler for `key` at `pct` ("" removes it): clears
/// prior built-in/user shaders by prefix, then appends the chosen one.
fn apply_builtin_upscaler(mpv: &MpvBackend, key: &str, pct: f64) {
    mpv.glsl_shader_remove_prefix(config::SHADERS_DIR);
    if let Some(user_dir) = gtk::glib::user_config_dir()
        .join(config::video::USER_SHADERS_RELATIVE)
        .to_str()
    {
        mpv.glsl_shader_remove_prefix(user_dir);
    }
    let path = match key {
        "fsr" => shader_gen::shader_path(
            "FSR.glsl",
            &[ShaderParam {
                define_name: "SHARPNESS",
                value: fsr_ui_pct_to_shader(pct),
                precision: 2,
            }],
        ),
        "cas" => shader_gen::shader_path(
            "CAS-scaled.glsl",
            &[ShaderParam {
                define_name: "SHARPENING",
                value: cas_ui_pct_to_shader(pct),
                precision: 2,
            }],
        ),
        _ => return,
    };
    mpv.glsl_shader_append(&path);
}

fn apply_neural_upscaler(mpv: &MpvBackend, variant: &str) {
    if let Some(dir) = shader_gen::ravu_dir().to_str() {
        mpv.glsl_shader_remove_prefix(dir);
    }
    if let Some(path) = shader_gen::neural_upscaler_path(variant) {
        mpv.glsl_shader_append(&path);
    }
}

fn apply_anime4k(mpv: &MpvBackend, preset: &str) {
    let dir = shader_gen::anime4k_dir();
    if let Some(dir_str) = dir.to_str() {
        mpv.glsl_shader_remove_prefix(dir_str);
    }
    for filename in shader_gen::anime4k_preset_shaders(preset) {
        let path = dir.join(filename);
        if let Some(path) = path.exists().then(|| path.to_str()).flatten() {
            mpv.glsl_shader_append(path);
        }
    }
}

/// The upscaler, neural upscaler and Anime4K preset the dialog saved, each
/// empty when off or when its shaders are not installed.
pub(crate) fn saved_enhancements(settings: &Settings) -> [String; 3] {
    let available = [
        std::path::Path::new(config::SHADERS_DIR).is_dir(),
        shader_gen::neural_shaders_available(),
        shader_gen::anime4k_dir().is_dir(),
    ];
    let keys = [
        keys::BUILTIN_UPSCALER,
        keys::NEURAL_UPSCALER,
        keys::ANIME4K_PRESET,
    ];
    std::array::from_fn(|i| {
        if available[i] {
            settings.get_string(keys[i])
        } else {
            String::new()
        }
    })
}

/// Put the saved choices on the picture: they outlive the session, so the
/// shaders must too (the dialog shows them as on).
pub(crate) fn apply_saved_enhancements(mpv: &MpvBackend, settings: &Settings) {
    let [upscaler, neural, anime4k] = saved_enhancements(settings);
    apply_builtin_upscaler(mpv, &upscaler, saved_upscaler_percent(settings, &upscaler));
    apply_neural_upscaler(mpv, &neural);
    apply_anime4k(mpv, &anime4k);
}

/// Neural upscaling: single RAVU-r4 toggle.
pub(super) fn build_neural_group(
    mpv: &Arc<MpvBackend>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Neural Network Upscaling")).build();

    if !shader_gen::neural_shaders_available() {
        group.add(&neural_not_installed_row());
        return group;
    }

    let is_active = settings.get_string(keys::NEURAL_UPSCALER) == "ravu-r4";
    let row = adw::SwitchRow::builder()
        .title(i18n("RAVU Neural Upscaler"))
        .subtitle(i18n(
            "Reconstructs detail lost during video compression — \
             uses RAVU r4, the best fragment-shader neural upscaler available",
        ))
        .active(is_active)
        .build();

    let mpv = Arc::clone(mpv);
    let settings = settings.clone();
    row.connect_active_notify(move |row| {
        let variant = if row.is_active() { "ravu-r4" } else { "" };
        apply_neural_upscaler(&mpv, variant);
        settings.put_string(keys::NEURAL_UPSCALER, variant);
    });

    group.add(&row);
    group
}

/// Anime4K preset selector.
pub(super) fn build_anime4k_group(
    mpv: &Arc<MpvBackend>,
    settings: &Settings,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Anime / Cartoon Enhancement")).build();

    if !shader_gen::anime4k_dir().is_dir() {
        group.add(&anime4k_not_installed_row());
        return group;
    }

    let current = settings.get_string(keys::ANIME4K_PRESET);
    let ids = ["", "a", "b", "c"];
    let (anime4k_preset_row, combo) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Anime4K Preset"),
            [
                i18n("Disabled"),
                i18n("Quality A — Best (1080p sources)"),
                i18n("Quality B — Soft Restore (720p sources)"),
                i18n("Quality C — Fast (low-end GPU)"),
            ],
        )
        .subtitle(i18n(
            "Select a quality preset — higher quality uses more GPU power",
        )),
    )
    .into_parts();
    combo.set_selected(ids.iter().position(|id| *id == current).unwrap_or(0) as u32);

    let mpv = Arc::clone(mpv);
    let settings = settings.clone();
    combo.connect_selected_notify(move |combo| {
        let idx = combo.selected() as usize;
        let preset = ids.get(idx).copied().unwrap_or("");
        apply_anime4k(&mpv, preset);
        settings.put_string(keys::ANIME4K_PRESET, preset);
    });

    group.add(&anime4k_preset_row);
    group
}

fn not_installed_row() -> adw::ActionRow {
    BigDetailRow::new(
        BigDetailRowSpec::new(i18n("Shaders not installed")).subtitle(i18n(
            "Install the big-media-runtime package to get built-in shaders",
        )),
    )
    .into_root()
}

fn neural_not_installed_row() -> adw::ActionRow {
    BigDetailRow::new(
        BigDetailRowSpec::new(i18n("Neural upscaler shaders not installed")).subtitle(i18n(
            "Install the big-media-runtime package to get neural upscaling shaders",
        )),
    )
    .into_root()
}

fn anime4k_not_installed_row() -> adw::ActionRow {
    BigDetailRow::new(
        BigDetailRowSpec::new(i18n("Anime4K shaders not installed")).subtitle(i18n(
            "Install the big-media-runtime package to get Anime4K shaders",
        )),
    )
    .into_root()
}
