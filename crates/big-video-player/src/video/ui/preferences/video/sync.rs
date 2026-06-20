//! Video sync preferences group: video-sync mode, interpolation toggle,
//! tscale filter. Whole group is advanced-gated and self-registers in
//! `adv_widgets`.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    tracked_preferences_group, BigPreferencesGroupSpec,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::{add_combo_row, add_switch_row, AdvancedWidgets};
use super::common::illustration;

pub(super) fn build_video_sync_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let sync_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Synchronization"))
            .description(i18n(
                "How video frames are timed to your display refresh rate",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    let vsync_labels = &[
        i18n("Follow audio (default)"),
        i18n("Match display rate"),
        i18n("Match display (drop frames)"),
        i18n("Display free-run"),
    ];
    let m = mpv.cloned();
    let vsync_row = add_combo_row(
        &sync_group,
        &i18n("Video Sync"),
        &i18n("How video frames are timed relative to your display"),
        &vsync_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &[
            "audio",
            "display-resample",
            "display-resample-vdrop",
            "display-desync",
        ],
        "video-sync",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_video_sync(v);
            }
        },
    );
    vsync_row.add_prefix(&illustration("sync.svg"));

    let m = mpv.cloned();
    let interp_row = add_switch_row(
        &sync_group,
        &i18n("Interpolation"),
        &i18n("Smooth motion — requires a display sync mode above"),
        "interpolation",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_interpolation(v);
            }
        },
    );
    interp_row.add_prefix(&illustration("interpolation.svg"));

    let m = mpv.cloned();
    let tscale_row = add_combo_row(
        &sync_group,
        &i18n("Interpolation Filter"),
        &i18n("Algorithm used for motion smoothing"),
        &[
            "Mitchell",
            "Catmull-Rom",
            "Oversample",
            "Linear",
            "Gaussian",
            "Bicubic",
        ],
        &[
            "mitchell",
            "catmull_rom",
            "oversample",
            "linear",
            "gaussian",
            "bicubic",
        ],
        "tscale",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_tscale(v);
            }
        },
    );
    tscale_row.add_prefix(&illustration("interpolation.svg"));

    sync_group
}
