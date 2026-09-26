//! Video page: picture quality, hardware decoding, deinterlacing, and the
//! fixes to try when the picture is missing or broken.

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::{add_combo_row, add_switch_row, live, live_str};

pub(super) fn page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    refresh_preset_rows: impl Fn() + 'static,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Video"))
        .icon_name("video-display-symbolic")
        .build();

    let picture = adw::PreferencesGroup::builder()
        .title(i18n("Picture"))
        .build();
    let preset_settings = settings.clone();
    let preset_mpv = mpv.cloned();
    add_combo_row(
        &picture,
        &i18n("Picture Quality"),
        &i18n("Light suits older computers; Detailed needs a stronger graphics card"),
        &[&i18n("Light"), &i18n("Balanced"), &i18n("Detailed")],
        &["default", "balanced", "high-quality"],
        keys::RENDER_PROFILE.0,
        settings,
        move |profile| {
            write_profile_details(&preset_settings, profile);
            if let Some(mpv) = &preset_mpv {
                mpv.apply_render_profile(profile);
            }
            // The preset rewrites settings shown on the Advanced page.
            refresh_preset_rows();
        },
    );
    add_combo_row(
        &picture,
        &i18n("Hardware Decoding"),
        &i18n("Lets the graphics card decode video, so the processor stays free"),
        &[
            &i18n("Automatic"),
            &i18n("Any method"),
            "VA-API (Intel/AMD)",
            "NVDEC (NVIDIA)",
            "CUDA (NVIDIA)",
            "VDPAU",
            &i18n("Off"),
        ],
        &["auto-safe", "auto", "vaapi", "nvdec", "cuda", "vdpau", "no"],
        keys::HWDEC.0,
        settings,
        live_str(mpv, MpvBackend::set_hwdec),
    );
    add_switch_row(
        &picture,
        &i18n("Remove Interlacing Lines"),
        &i18n("Fixes comb-like stripes in old TV recordings and DVDs"),
        "deinterlace",
        settings,
        live(mpv, MpvBackend::set_deinterlace),
    );
    page.add(&picture);

    let troubleshooting = adw::PreferencesGroup::builder()
        .title(i18n("If the Picture Has Problems"))
        .description(i18n(
            "Change these only if the video is black, corrupted or very slow. They take effect after reopening the player.",
        ))
        .build();
    add_combo_row(
        &troubleshooting,
        &i18n("Video Output"),
        &i18n("Compatibility mode works without a graphics card but uses more energy"),
        &[&i18n("Automatic"), "OpenGL", &i18n("Compatibility")],
        &["auto", "gl", "sw"],
        keys::RENDER_MODE.0,
        settings,
        |_| {},
    );
    add_switch_row(
        &troubleshooting,
        &i18n("Simple Rendering"),
        &i18n("Turns off effects and shaders for very old graphics cards"),
        keys::GPU_DUMB_MODE.0,
        settings,
        |_| {},
    );
    page.add(&troubleshooting);

    page
}

/// Store the per-scene HDR choice a picture-quality preset implies, so the
/// Advanced page and the next start agree with the preset.
fn write_profile_details(settings: &Settings, profile: &str) {
    let _ = settings.put_bool(keys::HDR_COMPUTE_PEAK, profile == "high-quality");
}
