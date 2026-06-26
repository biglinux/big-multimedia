//! GPU / rendering preferences group: custom GLSL shader path list,
//! auto-resize window on resolution change, seekbar thumbnail preview.
//! Whole group is advanced-gated and self-registers in `adv_widgets`.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    tracked_preferences_group, BigPreferencesGroupSpec,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::{add_entry_row, add_switch_row, AdvancedWidgets};
use super::illustration_assets::illustration;

pub(super) fn build_gpu_rendering_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let gpu_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("GPU / Rendering"))
            .description(i18n("Custom shaders, window behavior and video preview"))
            .visible(is_advanced),
        adv_widgets,
    );

    let m = mpv.cloned();
    let shader_row = add_entry_row(
        &gpu_group,
        &i18n("GLSL Shaders"),
        &i18n("Paths to .glsl files separated by commas — e.g. /path/to/anime4k.glsl"),
        "glsl-shaders",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_glsl_shaders(v);
            }
        },
    );
    shader_row.add_prefix(&illustration("shaders.svg"));

    let m = mpv.cloned();
    let autoresize_row = add_switch_row(
        &gpu_group,
        &i18n("Auto-resize Window"),
        &i18n("Adjust window when video resolution changes"),
        "auto-window-resize",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_auto_window_resize(v);
            }
        },
    );
    autoresize_row.add_prefix(&illustration("window_resize.svg"));

    add_switch_row(
        &gpu_group,
        &i18n("Seekbar Thumbnail Preview"),
        &i18n("Show video frame preview on seekbar hover"),
        "seekbar-thumbnail",
        settings,
        |_| {},
    )
    .add_prefix(&illustration("seekbar_preview.svg"));

    gpu_group
}
