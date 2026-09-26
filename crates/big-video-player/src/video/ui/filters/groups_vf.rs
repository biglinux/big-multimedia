//! `vf`-chain filter groups for the filters dialog: denoise/sharpen, color,
//! style presets, and effects. Each builds an `adw::PreferencesGroup` whose
//! controls drive `mpv.vf_append`/`mpv.vf_remove` on an `Arc<MpvBackend>`
//! directly. Filter string
//! math lives in [`super::filter_specs`].

use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use super::filter_specs::{colorize_vf, colortemp_vf, hqdn3d_vf, pixelize_vf};
use super::vf_row::{VfFilterSpec, build_vf_expander_row, illustration};
use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;

/// Denoise (hqdn3d) + software CAS sharpen.
pub(super) fn build_denoise_sharpen_group(
    mpv: &Arc<MpvBackend>,
    current_vf: &str,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Denoise and Sharpen"))
        .description(i18n(
            "Software filters that clean up or enhance the image. \
             Useful for old or low-quality video sources.",
        ))
        .build();

    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "hqdn3d",
            svg: "denoise.svg",
            title: i18n("Denoise (hqdn3d)"),
            subtitle: i18n("Reduces visual noise and grain from old or low-light footage"),
            spin_title: i18n("Strength"),
            adjustment: (4.0, 1.0, 16.0, 0.5, 1.0),
            digits: 0,
            debounce_ms: 300,
            format_vf: hqdn3d_vf,
        },
    ));

    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "cas",
            svg: "sharpen_cas.svg",
            title: i18n("Sharpen (CAS)"),
            subtitle: i18n("Software contrast-adaptive sharpening — works at any resolution"),
            spin_title: i18n("Strength"),
            adjustment: (0.5, 0.0, 1.0, 0.05, 0.1),
            digits: 2,
            debounce_ms: 300,
            format_vf: |v| format!("cas=strength={v:.2}"),
        },
    ));

    group
}

/// Color: temperature, vibrance, colorize (hue/sat/mix), monochrome.
pub(super) fn build_color_group(mpv: &Arc<MpvBackend>, current_vf: &str) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Color")).build();

    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "colortemp",
            svg: "color_temp.svg",
            title: i18n("Color Temperature"),
            subtitle: i18n("Warm or cool tone (Kelvin)"),
            spin_title: i18n("Temperature"),
            adjustment: (6500.0, 1000.0, 40000.0, 100.0, 500.0),
            digits: 0,
            debounce_ms: 300,
            format_vf: colortemp_vf,
        },
    ));

    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "vibrance",
            svg: "vibrance.svg",
            title: i18n("Vibrance"),
            subtitle: i18n("Boost color saturation intelligently"),
            spin_title: i18n("Intensity"),
            adjustment: (0.5, -2.0, 2.0, 0.1, 0.5),
            digits: 1,
            debounce_ms: 300,
            format_vf: |v| format!("vibrance=intensity={v:.1}"),
        },
    ));

    group.add(&build_colorize_row(mpv, current_vf));
    group.add(&build_monochrome_row(mpv, current_vf));
    group
}

/// Colorize: a 3-spin (hue/sat/mix) expander applied with a shared debounce.
fn build_colorize_row(mpv: &Arc<MpvBackend>, current_vf: &str) -> adw::ExpanderRow {
    const LABEL: &str = "colorize";
    let expander = adw::ExpanderRow::builder()
        .title(i18n("Colorize"))
        .subtitle(i18n("Apply a color tint overlay"))
        .show_enable_switch(true)
        .enable_expansion(current_vf.contains(LABEL))
        .build();
    expander.add_prefix(&illustration("colorize.svg"));

    let hue = gtk::Adjustment::new(30.0, 0.0, 360.0, 5.0, 30.0, 0.0);
    let saturation = gtk::Adjustment::new(0.5, 0.0, 1.0, 0.05, 0.1, 0.0);
    let mix = gtk::Adjustment::new(0.5, 0.0, 1.0, 0.05, 0.1, 0.0);
    expander.add_row(
        &adw::SpinRow::builder()
            .title(i18n("Hue"))
            .adjustment(&hue)
            .digits(0)
            .build(),
    );
    expander.add_row(
        &adw::SpinRow::builder()
            .title(i18n("Saturation"))
            .adjustment(&saturation)
            .digits(2)
            .build(),
    );
    expander.add_row(
        &adw::SpinRow::builder()
            .title(i18n("Mix"))
            .adjustment(&mix)
            .digits(2)
            .build(),
    );

    let apply = {
        let mpv = Arc::clone(mpv);
        let hue = hue.clone();
        let saturation = saturation.clone();
        let mix = mix.clone();
        move || {
            mpv.vf_append(
                LABEL,
                &colorize_vf(hue.value(), saturation.value(), mix.value()),
            );
        }
    };

    let apply_on_toggle = apply.clone();
    let mpv_toggle = Arc::clone(mpv);
    expander.connect_enable_expansion_notify(move |row| {
        if row.enables_expansion() {
            apply_on_toggle();
        } else {
            mpv_toggle.vf_remove(LABEL);
        }
    });

    let debounce = DebouncedAction::new(Duration::from_millis(300));
    for adjustment in [&hue, &saturation, &mix] {
        let apply = apply.clone();
        let debounce = debounce.clone();
        // Weak back-ref to avoid a mutual reference cycle (leaks on close).
        let expander_weak = expander.downgrade();
        adjustment.connect_value_changed(move |_| {
            let Some(expander_ref) = expander_weak.upgrade() else {
                return;
            };
            if expander_ref.enables_expansion() {
                debounce.run(apply.clone());
            }
        });
    }

    expander
}

