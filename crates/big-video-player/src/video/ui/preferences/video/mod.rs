//! Video preferences page assembler: composes render-profile / decoding /
//! sync / hdr / gpu / icc groups in the order shown to the user.
//! Per-group advanced gating + simple-mode rows are tracked through the two
//! `AdvancedWidgets` registers passed in.

mod common;
mod decoding;
mod gpu;
mod hdr;
mod icc;
mod render_profile;
mod sync;

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

use super::AdvancedWidgets;

pub(super) fn create_video_page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
    simple_widgets: &AdvancedWidgets,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Video"))
        .icon_name("video-display-symbolic")
        .build();

    page.add(&render_profile::build_render_profile_group(
        settings,
        mpv,
        is_advanced,
        simple_widgets,
    ));
    page.add(&decoding::build_hw_decoding_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
        simple_widgets,
    ));
    page.add(&sync::build_video_sync_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&hdr::build_hdr_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&gpu::build_gpu_rendering_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));
    page.add(&icc::build_icc_group(
        settings,
        mpv,
        is_advanced,
        adv_widgets,
    ));

    page
}
