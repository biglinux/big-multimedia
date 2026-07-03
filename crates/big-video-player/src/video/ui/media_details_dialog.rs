//! App-local media-details dialog (video port, V5).
//!
//! Reads the live mpv backend (general / metadata / video / audio / streaming /
//! track sections) into copyable rows, with a "Copy All" header button. A free
//! function rather than a Relm4 component: it builds and presents a transient
//! `adw::Dialog` from a one-shot snapshot of mpv state and owns no further
//! lifecycle (the shell calls it from a header/menu action).
//!
//! Reuse note: the shared `big_media_components::media::file_metadata_dialog`
//! consumes an ffprobe JSON model, whereas the player wants the *live* mpv view
//! (hwdec, cache %, selected tracks). Those differ, so this dialog stays
//! app-local but reuses the shared dialog chrome, copyable-row + copy-all
//! widgets, and the shared byte/time formatting helpers, not bespoke ones.

use std::sync::Arc;

use adw::prelude::*;
use big_media_components::bytes::decimal_detailed;
use big_media_components::time::duration_hms;
use big_relm4_components::layout::{
    dialog_page::BigDialogPage, preferences::BigPreferencesGroupSpec,
};
use big_relm4_components::list::copyable_detail_row::{
    copy_all_button, BigCopyableDetailRow, BigCopyableDetailRowSpec, BigDetailLines,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::{MpvBackend, TrackMetadata};

/// Build one copyable detail row and register it for "Copy All".
fn media_detail_row(title: &str, value: &str, lines: &BigDetailLines) -> adw::ActionRow {
    BigCopyableDetailRow::new(
        BigCopyableDetailRowSpec::new(title, value, i18n("Copy")),
        Some(lines),
    )
    .into_row()
}

/// Present the media-details dialog over `parent`.
pub fn show_video_media_details_dialog(parent: &impl IsA<gtk::Widget>, mpv: &Arc<MpvBackend>) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Media Information"))
        .content_width(500)
        .content_height(500)
        .build();

    let page = BigDialogPage::scrolled_clamped(500, 420);
    let lines = BigDetailLines::new();
    page.header()
        .pack_end(&copy_all_button(&i18n("Copy All"), lines.clone()));

    let content = page.content().clone();
    content.set_spacing(12);
    content.append(&build_general_group(mpv, &lines));
    if let Some(group) = build_metadata_group(mpv, &lines) {
        content.append(&group);
    }
    if let Some(group) = build_video_group(mpv, &lines) {
        content.append(&group);
    }
    if let Some(group) = build_audio_group(mpv, &lines) {
        content.append(&group);
    }
    if let Some(group) = build_streaming_group(mpv, &lines) {
        content.append(&group);
    }
    for group in build_track_groups(&mpv.get_track_list(), &lines) {
        content.append(&group);
    }

    let root = page.into_root();
    dialog.set_child(Some(&root));
    dialog.present(Some(parent));
}

fn build_general_group(mpv: &Arc<MpvBackend>, lines: &BigDetailLines) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("General")).build();

    let file_path = mpv.get_current_file().unwrap_or_default();
    if !file_path.is_empty() {
        group.add(&media_detail_row(&i18n("File"), &file_path, lines));
        if let Ok(meta) = std::fs::metadata(&file_path) {
            group.add(&media_detail_row(
                &i18n("File Size"),
                &decimal_detailed(meta.len()),
                lines,
            ));
        }
    }
    let media_title = mpv.get_media_title().unwrap_or_default();
    if !media_title.is_empty() && media_title != file_path {
        group.add(&media_detail_row(&i18n("Title"), &media_title, lines));
    }
    let format = mpv.get_file_format();
    if !format.is_empty() {
        group.add(&media_detail_row(&i18n("Format"), &format, lines));
    }
    let duration = mpv.get_duration();
    if duration > 0.0 {
        group.add(&media_detail_row(
            &i18n("Duration"),
            &duration_hms(duration),
            lines,
        ));
    }
    let chapters = mpv.get_chapter_count();
    if chapters > 0 {
        group.add(&media_detail_row(
            &i18n("Chapters"),
            &chapters.to_string(),
            lines,
        ));
    }
    group
}

fn build_metadata_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigDetailLines,
) -> Option<adw::PreferencesGroup> {
    let metadata = mpv.get_metadata();
    if metadata.is_empty() {
        return None;
    }
    let group = BigPreferencesGroupSpec::new(i18n("Metadata")).build();
    for (key, label) in [
        ("artist", "Artist"),
        ("album", "Album"),
        ("genre", "Genre"),
        ("date", "Date"),
        ("track", "Track"),
    ] {
        if let Some(value) = metadata.get(key).filter(|v| !v.is_empty()) {
            group.add(&media_detail_row(&i18n(label), value, lines));
        }
    }
    Some(group)
}

