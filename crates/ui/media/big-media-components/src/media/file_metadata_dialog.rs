//! Copyable media file metadata dialog.

#![allow(clippy::needless_borrows_for_generic_args)]

use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::list::copyable_detail_row::{
    BigCopyableDetailRow, BigCopyableDetailRowSpec, BigDetailLines, copy_all_button,
};
use relm4::gtk;

use crate::bytes::decimal_compact;
use crate::time::duration_hms;

type OpenContainingFolderCallback = Rc<dyn Fn(PathBuf)>;
type CopyableMetadataLines = BigDetailLines;

/// Translated labels rendered by the media file information dialog.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMediaFileMetadataDialogLabels {
    pub title: String,
    pub copy: String,
    pub copy_all: String,
    pub open_folder: String,
    pub file_size: String,
    pub duration: String,
    pub container: String,
    pub format: String,
    pub bitrate: String,
    pub video_stream: String,
    pub audio_stream: String,
    pub subtitle_stream: String,
    pub codec: String,
    pub language: String,
    pub resolution: String,
    pub pixel_format: String,
    pub frame_rate: String,
    pub sample_rate: String,
    pub channels: String,
}

impl BigMediaFileMetadataDialogLabels {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            title: text("File Information"),
            copy: text("Copy"),
            copy_all: text("Copy All"),
            open_folder: text("Open Folder"),
            file_size: text("File Size"),
            duration: text("Duration"),
            container: text("Container"),
            format: text("Format"),
            bitrate: text("Bitrate"),
            video_stream: text("Video Stream"),
            audio_stream: text("Audio Stream"),
            subtitle_stream: text("Subtitle Stream"),
            codec: text("Codec"),
            language: text("Language"),
            resolution: text("Resolution"),
            pixel_format: text("Pixel Format"),
            frame_rate: text("Frame Rate"),
            sample_rate: text("Sample Rate"),
            channels: text("Channels"),
        }
    }
}

/// Builder spec for [`show_media_file_metadata_dialog`].
///
/// Build with [`BigMediaFileMetadataDialogSpec::new`] then chain the `.media_metadata`,
/// `.duration_secs`, and `.open_folder` builders to enrich the dialog.
#[allow(missing_docs)]
#[derive(Clone)]
pub struct BigMediaFileMetadataDialogSpec {
    pub file_path: PathBuf,
    pub labels: BigMediaFileMetadataDialogLabels,
    pub media_metadata: Option<serde_json::Value>,
    pub duration_secs: Option<f64>,
    pub open_folder: Option<OpenContainingFolderCallback>,
}

impl BigMediaFileMetadataDialogSpec {
    /// Build a spec showing only the basic file size/duration rows.
    #[must_use]
    pub fn new(file_path: PathBuf, labels: BigMediaFileMetadataDialogLabels) -> Self {
        Self {
            file_path,
            labels,
            media_metadata: None,
            duration_secs: None,
            open_folder: None,
        }
    }

    /// Add a duration row; non-positive durations are stored as [`None`].
    #[must_use]
    pub fn duration_secs(mut self, duration_secs: f64) -> Self {
        self.duration_secs = (duration_secs > 0.0).then_some(duration_secs);
        self
    }
}

/// Present a copyable media file information dialog as a transient child of `parent`.
pub fn show_media_file_metadata_dialog(
    parent: &impl IsA<gtk::Window>,
    spec: BigMediaFileMetadataDialogSpec,
) {
    let dialog = adw::Window::builder()
        .title(&spec.labels.title)
        .modal(true)
        .transient_for(parent)
        .default_width(500)
        .default_height(600)
        .build();

    let page = BigDialogPage::scrolled();
    let copyable_metadata_lines = CopyableMetadataLines::new();
    let copy_all_button = copy_all_button(&spec.labels.copy_all, copyable_metadata_lines.clone());
    page.header().pack_end(&copy_all_button);

    let content = page.content().clone();
    content.set_spacing(12);
    append_basic_group(&content, &spec, &copyable_metadata_lines);
    if let Some(media_metadata) = &spec.media_metadata {
        append_container_group(
            &content,
            media_metadata,
            &spec.labels,
            &copyable_metadata_lines,
        );
        append_stream_groups(
            &content,
            media_metadata,
            &spec.labels,
            &copyable_metadata_lines,
        );
    }

    let root = page.into_root();
    dialog.set_content(Some(&root));
    dialog.present();
}

