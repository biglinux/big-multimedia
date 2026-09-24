//! Subtitles position + rendering preferences group.
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
use super::super::{add_combo_row, add_spin_int_row, add_switch_row};

use crate::shared::illustration::illustration_auto as illustration;

pub(in crate::video::ui::preferences) fn build_position_rendering_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Position and Rendering"))
            .description(i18n("Control where and how subtitles are drawn on screen"))
            .visible(is_advanced),
        adv_widgets,
    );

    let m = mpv.cloned();
    let margin_row = add_spin_int_row(
        &group,
        &i18n("Bottom Margin"),
        &i18n("Distance from the bottom of the window in pixels"),
        "sub-margin-y",
        0.0,
        200.0,
        1.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_margin_y(v);
            }
        },
    );
    margin_row.add_prefix(&illustration("sub_position.svg"));

    let border_style_labels = &[
        i18n("Outline and Shadow"),
        i18n("Opaque Box"),
        i18n("Background Box"),
    ];
    let m = mpv.cloned();
    let border_style_row = add_combo_row(
        &group,
        &i18n("Border Style"),
        &i18n("How the outline and background around subtitle text are drawn"),
        &border_style_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["outline-and-shadow", "opaque-box", "background-box"],
        "sub-border-style",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_border_style(v);
            }
        },
    );
    border_style_row.add_prefix(&illustration("sub_position.svg"));

    let m = mpv.cloned();
    let scale_row = add_switch_row(
        &group,
        &i18n("Scale with Window"),
        &i18n("Subtitle size follows window size"),
        "sub-scale-with-window",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_scale_with_window(v);
            }
        },
    );
    scale_row.add_prefix(&illustration("scale_window.svg"));

    let m = mpv.cloned();
    let ass_scale_row = add_switch_row(
        &group,
        &i18n("Scale Styled Subtitles"),
        &i18n("Also resize ASS/SSA subtitles (common in anime) when the window changes size"),
        "sub-ass-scale-with-window",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_sub_ass_scale_with_window(v);
            }
        },
    );
    ass_scale_row.add_prefix(&illustration("scale_window.svg"));

    group
}
