//! Advanced page: technical mpv options for people who know what they need.
//! The defaults suit almost everyone, and the page says so up front.

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::{
    add_combo_row, add_entry_row, add_spin_double_row, add_spin_int_row, add_switch_row, live,
    live_str,
};

/// Build the page. The returned closure re-reads the per-scene HDR row, which a
/// picture-quality preset rewrites, from the settings.
pub(super) fn page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
) -> (adw::PreferencesPage, impl Fn() + 'static) {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Advanced"))
        .icon_name("preferences-other-symbolic")
        .description(i18n(
            "Technical options for experienced users. The defaults work well for most videos and computers.",
        ))
        .build();

    // No motion-smoothing options: display-synced timing and interpolation
    // need the display rate, which mpv's embedded output never learns.
    let (hdr, scene_row) = hdr_group(settings, mpv);
    page.add(&hdr);
    page.add(&color_group(settings, mpv));
    page.add(&cache_group(settings, mpv));
    page.add(&extras_group(settings, mpv));
    page.add(&custom_group(settings, mpv));
    page.add(&mpv_options_group(settings));

    let refresh = {
        let settings = settings.clone();
        let scene_row = scene_row.downgrade();
        move || {
            if let Some(row) = scene_row.upgrade() {
                row.set_active(settings.get_bool(keys::HDR_COMPUTE_PEAK));
            }
        }
    };
    (page, refresh)
}

fn hdr_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
) -> (adw::PreferencesGroup, adw::SwitchRow) {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("HDR Video"))
        .description(i18n("How bright HDR video is fitted to your screen"))
        .build();
    add_combo_row(
        &group,
        &i18n("Tone Mapping"),
        "",
        &[
            &i18n("Automatic"),
            "Möbius",
            "Reinhard",
            "Hable",
            "BT.2390",
            &i18n("Clip"),
        ],
        &["auto", "mobius", "reinhard", "hable", "bt.2390", "clip"],
        "tone-mapping",
        settings,
        live_str(mpv, MpvBackend::set_tone_mapping),
    );
    let scene_row = add_switch_row(
        &group,
        &i18n("Adapt to Each Scene"),
        &i18n("Measures the brightness of every scene; uses more graphics power"),
        keys::HDR_COMPUTE_PEAK.0,
        settings,
        live(mpv, MpvBackend::set_hdr_compute_peak),
    );
    add_spin_int_row(
        &group,
        &i18n("Screen Peak Brightness"),
        &i18n("In nits; 0 detects it automatically"),
        "target-peak",
        0.0,
        10000.0,
        10.0,
        settings,
        live(mpv, MpvBackend::set_target_peak),
    );
    add_combo_row(
        &group,
        &i18n("Out-of-Range Colors"),
        &i18n("How colors your screen cannot show are handled"),
        &[
            &i18n("Automatic"),
            &i18n("Clip"),
            &i18n("Desaturate"),
            &i18n("Highlight"),
        ],
        // vo=gpu, which the embedded render API uses, accepts only these.
        &["auto", "clip", "desaturate", "warn"],
        "gamut-mapping-mode",
        settings,
        live_str(mpv, MpvBackend::set_gamut_mapping_mode),
    );
    (group, scene_row)
}

fn color_group(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Color Management"))
        .description(i18n(
            "Uses a monitor color profile (ICC) for accurate colors. Enter the path to an .icc or .icm file and press Enter; leave it empty to turn this off.",
        ))
        .build();
    add_applied_entry_row(
        &group,
        &i18n("Profile File"),
        "icc-profile",
        settings,
        live_str(mpv, MpvBackend::set_icc_profile),
    );
    let intent_mpv = mpv.cloned();
    let intent_settings = settings.clone();
    big_media_components::preference_rows::combo_row(
        &group,
        &i18n("Rendering Intent"),
        "",
        &[
            &i18n("Perceptual"),
            &i18n("Relative"),
            &i18n("Saturation"),
            &i18n("Absolute"),
        ],
        settings.get_int(keys::ICC_INTENT).clamp(0, 3) as u32,
        move |intent| {
            let intent = intent as i32;
            let _ = intent_settings.put_int(keys::ICC_INTENT, intent);
            if let Some(mpv) = &intent_mpv {
                mpv.set_icc_intent(i64::from(intent));
            }
        },
    );
    group
}