fn build_video_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigDetailLines,
) -> Option<adw::PreferencesGroup> {
    let video_codec = mpv.get_video_codec();
    if video_codec.is_empty() {
        return None;
    }
    let group = BigPreferencesGroupSpec::new(i18n("Video")).build();
    group.add(&media_detail_row(&i18n("Codec"), &video_codec, lines));
    if let Some((width, height)) = mpv.get_video_dimensions() {
        group.add(&media_detail_row(
            &i18n("Resolution"),
            &format!("{width}×{height}"),
            lines,
        ));
    }
    let fps = mpv.get_property_f64("estimated-vf-fps");
    if fps > 0.0 {
        group.add(&media_detail_row(
            &i18n("Frame Rate"),
            &format!("{fps:.2} fps"),
            lines,
        ));
    }
    let bitrate = mpv.get_video_bitrate();
    if bitrate > 0.0 {
        group.add(&media_detail_row(
            &i18n("Bitrate"),
            &format!("{:.0} kbps", bitrate / 1000.0),
            lines,
        ));
    }
    let hwdec = mpv.get_property_string("hwdec-current");
    group.add(&media_detail_row(
        &i18n("Hardware Decoder"),
        if hwdec.is_empty() { "Software" } else { &hwdec },
        lines,
    ));
    let pixel_format = mpv.get_property_string("video-params/pixelformat");
    if !pixel_format.is_empty() {
        group.add(&media_detail_row(
            &i18n("Pixel Format"),
            &pixel_format,
            lines,
        ));
    }
    let colormatrix = mpv.get_property_string("video-params/colormatrix");
    if !colormatrix.is_empty() {
        group.add(&media_detail_row(&i18n("Colorspace"), &colormatrix, lines));
    }
    let primaries = mpv.get_property_string("video-params/primaries");
    if !primaries.is_empty() {
        group.add(&media_detail_row(&i18n("Primaries"), &primaries, lines));
    }
    let transfer = mpv.get_property_string("video-params/gamma");
    if !transfer.is_empty() {
        let label = if transfer.contains("pq") || transfer.contains("hlg") {
            i18n("HDR")
        } else {
            i18n("Transfer")
        };
        group.add(&media_detail_row(&label, &transfer, lines));
    }
    Some(group)
}

fn build_audio_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigDetailLines,
) -> Option<adw::PreferencesGroup> {
    let audio_codec = mpv.get_audio_codec_name();
    if audio_codec.is_empty() {
        return None;
    }
    let group = BigPreferencesGroupSpec::new(i18n("Audio")).build();
    group.add(&media_detail_row(&i18n("Codec"), &audio_codec, lines));
    let bitrate = mpv.get_audio_bitrate();
    if bitrate > 0.0 {
        group.add(&media_detail_row(
            &i18n("Bitrate"),
            &format!("{:.0} kbps", bitrate / 1000.0),
            lines,
        ));
    }
    let audio_properties = mpv.get_audio_metadata();
    if audio_properties.sample_rate > 0 {
        group.add(&media_detail_row(
            &i18n("Sample Rate"),
            &format!("{} Hz", audio_properties.sample_rate),
            lines,
        ));
    }
    if !audio_properties.channels.is_empty() {
        group.add(&media_detail_row(
            &i18n("Channels"),
            &audio_properties.channels,
            lines,
        ));
    }
    if !audio_properties.format.is_empty() {
        group.add(&media_detail_row(
            &i18n("Format"),
            &audio_properties.format,
            lines,
        ));
    }
    Some(group)
}

fn build_streaming_group(
    mpv: &Arc<MpvBackend>,
    lines: &BigDetailLines,
) -> Option<adw::PreferencesGroup> {
    let cache_state = mpv.get_cache_buffering_state();
    if cache_state < 0 {
        return None;
    }
    let group = BigPreferencesGroupSpec::new(i18n("Streaming")).build();
    group.add(&media_detail_row(
        &i18n("Cache"),
        &format!("{cache_state}%"),
        lines,
    ));
    Some(group)
}

/// One group per track type, shown only when it carries a meaningful choice
/// (>1 video/audio track, or any subtitle track).
fn build_track_groups(
    tracks: &[TrackMetadata],
    lines: &BigDetailLines,
) -> Vec<adw::PreferencesGroup> {
    let mut groups = Vec::new();
    let video: Vec<_> = tracks.iter().filter(|t| t.track_type == "video").collect();
    let audio: Vec<_> = tracks.iter().filter(|t| t.track_type == "audio").collect();
    let subtitle: Vec<_> = tracks.iter().filter(|t| t.track_type == "sub").collect();

    if video.len() > 1 {
        let group = BigPreferencesGroupSpec::new(i18n("Video Tracks")).build();
        for track in &video {
            let label = if track.title.is_empty() {
                format!("{} #{}", i18n("Video"), track.id)
            } else {
                track.title.clone()
            };
            let detail = format!("{}{}", track.codec, selected_mark(track.selected));
            group.add(&media_detail_row(&label, &detail, lines));
        }
        groups.push(group);
    }

    if audio.len() > 1 {
        let group = BigPreferencesGroupSpec::new(i18n("Audio Tracks")).build();
        for track in &audio {
            let detail = format!("{}{}", track.codec, selected_mark(track.selected));
            group.add(&media_detail_row(
                &track_label(track, &i18n("Audio")),
                &detail,
                lines,
            ));
        }
        groups.push(group);
    }

    if !subtitle.is_empty() {
        let group = BigPreferencesGroupSpec::new(i18n("Subtitle Tracks")).build();
        for track in &subtitle {
            let detail = format!(
                "{}{}{}",
                track.codec,
                if track.external { " (ext)" } else { "" },
                selected_mark(track.selected),
            );
            group.add(&media_detail_row(
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
fn track_label(track: &TrackMetadata, kind: &str) -> String {
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