/// Monochrome: a single toggle row.
fn build_monochrome_row(mpv: &Arc<MpvBackend>, current_vf: &str) -> adw::SwitchRow {
    const LABEL: &str = "monochrome";
    let row = adw::SwitchRow::builder()
        .title(i18n("Monochrome"))
        .subtitle(i18n("Cinematic black and white"))
        .active(current_vf.contains(LABEL))
        .build();
    row.add_prefix(&illustration("monochrome.svg"));
    let mpv = Arc::clone(mpv);
    row.connect_active_notify(move |row| {
        if row.is_active() {
            mpv.vf_append(LABEL, "monochrome");
        } else {
            mpv.vf_remove(LABEL);
        }
    });
    row
}

/// Style presets: a `curves=preset=<id>` expander + preset combo.
pub(super) fn build_presets_group(
    mpv: &Arc<MpvBackend>,
    current_vf: &str,
) -> adw::PreferencesGroup {
    const LABEL: &str = "curves_preset";
    let group = BigPreferencesGroupSpec::new(i18n("Style Presets"))
        .description(i18n("One-click color grading effects"))
        .build();

    let expander = adw::ExpanderRow::builder()
        .title(i18n("Color Curves"))
        .subtitle(i18n("Predefined color grading looks"))
        .show_enable_switch(true)
        .enable_expansion(current_vf.contains(LABEL))
        .build();
    expander.add_prefix(&illustration("curves.svg"));

    let preset_ids = [
        "vintage",
        "cross_process",
        "darker",
        "lighter",
        "increase_contrast",
        "strong_contrast",
        "color_negative",
    ];
    let (preset_row, combo) = BigDropdownRow::new(BigDropdownRowSpec::new(
        i18n("Preset"),
        [
            i18n("Vintage"),
            i18n("Cross Process"),
            i18n("Darker"),
            i18n("Lighter"),
            i18n("More Contrast"),
            i18n("Strong Contrast"),
            i18n("Color Negative"),
        ],
    ))
    .into_parts();
    expander.add_row(&preset_row);

    let apply = {
        let mpv = Arc::clone(mpv);
        let combo = combo.clone();
        move || {
            let idx = combo.selected() as usize;
            if let Some(id) = preset_ids.get(idx) {
                mpv.vf_append(LABEL, &format!("curves=preset={id}"));
            }
        }
    };

    let apply_on_toggle = apply.clone();
    let mpv_toggle = Arc::clone(mpv);
    expander.connect_enable_expansion_notify(move |row| {
        if row.enables_expansion() {
            apply_on_toggle();
        } else {
            mpv_toggle.vf_remove(LABEL);
        }
    });
    // Weak back-ref: the expander owns the combo; a strong capture here would be
    // a mutual reference cycle (leaks on dialog close).
    let expander_weak = expander.downgrade();
    combo.connect_selected_notify(move |_| {
        let Some(expander_ref) = expander_weak.upgrade() else {
            return;
        };
        if expander_ref.enables_expansion() {
            apply();
        }
    });

    group.add(&expander);
    group
}

/// Effects: simple toggles + parameterized expanders.
pub(super) fn build_effects_group(
    mpv: &Arc<MpvBackend>,
    current_vf: &str,
) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Effects"))
        .description(i18n(
            "Creative visual effects and artistic transformations.",
        ))
        .build();

    let toggles: &[(&str, &str, &str, String, String)] = &[(
        "vignette",
        "vignette",
        "vignette.svg",
        i18n("Vignette"),
        i18n("Darken edges of the image"),
    )];
    for (label, filter, svg, title, subtitle) in toggles {
        let row = adw::SwitchRow::builder()
            .title(title)
            .subtitle(subtitle)
            .active(current_vf.contains(label))
            .build();
        row.add_prefix(&illustration(svg));
        let label = (*label).to_string();
        let filter = (*filter).to_string();
        let mpv = Arc::clone(mpv);
        row.connect_active_notify(move |row| {
            if row.is_active() {
                mpv.vf_append(&label, &filter);
            } else {
                mpv.vf_remove(&label);
            }
        });
        group.add(&row);
    }

    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "pixelize",
            svg: "pixelize.svg",
            title: i18n("Pixelize"),
            subtitle: i18n("Mosaic / pixel art effect"),
            spin_title: i18n("Block Size"),
            adjustment: (16.0, 2.0, 256.0, 2.0, 16.0),
            digits: 0,
            debounce_ms: 300,
            format_vf: pixelize_vf,
        },
    ));
    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "gblur",
            svg: "gblur.svg",
            title: i18n("Gaussian Blur"),
            subtitle: i18n("Smooth blur effect"),
            spin_title: i18n("Sigma"),
            adjustment: (5.0, 0.5, 50.0, 0.5, 5.0),
            digits: 1,
            debounce_ms: 300,
            format_vf: |v| format!("gblur=sigma={v:.1}"),
        },
    ));
    group.add(&build_vf_expander_row(
        mpv,
        current_vf,
        VfFilterSpec {
            label: "lagfun",
            svg: "motion_trail.svg",
            title: i18n("Motion Trail"),
            subtitle: i18n("Slowly fading visual trail effect"),
            spin_title: i18n("Decay"),
            adjustment: (0.95, 0.80, 0.99, 0.01, 0.05),
            digits: 2,
            debounce_ms: 300,
            format_vf: |v| format!("lagfun=decay={v:.2}"),
        },
    ));

    group
}
