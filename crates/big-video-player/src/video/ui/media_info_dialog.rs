//! App-local media-information dialog (video port, V5).
//!
//! Reads the live mpv backend (general / metadata / video / audio / streaming /
//! track sections) into copyable rows, with a "Copy All" header button. A free
//! function rather than a Relm4 component: it builds and presents a transient
//! `adw::Window` from a one-shot snapshot of mpv state and owns no further
//! lifecycle (the shell calls it from a header/menu action).
//!
//! Reuse note: the shared `big_media_components::media::file_info_dialog`
//! consumes an ffprobe JSON model, whereas the player wants the *live* mpv view
//! (hwdec, cache %, selected tracks). Those differ, so this dialog stays
//! app-local but reuses the shared copyable-row + copy-all widgets and the
//! shared byte/time formatting helpers, not bespoke ones.

use std::sync::Arc;

use adw::prelude::*;
use big_media_components::bytes::decimal_detailed;
use big_media_components::time::duration_hms;
use big_relm4_components::list::copyable_info_row::{
    copy_all_button, BigCopyableInfoRow, BigCopyableInfoRowSpec, BigInfoLines,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::{MpvBackend, TrackInfo};

/// Build one copyable info row and register it for "Copy All".
fn info_row(title: &str, value: &str, lines: &BigInfoLines) -> adw::ActionRow {
    BigCopyableInfoRow::new(
        BigCopyableInfoRowSpec::new(title, value, i18n("Copy")),
        Some(lines),
    )
    .into_row()
}

/// Present the media-information dialog as a transient child of `parent`.
pub fn show_video_media_info_dialog(parent: &impl IsA<gtk::Window>, mpv: &Arc<MpvBackend>) {
    let dialog = adw::Window::builder()
        .title(i18n("Media Information"))
        .transient_for(parent)
        .modal(false)
        .default_width(500)
        .default_height(500)
        .build();

    let toolbar = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let lines = BigInfoLines::new();
    header.pack_end(&copy_all_button(&i18n("Copy All"), lines.clone()));
    toolbar.add_top_bar(&header);

    let page = adw::PreferencesPage::new();
    page.add(&build_general_group(mpv, &lines));
    if let Some(group) = build_metadata_group(mpv, &lines) {
        page.add(&group);
    }
    if let Some(group) = build_video_group(mpv, &lines) {
        page.add(&group);
    }
    if let Some(group) = build_audio_group(mpv, &lines) {
        page.add(&group);
    }
    if let Some(group) = build_streaming_group(mpv, &lines) {
        page.add(&group);
    }
    for group in build_track_groups(&mpv.get_track_list(), &lines) {
        page.add(&group);
    }

    toolbar.set_content(Some(&page));
    dialog.set_content(Some(&toolbar));
    dialog.present();
}

fn build_general_group(mpv: &Arc<MpvBackend>, lines: &BigInfoLines) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("General"))
        .build();

    let file_path = mpv.get_current_file().unwrap_or_default();
    if !file_path.is_empty() {
        group.add(&info_row(&i18n("File"), &file_path, lines));
        if let Ok(meta) = std::fs::metadata(&file_path) {
            group.add(&info_row(
                &i18n("File Size"),
                &decimal_detailed(meta.len()),
                lines,
            ));
        }
    }
    let media_title = mpv.get_media_title().unwrap_or_default();
    if !media_title.is_empty() && media_title != file_path {
        group.add(&info_row(&i18n("Title"), &media_title, lines));
    }
    let format = mpv.get_file_format();
    if !format.is_empty() {
        group.add(&info_row(&i18n("Format"), &format, lines));
    }
    let duration = mpv.get_duration();
    if duration > 0.0 {
        group.add(&info_row(&i18n("Duration"), &duration_hms(duration), lines));
    }
    let chapters = mpv.get_chapter_count();
    if chapters > 0 {
        group.add(&info_row(&i18n("Chapters"), &chapters.to_string(), lines));
    }
    group
}

fn build_metadata_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigInfoLines,
) -> Option<adw::PreferencesGroup> {
    let metadata = mpv.get_metadata();
    if metadata.is_empty() {
        return None;
    }
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Metadata"))
        .build();
    for (key, label) in [
        ("artist", "Artist"),
        ("album", "Album"),
        ("genre", "Genre"),
        ("date", "Date"),
        ("track", "Track"),
    ] {
        if let Some(value) = metadata.get(key).filter(|v| !v.is_empty()) {
            group.add(&info_row(&i18n(label), value, lines));
        }
    }
    Some(group)
}