fn copyable_metadata_row(
    title: &str,
    value: &str,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) -> adw::ActionRow {
    BigCopyableDetailRow::new(
        // Values come from the file and its tags: "Tom & Jerry" is text.
        BigCopyableDetailRowSpec::new(title, value, &labels.copy).selectable_subtitle(false),
        Some(copyable_metadata_lines),
    )
    .into_row()
}

fn append_basic_group(
    list: &gtk::Box,
    spec: &BigMediaFileMetadataDialogSpec,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    let filename = spec
        .file_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    copyable_metadata_lines.push(filename.clone());

    let group = BigPreferencesGroupSpec::new(&filename).build();
    if spec.file_path.parent().is_some() {
        append_open_folder_button(&group, spec);
    }

    if let Ok(meta) = std::fs::metadata(&spec.file_path) {
        group.add(&copyable_metadata_row(
            &spec.labels.file_size,
            &decimal_compact(meta.len()),
            &spec.labels,
            copyable_metadata_lines,
        ));
    }

    if let Some(duration) = spec.duration_secs {
        group.add(&copyable_metadata_row(
            &spec.labels.duration,
            &duration_hms(duration),
            &spec.labels,
            copyable_metadata_lines,
        ));
    }

    list.append(&group);
}

fn append_open_folder_button(group: &adw::PreferencesGroup, spec: &BigMediaFileMetadataDialogSpec) {
    let Some(open_folder) = &spec.open_folder else {
        return;
    };
    let open_button = gtk::Button::builder()
        .label(&spec.labels.open_folder)
        .css_classes(["flat"])
        .build();
    let target = spec.file_path.clone();
    let open_folder_callback = open_folder.clone();
    open_button.connect_clicked(move |_| {
        open_folder_callback(target.clone());
    });
    group.set_header_suffix(Some(&open_button));
}

