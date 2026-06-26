//! Hardware decoding preferences group: detailed combo (advanced) vs. simple
//! on/off switch — they share the `hwdec` setting key and toggle visibility
//! based on `is_advanced`. Plus deinterlace (always visible).

use adw::prelude::*;
use big_relm4_components::layout::preferences::{track_widget_visibility, BigPreferencesGroupSpec};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{keys, Settings};

use super::super::{add_combo_row, add_switch_row, AdvancedWidgets};
use super::illustration_assets::illustration;

pub(super) fn build_hw_decoding_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
    simple_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let hw_group = BigPreferencesGroupSpec::new(i18n("Hardware Decoding"))
        .description(i18n(
            "Let your graphics card handle video decoding — smoother playback and lower CPU usage",
        ))
        .build();

    let hwdec_labels = &[
        i18n("Automatic (safe)"),
        i18n("Automatic"),
        "VA-API (Intel/AMD)".to_string(),
        "NVDEC (NVIDIA)".to_string(),
        "CUDA (NVIDIA)".to_string(),
        "VDPAU (legacy)".to_string(),
        i18n("Disabled"),
    ];
    let m = mpv.cloned();
    let hwdec_combo = add_combo_row(
        &hw_group,
        &i18n("Hardware acceleration"),
        &i18n("Uses the GPU to decode video, reducing CPU usage. Auto-safe is recommended."),
        &hwdec_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["auto-safe", "auto", "vaapi", "nvdec", "cuda", "vdpau", "no"],
        "hwdec",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_hwdec(v);
            }
        },
    );
    hwdec_combo.add_prefix(&illustration("hw_decoding.svg"));
    track_widget_visibility(&hwdec_combo, is_advanced, adv_widgets);

    let hwdec_simple_switch = adw::SwitchRow::builder()
        .title(i18n("Hardware acceleration"))
        .subtitle(i18n(
            "Recommended ON. Uses your GPU for smooth playback with less CPU usage",
        ))
        .active(settings.get_string(keys::HWDEC) != "no")
        .build();
    {
        let s = settings.clone();
        let m = mpv.cloned();
        hwdec_simple_switch.connect_active_notify(move |r| {
            let val = if r.is_active() { "auto-safe" } else { "no" };
            let _ = s.put_string(keys::HWDEC, val);
            if let Some(ref mpv) = m {
                mpv.set_hwdec(val);
            }
        });
    }
    hw_group.add(&hwdec_simple_switch);
    hwdec_simple_switch.add_prefix(&illustration("hw_decoding.svg"));
    track_widget_visibility(&hwdec_simple_switch, !is_advanced, simple_widgets);

    let m = mpv.cloned();
    let deinterlace_row = add_switch_row(
        &hw_group,
        &i18n("Fix interlaced video"),
        &i18n("Removes visible lines on old TV recordings and DVDs"),
        "deinterlace",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_deinterlace(v);
            }
        },
    );
    deinterlace_row.add_prefix(&illustration("deinterlace.svg"));

    hw_group
}
