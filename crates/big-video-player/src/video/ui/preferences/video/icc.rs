//! ICC color-management group: auto-detect monitor profile, custom profile
//! path, rendering intent (perceptual / relative / saturation / absolute).
//! Whole group is advanced-gated and self-registers in `adv_widgets`.

use adw::prelude::*;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::layout::preferences::{
    BigPreferencesGroupSpec, tracked_preferences_group,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::super::{AdvancedWidgets, add_entry_row, add_switch_row};
use super::illustration_assets::illustration;

pub(super) fn build_icc_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let icc_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Color Management (ICC)"))
            .description(i18n(
                "Use your monitor’s color profile for accurate color reproduction",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    let m = mpv.cloned();
    let icc_auto_row = add_switch_row(
        &icc_group,
        &i18n("Auto-detect ICC Profile"),
        &i18n("Use your monitor color profile for accurate color reproduction"),
        "icc-profile-auto",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_icc_profile_auto(v);
            }
        },
    );
    icc_auto_row.add_prefix(&illustration("icc_profile.svg"));

    let m = mpv.cloned();
    let icc_path_row = add_entry_row(
        &icc_group,
        &i18n("ICC Profile Path"),
        &i18n("Path to a custom ICC/ICM color profile file"),
        "icc-profile",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_icc_profile(v);
            }
        },
    );
    icc_path_row.add_prefix(&illustration("folder.svg"));

    let (icc_intent_row, icc_intent_dropdown) = BigDropdownRow::new(BigDropdownRowSpec::new(
        i18n("Rendering Intent"),
        [
            i18n("Perceptual"),
            i18n("Relative"),
            i18n("Saturation"),
            i18n("Absolute"),
        ],
    ))
    .into_parts();
    icc_intent_dropdown.set_selected((settings.get_int(keys::ICC_INTENT) as u32).min(3));
    icc_intent_row.add_prefix(&illustration("icc_profile.svg"));
    {
        let s = settings.clone();
        let m = mpv.cloned();
        icc_intent_dropdown.connect_selected_notify(move |dropdown| {
            let val = dropdown.selected() as i32;
            let _ = s.put_int(keys::ICC_INTENT, val);
            if let Some(ref mpv) = m {
                mpv.set_icc_intent(i64::from(val));
            }
        });
    }
    icc_group.add(&icc_intent_row);

    icc_group
}