fn build_video_group(mpv: &Arc<MpvBackend>, lines: &BigInfoLines) -> Option<adw::PreferencesGroup> {
    let video_codec = mpv.get_video_codec();
    if video_codec.is_empty() {
        return None;
    }
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Video"))
        .build();
    group.add(&info_row(&i18n("Codec"), &video_codec, lines));
    if let Some((width, height)) = mpv.get_video_dimensions() {
        group.add(&info_row(
            &i18n("Resolution"),
            &format!("{width}×{height}"),
            lines,
        ));
    }
    let fps = mpv.get_property_f64("estimated-vf-fps");
    if fps > 0.0 {
        group.add(&info_row(
            &i18n("Frame Rate"),
            &format!("{fps:.2} fps"),
            lines,
        ));
    }
    let bitrate = mpv.get_video_bitrate();
    if bitrate > 0.0 {
        group.add(&info_row(
            &i18n("Bitrate"),
            &format!("{:.0} kbps", bitrate / 1000.0),
            lines,
        ));
    }
    let hwdec = mpv.get_property_string("hwdec-current");
    group.add(&info_row(
        &i18n("Hardware Decoder"),
        if hwdec.is_empty() { "Software" } else { &hwdec },
        lines,
    ));
    let pixel_format = mpv.get_property_string("video-params/pixelformat");
    if !pixel_format.is_empty() {
        group.add(&info_row(&i18n("Pixel Format"), &pixel_format, lines));
    }
    let colormatrix = mpv.get_property_string("video-params/colormatrix");
    if !colormatrix.is_empty() {
        group.add(&info_row(&i18n("Colorspace"), &colormatrix, lines));
    }
    let primaries = mpv.get_property_string("video-params/primaries");
    if !primaries.is_empty() {
        group.add(&info_row(&i18n("Primaries"), &primaries, lines));
    }
    let transfer = mpv.get_property_string("video-params/gamma");
    if !transfer.is_empty() {
        let label = if transfer.contains("pq") || transfer.contains("hlg") {
            i18n("HDR")
        } else {
            i18n("Transfer")
        };
        group.add(&info_row(&label, &transfer, lines));
    }
    Some(group)
}

fn build_audio_group(mpv: &Arc<MpvBackend>, lines: &BigInfoLines) -> Option<adw::PreferencesGroup> {
    let audio_codec = mpv.get_audio_codec_name();
    if audio_codec.is_empty() {
        return None;
    }
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Audio"))
        .build();
    group.add(&info_row(&i18n("Codec"), &audio_codec, lines));
    let bitrate = mpv.get_audio_bitrate();
    if bitrate > 0.0 {
        group.add(&info_row(
            &i18n("Bitrate"),
            &format!("{:.0} kbps", bitrate / 1000.0),
            lines,
        ));
    }
    let audio_info = mpv.get_audio_info();
    if audio_info.sample_rate > 0 {
        group.add(&info_row(
            &i18n("Sample Rate"),
            &format!("{} Hz", audio_info.sample_rate),
            lines,
        ));
    }
    if !audio_info.channels.is_empty() {
        group.add(&info_row(&i18n("Channels"), &audio_info.channels, lines));
    }
    if !audio_info.format.is_empty() {
        group.add(&info_row(&i18n("Format"), &audio_info.format, lines));
    }
    Some(group)
}

fn build_streaming_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigInfoLines,
) -> Option<adw::PreferencesGroup> {
    let cache_state = mpv.get_cache_buffering_state();
    if cache_state < 0 {
        return None;
    }
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Streaming"))
        .build();
    group.add(&info_row(&i18n("Cache"), &format!("{cache_state}%"), lines));
    Some(group)
}

/// One group per track type, shown only when it carries a meaningful choice
/// (>1 video/audio track, or any subtitle track).
fn build_track_groups(tracks: &[TrackInfo], lines: &BigInfoLines) -> Vec<adw::PreferencesGroup> {
    let mut groups = Vec::new();
    let video: Vec<_> = tracks.iter().filter(|t| t.track_type == "video").collect();
    let audio: Vec<_> = tracks.iter().filter(|t| t.track_type == "audio").collect();
    let subtitle: Vec<_> = tracks.iter().filter(|t| t.track_type == "sub").collect();

    if video.len() > 1 {
        let group = adw::PreferencesGroup::builder()
            .title(i18n("Video Tracks"))
            .build();
        for track in &video {
            let label = if track.title.is_empty() {
                format!("{} #{}", i18n("Video"), track.id)
            } else {
                track.title.clone()
            };
            let detail = format!("{}{}", track.codec, selected_mark(track.selected));
            group.add(&info_row(&label, &detail, lines));
        }
        groups.push(group);
    }

    if audio.len() > 1 {
        let group = adw::PreferencesGroup::builder()
            .title(i18n("Audio Tracks"))
            .build();
        for track in &audio {
            let detail = format!("{}{}", track.codec, selected_mark(track.selected));
            group.add(&info_row(
                &track_label(track, &i18n("Audio")),
                &detail,
                lines,
            ));
        }
        groups.push(group);
    }

    if !subtitle.is_empty() {
        let group = adw::PreferencesGroup::builder()
            .title(i18n("Subtitle Tracks"))
            .build();
        for track in &subtitle {
            let detail = format!(
                "{}{}{}",
                track.codec,
                if track.external { " (ext)" } else { "" },
                selected_mark(track.selected),
            );
            group.add(&info_row(
                &track_label(track, &i18n("Subtitle")),
                &detail,
                lines,
            ));
        }
        groups.push(group);
    }

    groups
}

/// Display label for a track: its title, else its language, else `kind #id`.
fn track_label(track: &TrackInfo, kind: &str) -> String {
    if !track.title.is_empty() {
        track.title.clone()
    } else if !track.lang.is_empty() {
        track.lang.clone()
    } else {
        format!("{kind} #{}", track.id)
    }
}

fn selected_mark(selected: bool) -> &'static str {
    if selected {
        " ✓"
    } else {
        ""
    }
}
