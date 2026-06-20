//! Advanced preferences groups: cache/buffer (live-applied via mpv), screensaver
//! inhibitor mode, GPU dumb-mode (restart required), and free-form custom mpv
//! options. Groups self-register via `adv_widgets` so the basic/advanced toggle
//! can hide them in one shot.

use adw::prelude::*;
use big_relm4_components::layout::preferences::{
    tracked_preferences_group, BigPreferencesGroupSpec,
};
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::super::AdvancedWidgets;
use super::super::{
    add_combo_row, add_entry_row, add_spin_double_row, add_spin_int_row, add_switch_row,
};
use super::common::illustration;

pub(super) fn build_adv_cache_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let cache_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Cache and Buffer"))
            .description(i18n(
                "Buffer media data to reduce stuttering on slow or remote connections",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    let m = mpv.cloned();
    let cache_row = add_switch_row(
        &cache_group,
        &i18n("Enable Cache"),
        &i18n("Buffer media data to reduce stuttering on slow connections"),
        "cache",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_cache(v);
            }
        },
    );
    cache_row.add_prefix(&illustration("cache.svg"));

    let m = mpv.cloned();
    let cache_secs_row = add_spin_int_row(
        &cache_group,
        &i18n("Cache Duration"),
        &i18n("Buffer length in seconds"),
        "cache-secs",
        1.0,
        600.0,
        10.0,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_cache_secs(v);
            }
        },
    );
    cache_secs_row.add_prefix(&illustration("timer.svg"));

    let m = mpv.cloned();
    let cache_pause_row = add_switch_row(
        &cache_group,
        &i18n("Pause When Buffer Low"),
        &i18n("Pause playback while buffering catches up instead of stuttering"),
        "cache-pause",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_cache_pause(v);
            }
        },
    );
    cache_pause_row.add_prefix(&illustration("pause.svg"));

    let m = mpv.cloned();
    let disk_row = add_switch_row(
        &cache_group,
        &i18n("Cache on Disk"),
        &i18n("Store cache on disk instead of RAM — useful for very long streams"),
        "cache-on-disk",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_cache_on_disk(v);
            }
        },
    );
    disk_row.add_prefix(&illustration("disk.svg"));

    let m = mpv.cloned();
    let readahead_row = add_spin_double_row(
        &cache_group,
        &i18n("Demuxer Readahead"),
        &i18n("Readahead in seconds"),
        "demuxer-readahead-secs",
        0.0,
        60.0,
        0.5,
        1,
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_demuxer_readahead_secs(v);
            }
        },
    );
    readahead_row.add_prefix(&illustration("timer.svg"));

    cache_group
}

pub(super) fn build_adv_screensaver_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let ss_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Screensaver"))
            .description(i18n(
                "Prevent the screen from going dark while watching videos",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    let screensaver_labels = &[
        i18n("During playback"),
        i18n("Never block"),
        i18n("Always block"),
    ];
    let m = mpv.cloned();
    let screensaver_row = add_combo_row(
        &ss_group,
        &i18n("Block Screensaver"),
        &i18n("Prevent the screen from dimming while watching videos"),
        &screensaver_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["yes", "no", "always"],
        "stop-screensaver",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_stop_screensaver(v);
            }
        },
    );
    screensaver_row.add_prefix(&illustration("screensaver.svg"));

    ss_group
}

pub(super) fn build_adv_gpu_group(
    settings: &Settings,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let gpu_group = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("GPU"))
            .description(i18n(
                "Low-level GPU options for troubleshooting rendering issues",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    add_switch_row(
        &gpu_group,
        &i18n("GPU Dumb Mode"),
        &i18n("Disable FBOs and shaders for low-end GPUs (requires restart)"),
        "gpu-dumb-mode",
        settings,
        |_| {},
    )
    .add_prefix(&illustration("gpu.svg"));

    gpu_group
}

pub(super) fn build_adv_custom_group(
    settings: &Settings,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let custom = tracked_preferences_group(
        BigPreferencesGroupSpec::new(i18n("Custom mpv Options"))
            .description(i18n(
                "Extra mpv options applied on next startup — comma-separated key=value pairs",
            ))
            .visible(is_advanced),
        adv_widgets,
    );

    add_entry_row(
        &custom,
        &i18n("Options"),
        "",
        "custom-mpv-options",
        settings,
        |_| {},
    )
    .add_prefix(&illustration("terminal.svg"));

    custom
}
