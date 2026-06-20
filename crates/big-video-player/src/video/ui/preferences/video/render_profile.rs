//! Simple-mode "Rendering Quality" group: combo of Fast / Balanced / High
//! Quality presets. Selecting a preset writes individual settings keys
//! (sync / interpolation / tscale / compute-peak) so the advanced UI stays
//! in sync, then calls `apply_render_profile` for keys without settings
//! mirror. Group is gated by `!is_advanced` (advanced view shows the
//! per-feature rows directly).

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    tracked_preferences_group, BigPreferencesGroupSpec,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{keys, Settings};

use super::super::AdvancedWidgets;
use super::common::illustration;

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

    let profile_labels = &[i18n("⚡ Fast"), i18n("⚖ Balanced"), i18n("✨ High Quality")];
    let profile_options = gtk::StringList::new(
        &profile_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
    );
    let profile_values: &[&str] = &["default", "balanced", "high-quality"];

    let profile_row = adw::ComboRow::builder()
        .title(i18n("Video Profile"))
        .subtitle(i18n("Higher quality uses more GPU power"))
        .model(&profile_options)
        .build();
    profile_row.add_prefix(&illustration("render_profile.svg"));

    let current = settings.get_string(keys::RENDER_PROFILE);
    let idx = profile_values
        .iter()
        .position(|v| *v == current.as_str())
        .unwrap_or(0);
    profile_row.set_selected(idx as u32);

    {
        let s = settings.clone();
        let m = mpv.cloned();
        profile_row.connect_selected_notify(move |row| {
            let selected = row.selected() as usize;
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