fn append_container_group(
    list: &gtk::Box,
    media_metadata: &serde_json::Value,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    let Some(fmt) = media_metadata.get("format") else {
        return;
    };
    push_section_header(copyable_metadata_lines, &labels.container);

    let group = BigPreferencesGroupSpec::new(&labels.container).build();
    if let Some(name) = fmt
        .get("format_long_name")
        .and_then(serde_json::Value::as_str)
    {
        group.add(&copyable_metadata_row(
            &labels.format,
            name,
            labels,
            copyable_metadata_lines,
        ));
    }
    if let Some(kbps) = fmt
        .get("bit_rate")
        .and_then(serde_json::Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(|bits| bits / 1000)
    {
        group.add(&copyable_metadata_row(
            &labels.bitrate,
            &format!("{kbps} kbps"),
            labels,
            copyable_metadata_lines,
        ));
    }
    list.append(&group);
}

fn append_stream_groups(
    list: &gtk::Box,
    media_metadata: &serde_json::Value,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    let Some(streams) = media_metadata
        .get("streams")
        .and_then(serde_json::Value::as_array)
    else {
        return;
    };
    for stream in streams {
        append_stream_group(list, stream, labels, copyable_metadata_lines);
    }
}

fn append_stream_group(
    list: &gtk::Box,
    stream: &serde_json::Value,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    let codec_type = stream
        .get("codec_type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let codec_name = stream
        .get("codec_long_name")
        .or_else(|| stream.get("codec_name"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");

    let title = match codec_type {
        "video" => labels.video_stream.clone(),
        "audio" => labels.audio_stream.clone(),
        "subtitle" => labels.subtitle_stream.clone(),
        _ => codec_type.to_string(),
    };
    push_section_header(copyable_metadata_lines, &title);

    let group = BigPreferencesGroupSpec::new(&title).build();
    group.add(&copyable_metadata_row(
        &labels.codec,
        codec_name,
        labels,
        copyable_metadata_lines,
    ));

    match codec_type {
        "video" => append_video_stream_rows(&group, stream, labels, copyable_metadata_lines),
        "audio" => append_audio_stream_rows(&group, stream, labels, copyable_metadata_lines),
        _ => {}
    }
    if let Some(lang) = stream
        .get("tags")
        .and_then(|tags| tags.get("language"))
        .and_then(serde_json::Value::as_str)
    {
        group.add(&copyable_metadata_row(
            &labels.language,
            lang,
            labels,
            copyable_metadata_lines,
        ));
    }
    list.append(&group);
}

fn append_video_stream_rows(
    group: &adw::PreferencesGroup,
    stream: &serde_json::Value,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    if let (Some(width), Some(height)) = (
        stream.get("width").and_then(serde_json::Value::as_u64),
        stream.get("height").and_then(serde_json::Value::as_u64),
    ) {
        let label = resolution_label(height);
        group.add(&copyable_metadata_row(
            &labels.resolution,
            &format!("{width}x{height} ({label})"),
            labels,
            copyable_metadata_lines,
        ));
    }
    if let Some(pix_fmt) = stream.get("pix_fmt").and_then(serde_json::Value::as_str) {
        group.add(&copyable_metadata_row(
            &labels.pixel_format,
            pix_fmt,
            labels,
            copyable_metadata_lines,
        ));
    }
    if let Some(fps) = stream
        .get("r_frame_rate")
        .and_then(serde_json::Value::as_str)
    {
        group.add(&copyable_metadata_row(
            &labels.frame_rate,
            &format_fps(fps),
            labels,
            copyable_metadata_lines,
        ));
    }
}

fn append_audio_stream_rows(
    group: &adw::PreferencesGroup,
    stream: &serde_json::Value,
    labels: &BigMediaFileMetadataDialogLabels,
    copyable_metadata_lines: &CopyableMetadataLines,
) {
    if let Some(sample_rate) = stream
        .get("sample_rate")
        .and_then(serde_json::Value::as_str)
    {
        let rate = sample_rate.parse().unwrap_or(0);
        group.add(&copyable_metadata_row(
            &labels.sample_rate,
            &format!("{sample_rate} Hz ({})", sample_rate_label(rate)),
            labels,
            copyable_metadata_lines,
        ));
    }
    if let Some(channels) = stream.get("channels").and_then(serde_json::Value::as_u64) {
        group.add(&copyable_metadata_row(
            &labels.channels,
            &channel_label(channels),
            labels,
            copyable_metadata_lines,
        ));
    }
}

fn push_section_header(copyable_metadata_lines: &CopyableMetadataLines, title: &str) {
    copyable_metadata_lines.push(String::new());
    copyable_metadata_lines.push(format!("[{title}]"));
}

/// Classify `height` into the marketing labels `SD`/`HD`/`FHD`/`4K`/`8K`.
#[must_use]
pub fn resolution_label(height: u64) -> &'static str {
    match height {
        0..=480 => "SD",
        481..=720 => "HD",
        721..=1080 => "FHD",
        1081..=2160 => "4K",
        _ => "8K",
    }
}

/// Classify a sample rate as `Low`/`CD`/`Studio`/`Hi-Res`.
#[must_use]
pub fn sample_rate_label(rate: u64) -> &'static str {
    match rate {
        0..=22050 => "Low",
        22051..=44100 => "CD",
        44101..=48000 => "Studio",
        _ => "Hi-Res",
    }
}

/// Render an ffmpeg rational frame-rate string (`num/den`) as `"<fps> fps"`.
#[must_use]
pub fn format_fps(raw: &str) -> String {
    let Some((num, den)) = raw.split_once('/') else {
        return format!("{raw} fps");
    };
    let (Ok(n), Ok(d)) = (num.parse::<f64>(), den.parse::<f64>()) else {
        return format!("{raw} fps");
    };
    if d > 0.0 {
        format!("{:.2} fps", n / d)
    } else {
        format!("{raw} fps")
    }
}

/// Render a channel count with a human label suffix when known.
///
/// `1` → `"1 (Mono)"`, `2` → `"2 (Stereo)"`, `6` → `"6 (5.1)"`,
/// `8` → `"8 (7.1)"`. Other values are formatted as bare numbers.
#[must_use]
pub fn channel_label(channels: u64) -> String {
    let name = match channels {
        1 => "Mono",
        2 => "Stereo",
        6 => "5.1",
        8 => "7.1",
        _ => "",
    };
    if name.is_empty() {
        channels.to_string()
    } else {
        format!("{channels} ({name})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_labels_classify_known_values() {
        assert_eq!(resolution_label(720), "HD");
        assert_eq!(resolution_label(1080), "FHD");
        assert_eq!(sample_rate_label(44_100), "CD");
        assert_eq!(sample_rate_label(96_000), "Hi-Res");
    }

    #[test]
    fn media_values_format_for_dialog_rows() {
        assert_eq!(duration_hms(3_661.5), "1:01:01");
        assert_eq!(format_fps("30000/1001"), "29.97 fps");
        assert_eq!(format_fps("bad"), "bad fps");
        assert_eq!(channel_label(2), "2 (Stereo)");
        assert_eq!(channel_label(3), "3");
    }
}
