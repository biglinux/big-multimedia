//! HDR / tone-mapping preferences group: tone-mapping algorithm, display peak
//! / contrast, contrast-recovery, per-scene peak, colorspace hint, gamut
//! mapping. Whole group is gated by `is_advanced` and self-registers in
//! `adv_widgets` for the basic/advanced toggle.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    tracked_preferences_group, BigPreferencesGroupSpec,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::{
    add_combo_row, add_spin_double_row, add_spin_int_row, add_switch_row, AdvancedWidgets,
};
use super::illustration_assets::illustration;

pub(super) fn build_hdr_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let hdr_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("HDR / Tone Mapping"))
            .description(i18n("Adjust how HDR content is displayed on your screen"))
            .visible(is_advanced),
        adv_widgets,
    );

    let tonemapping_labels = &[
        i18n("Automatic"),
        "Möbius".to_string(),
        "Reinhard".to_string(),
        "Hable".to_string(),
        "BT.2390".to_string(),
        i18n("Clip (no mapping)"),
    ];
    let m = mpv.cloned();
    let tonemapping_row = add_combo_row(
        &hdr_group,
        &i18n("Tone Mapping Algorithm"),
        &i18n(
            "How HDR brightness is mapped to your screen — Automatic works well for most displays",
        ),
        &tonemapping_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["auto", "mobius", "reinhard", "hable", "bt.2390", "clip"],
        "tone-mapping",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_tone_mapping(v);
            }
        },
    );
    tonemapping_row.add_prefix(&illustration("hdr.svg"));

    let m = mpv.cloned();
    let peak_row = add_spin_int_row(
        &hdr_group,
        &i18n("Display Peak Brightness"),
        &i18n("Target peak in nits (0 = auto)"),
        "target-peak",
        0.0,
        10000.0,
        10.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_target_peak(v);
            }
        },
    );
    peak_row.add_prefix(&illustration("brightness.svg"));

    let m = mpv.cloned();
    let contrast_row = add_spin_int_row(
        &hdr_group,
        &i18n("Display Contrast"),
        &i18n("Contrast ratio (0 = infinite)"),
        "target-contrast",
        0.0,
        1_000_000.0,
        100.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_target_contrast(v);
            }
        },
    );
    contrast_row.add_prefix(&illustration("brightness.svg"));

    let m = mpv.cloned();
    let recovery_row = add_spin_double_row(
        &hdr_group,
        &i18n("HDR Contrast Recovery"),
        &i18n("Recovery strength (0.0-2.0)"),
        "hdr-contrast-recovery",
        0.0,
        2.0,
        0.1,
        1,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_hdr_contrast_recovery(v);
            }
        },
    );
    recovery_row.add_prefix(&illustration("brightness.svg"));

    let m = mpv.cloned();
    let compute_peak_row = add_switch_row(
        &hdr_group,
        &i18n("Compute HDR Peak Per Scene"),
        &i18n("Dynamically adjust brightness based on scene content for better HDR rendering"),
        "hdr-compute-peak",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_hdr_compute_peak(v);
            }
        },
    );
    compute_peak_row.add_prefix(&illustration("compute_peak.svg"));

    let m = mpv.cloned();
    let cs_hint_row = add_switch_row(
        &hdr_group,
        &i18n("Colorspace Hint"),
        &i18n("Send HDR colorspace hint to display"),
        "target-colorspace-hint",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_target_colorspace_hint(v);
            }
        },
    );
    cs_hint_row.add_prefix(&illustration("colorspace.svg"));

    let gamut_labels = &[
        i18n("Automatic"),
        i18n("Clip"),
        i18n("Perceptual"),
        i18n("Relative"),
        i18n("Saturation"),
        i18n("Absolute"),
        i18n("Desaturate"),
        i18n("Darken"),
        i18n("Warn"),
        i18n("Linear"),
    ];
    let m = mpv.cloned();
    let gamut_row = add_combo_row(
        &hdr_group,
        &i18n("Gamut Mapping Mode"),
        &i18n("How colors outside your display range are handled"),
        &gamut_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &[
            "auto",
            "clip",
            "perceptual",
            "relative",
            "saturation",
            "absolute",
            "desaturate",
            "darken",
            "warn",
            "linear",
        ],
        "gamut-mapping-mode",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_gamut_mapping_mode(v);
            }
        },
    );
    gamut_row.add_prefix(&illustration("colorspace.svg"));

    hdr_group
}
