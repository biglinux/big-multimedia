//! Subtitles "special formats" preferences group (ASS, bitmap, etc).
//!
//! Split out of `subtitles_groups/mod.rs` to keep that module under the
//! 400-LOC budget.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    BigPreferencesGroupSpec, tracked_preferences_group,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::AdvancedWidgets;
use super::super::{add_spin_double_row, add_switch_row};

use crate::shared::ui_helpers::illustration_auto as illustration;

pub(in crate::video::ui::preferences) fn build_special_formats_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Special Formats"))
            .description(i18n(
                "Options for SDH annotations, DVD/Blu-ray image subtitles and dual subtitles",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    let m = mpv.cloned();
    let sdh_row = add_switch_row(
        &group,
        &i18n("Filter SDH Annotations"),
        &i18n("Remove [music], [applause] and other sound descriptions from subtitles"),
        "sub-filter-sdh",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_filter_sdh(v);
            }
        },
    );
    sdh_row.add_prefix(&illustration("sdh_filter.svg"));

    let m = mpv.cloned();
    let sdh_harder_row = add_switch_row(
        &group,
        &i18n("Aggressive SDH Filtering"),
        &i18n("More aggressive removal of SDH content"),
        "sub-filter-sdh-harder",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_filter_sdh_harder(v);
            }
        },
    );
    sdh_harder_row.add_prefix(&illustration("sdh_filter.svg"));

    let m = mpv.cloned();
    let gauss_row = add_spin_double_row(
        &group,
        &i18n("Blur Image Subtitles"),
        &i18n("Smooth pixelated DVD/Blu-ray subtitles"),
        "sub-gauss",
        0.0,
        5.0,
        0.1,
        1,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_gauss(v);
            }
        },
    );
    gauss_row.add_prefix(&illustration("image_subs.svg"));

    let m = mpv.cloned();
    let gray_row = add_switch_row(
        &group,
        &i18n("Grayscale Image Subtitles"),
        &i18n("Convert bitmap subtitles to grayscale"),
        "sub-gray",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_gray(v);
            }
        },
    );
    gray_row.add_prefix(&illustration("image_subs.svg"));

    add_switch_row(
        &group,
        &i18n("Enable Secondary Subtitle"),
        &i18n("Show a second subtitle track alongside the primary one"),
        "secondary-subtitles",
        settings,
        |_| {},
    )
    .add_prefix(&illustration("dual_subs.svg"));

    group
}
