//! Always-visible picture quality summary, with resource-cost guidance.
//! Preset changes are persisted and applied to the current backend. Individual
//! technical rows are still rebuilt from settings when preferences reopen.

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
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
/// default Automatic. Requires recreating the player content (normally by
/// closing and reopening the player), not merely loading another file.
pub(super) fn build_video_output_group(settings: &Settings) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Video Output"))
        .description(i18n(
            "Keep Automatic unless the picture is missing or corrupted. Compatibility mode uses the processor and may use more energy.",
        ))
        .build();

    let values: &[&str] = &["auto", "gl", "sw"];

    let (row, dropdown) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Video Output"),
            [
                i18n("Automatic (recommended)"),
                i18n("OpenGL with fallback"),
                i18n("Compatibility (processor)"),
            ],
        )
        .subtitle(i18n("Close and reopen the player to apply this change")),
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
    _is_advanced: bool,
    _simple_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    // Keep the summary visible when technical details are revealed, so the
    // user's chosen preset does not disappear when switching modes.
    let group = BigPreferencesGroupSpec::new(i18n("Picture quality"))
        .description(i18n(
            "Use Low resource use on older computers. Choose more detail only when playback stays smooth.",
        ))
        .build();

    let profile_values: &[&str] = &["default", "balanced", "high-quality"];

    let (profile_row, profile_dropdown) = BigDropdownRow::new(
        BigDropdownRowSpec::new(
            i18n("Picture quality"),
            [
                i18n("Low resource use"),
                i18n("Balanced"),
                i18n("More detail"),
            ],
        )
        .subtitle(i18n(
            "More detail can increase graphics workload and battery use",
        )),
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