fn cache_group(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Streaming and Cache"))
        .description(i18n(
            "Loads media ahead of time to avoid pauses on slow networks",
        ))
        .build();
    let cache_row = add_switch_row(
        &group,
        &i18n("Load Ahead"),
        "",
        "cache",
        settings,
        live(mpv, MpvBackend::set_cache),
    );
    let seconds_row = add_spin_int_row(
        &group,
        &i18n("How Far Ahead"),
        &i18n("In seconds"),
        "cache-secs",
        1.0,
        600.0,
        10.0,
        settings,
        live(mpv, MpvBackend::set_cache_secs),
    );
    let pause_row = add_switch_row(
        &group,
        &i18n("Wait Instead of Stuttering"),
        &i18n("Pauses until enough is loaded to keep playing smoothly"),
        "cache-pause",
        settings,
        live(mpv, MpvBackend::set_cache_pause),
    );
    let disk_row = add_switch_row(
        &group,
        &i18n("Keep on Disk"),
        &i18n("Uses disk space instead of memory; useful for very long streams"),
        "cache-on-disk",
        settings,
        live(mpv, MpvBackend::set_cache_on_disk),
    );
    for row in [
        seconds_row.upcast_ref::<gtk::Widget>(),
        pause_row.upcast_ref(),
        disk_row.upcast_ref(),
    ] {
        cache_row
            .bind_property("active", row, "sensitive")
            .sync_create()
            .build();
    }
    add_spin_double_row(
        &group,
        &i18n("Read Ahead in the File"),
        &i18n("Seconds of the file read in advance, also for local files"),
        "demuxer-readahead-secs",
        0.0,
        60.0,
        0.5,
        1,
        settings,
        live(mpv, MpvBackend::set_demuxer_readahead_secs),
    );
    group
}

fn extras_group(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Other"))
        .build();
    add_switch_row(
        &group,
        &i18n("Pause on the Final Frame"),
        &i18n("When the player stops at the end of a video; off lets seeking back resume playback"),
        "keep-open-pause",
        settings,
        live(mpv, MpvBackend::set_keep_open_pause),
    );
    add_spin_int_row(
        &group,
        &i18n("Screenshot Compression"),
        &i18n("Higher values make smaller PNG files but take longer to save"),
        "screenshot-png-compression",
        0.0,
        9.0,
        1.0,
        settings,
        live(mpv, MpvBackend::set_screenshot_png_compression),
    );
    add_switch_row(
        &group,
        &i18n("Screenshot Color Profile"),
        &i18n("Stores the video color space in each screenshot"),
        "screenshot-tag-colorspace",
        settings,
        live(mpv, MpvBackend::set_screenshot_tag_colorspace),
    );
    add_switch_row(
        &group,
        &i18n("High Bit Depth Screenshots"),
        &i18n("Keeps more color precision; files get larger"),
        "screenshot-high-bit-depth",
        settings,
        live(mpv, MpvBackend::set_screenshot_high_bit_depth),
    );
    group
}

fn custom_group(settings: &Settings, mpv: Option<&Arc<MpvBackend>>) -> adw::PreferencesGroup {
    let shaders = adw::PreferencesGroup::builder()
        .title(i18n("Custom Shaders"))
        .description(i18n(
            "Paths to .glsl files, separated by commas. Press Enter to apply.",
        ))
        .build();
    add_applied_entry_row(&shaders, &i18n("Shader Files"), "glsl-shaders", settings, {
        // Replacing the list drops the dialog's upscalers; put them back.
        let mpv = mpv.cloned();
        let settings = settings.clone();
        move |shaders: &str| {
            if let Some(mpv) = &mpv {
                mpv.set_glsl_shaders(shaders);
                crate::video::ui::filters::apply_saved_enhancements(mpv, &settings);
            }
        }
    });
    shaders
}

fn mpv_options_group(settings: &Settings) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("mpv Options"))
        .description(i18n(
            "Extra key=value options, separated by commas. They take effect after reopening the player.",
        ))
        .build();
    add_entry_row(
        &group,
        &i18n("Options"),
        "",
        keys::CUSTOM_MPV_OPTIONS.0,
        settings,
        |_| {},
    );
    group
}

/// Entry saved on every edit but handed to the player only on Enter or the
/// apply button, so half-typed file paths never reach mpv.
fn add_applied_entry_row(
    group: &adw::PreferencesGroup,
    title: &str,
    key: &'static str,
    settings: &Settings,
    apply: impl Fn(&str) + 'static,
) -> adw::EntryRow {
    let row = add_entry_row(group, title, "", key, settings, |_| {});
    row.set_show_apply_button(true);
    row.connect_apply(move |row| apply(&row.text()));
    row
}
