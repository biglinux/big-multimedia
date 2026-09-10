//! Simple-mode "Rendering Quality" group: combo of Fast / Balanced / High
//! Quality presets. Selecting a preset writes individual settings keys
//! (sync / interpolation / tscale / compute-peak) so the advanced UI stays
//! in sync, then calls `apply_render_profile` for keys without settings
//! mirror. Group is gated by `!is_advanced` (advanced view shows the
//! per-feature rows directly).

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::{
    BigPreferencesGroupSpec, tracked_preferences_group,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::super::AdvancedWidgets;
use super::illustration_assets::illustration;

fn sync_profile_to_settings(settings: &Settings, profile: &str) {
    if profile == "high-quality" {
        let _ = settings.put_string(keys::VIDEO_SYNC, "display-resample");
        let _ = settings.put_bool(keys::INTERPOLATION, true);
        let _ = settings.put_string(keys::TSCALE, "oversample");
        let _ = settings.put_bool(keys::HDR_COMPUTE_PEAK, true);
    } else {
        let _ = settings.put_string(keys::VIDEO_SYNC, "audio");
        let _ = settings.put_bool(keys::INTERPOLATION, false);
        let _ = settings.put_bool(keys::HDR_COMPUTE_PEAK, false);
    }
}

/// Always-visible "Video Output" group: how frames are rendered — Automatic
/// (GPU when viable, software fallback), OpenGL (force GPU/offload), or Software
/// (force the no-GL path; plays without a GPU). Bound to `keys::RENDER_MODE`,
/// default Automatic. Takes effect on newly opened videos (the render path is
/// wired when the content realizes).
pub(super) fn build_video_output_group(settings: &Settings) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Video Output"))
        .description(i18n(
            "How video is rendered. Automatic uses the GPU when available and falls back to software.",
        ))
        .build();

    let values: &[&str] = &["auto", "gl", "sw"];

    let (row, dropdown) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Video Output"),
            [i18n("Automatic"), i18n("OpenGL (GPU)"), i18n("Software")],
        )
        .subtitle(i18n(
            "Software works without a GPU; applies to newly opened videos",
        )),
    )
    .into_parts();
    row.add_prefix(&illustration("render_profile.svg"));

    let current = settings.get_string(keys::RENDER_MODE);
    let current = if current.is_empty() {
        "auto"
    } else {
        current.as_str()
    };
    let idx = values.iter().position(|v| *v == current).unwrap_or(0);
    dropdown.set_selected(idx as u32);

    {
        let s = settings.clone();
        dropdown.connect_selected_notify(move |dropdown| {
            if let Some(&val) = values.get(dropdown.selected() as usize) {
                let _ = s.put_string(keys::RENDER_MODE, val);
            }
        });
    }

    group.add(&row);
    group
}

pub(super) fn build_render_profile_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    simple_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Rendering Quality"))
            .description(i18n(
                "Start with Balanced. Switch to Fast if you notice stuttering, or High Quality for movies.",
            ))
            .visible(!is_advanced),
        simple_widgets,
    );

    let profile_values: &[&str] = &["default", "balanced", "high-quality"];

    let (profile_row, profile_dropdown) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Video Profile"),
            [i18n("⚡ Fast"), i18n("⚖ Balanced"), i18n("✨ High Quality")],
        )
        .subtitle(i18n("Higher quality uses more GPU power")),
    )
    .into_parts();
    profile_row.add_prefix(&illustration("render_profile.svg"));

    let current = settings.get_string(keys::RENDER_PROFILE);
    let idx = profile_values
        .iter()
        .position(|v| *v == current.as_str())
        .unwrap_or(0);
    profile_dropdown.set_selected(idx as u32);

    {
        let s = settings.clone();
        let m = mpv.cloned();
        profile_dropdown.connect_selected_notify(move |dropdown| {
            let selected = dropdown.selected() as usize;
            if let Some(&val) = profile_values.get(selected) {
                let _ = s.put_string(keys::RENDER_PROFILE, val);
                sync_profile_to_settings(&s, val);
                if let Some(ref mpv) = m {
                    mpv.apply_render_profile(val);
                }
            }
        });
    }

    group.add(&profile_row);
    group
}
