//! Single-file quick-export: map the compact choice the in-player panel offers
//! onto a runtime [`Profile`].
//!
//! The full converter exposes ~15 controls; a player embedding "clip & export",
//! "convert this file", or "extract audio" only needs a handful. This module is
//! the pure bridge: it produces the same runtime [`Profile`] the converter
//! engine consumes, so there is one conversion path, not two. The editor's
//! trim segments travel on the `ConversionQueueEntry` (not here); transform/colour filters
//! travel in [`QuickExportChoice::filters`].

use std::path::{Path, PathBuf};

use crate::converter::profiles::{
    AudioConfig, OutputConfig, Profile, ProfileMeta, SubtitleConfig, VideoConfig, VideoFilters,
};
use crate::converter::queue::{ConversionQueue, QueueMessage};

/// Output format the quick-export panel produces for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickExportFormat {
    /// Re-encode video into `container` (e.g. `"mp4"`) with `codec` at `quality`.
    Video {
        /// Output container/extension, e.g. `"mp4"`, `"mkv"`, `"webm"`.
        container: String,
        /// Video codec id, e.g. `"h264"`, `"h265"`, `"vp9"`.
        codec: String,
        /// Quality preset id understood by the runtime, e.g. `"good"`.
        quality: String,
    },
    /// Audio-only extract/convert. The caller pairs this with
    /// `Queue::set_audio_only_mode(true)`; the container is derived from `codec`.
    Audio {
        /// Audio codec id, e.g. `"mp3"`, `"aac"`, `"flac"`.
        codec: String,
        /// Target bitrate, e.g. `"192k"` (ignored by lossless codecs).
        bitrate: String,
    },
}

/// A single-file export request from the in-player panel.
#[derive(Debug, Clone)]
pub struct QuickExportChoice {
    /// Chosen output format + codec/quality.
    pub format: QuickExportFormat,
    /// Editor transform/colour/crop filters (default = untouched source).
    pub filters: VideoFilters,
}

impl QuickExportChoice {
    /// Fast "keep format" clip: stream-copy (`-c copy`) into the **source's own
    /// container**, no re-encode. Near-instant; the cut snaps to the nearest
    /// keyframe and filters are cleared (a copied stream cannot be filtered).
    ///
    /// `source` only supplies the output extension; an unknown/missing extension
    /// falls back to `mkv` (the most permissive copy container).
    #[must_use]
    pub fn keep_format(source: &Path) -> Self {
        let container = source
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .filter(|ext| matches!(ext.as_str(), "mkv" | "mp4" | "mov" | "webm" | "avi" | "ts" | "m2ts" | "mpeg" | "mpg" | "flv" | "ogv"))
            .unwrap_or_else(|| "mkv".to_string());
        Self {
            format: QuickExportFormat::Video {
                container,
                codec: "copy".into(),
                quality: String::new(),
            },
            filters: VideoFilters::default(),
        }
    }
}

/// Default container for an audio-only export, by codec. `"auto"` lets the
/// runtime decide for anything unmapped.
#[must_use]
pub fn audio_container_for_codec(codec: &str) -> &'static str {
    match codec {
        "mp3" => "mp3",
        "aac" => "m4a",
        "flac" => "flac",
        "opus" => "opus",
        "vorbis" => "ogg",
        _ => "auto",
    }
}

/// Build the runtime [`Profile`] for a single-file quick export.
///
/// Only the panel's handful of choices are set; everything else uses the
/// runtime defaults, so the output matches what the full converter would
/// produce from the same selections. Audio exports set `handling = "encode"`
/// (the caller also flips `Queue::set_audio_only_mode`).
#[must_use]
pub fn single_file_export_profile(choice: &QuickExportChoice) -> Profile {
    match &choice.format {
        QuickExportFormat::Video {
            container,
            codec,
            quality,
        } => Profile {
            profile: ProfileMeta {
                name: "Quick Export".into(),
                ..ProfileMeta::default()
            },
            video: VideoConfig {
                codec: codec.clone(),
                quality: quality.clone(),
                container: container.clone(),
                filters: choice.filters.clone(),
                ..VideoConfig::default()
            },
            audio: AudioConfig::default(),
            subtitles: SubtitleConfig::default(),
            output: OutputConfig {
                container: container.clone(),
                ..OutputConfig::default()
            },
        },
        QuickExportFormat::Audio { codec, bitrate } => {
            let container = audio_container_for_codec(codec).to_string();
            Profile {
                profile: ProfileMeta {
                    name: "Extract Audio".into(),
                    ..ProfileMeta::default()
                },
                video: VideoConfig::default(),
                audio: AudioConfig {
                    handling: "encode".into(),
                    codec: codec.clone(),
                    bitrate: bitrate.clone(),
                    ..AudioConfig::default()
                },
                subtitles: SubtitleConfig::default(),
                output: OutputConfig {
                    container,
                    ..OutputConfig::default()
                },
            }
        }
    }
}

/// Convert a runtime conversion-queue progress snapshot into the shared progress
/// component's `FfmpegProgressSnapshot`.
///
/// `big-media-runtime` (the engine) and `big-media-components` (the UI) each
/// define a field-identical `FfmpegProgressSnapshot`; this is the single bridge so a player
/// embedding can drive [`big_media_components::converter::progress::BigConverterProgress`]
/// straight from `QueueMessage::Progress`, exactly as the full converter does.
#[must_use]
pub fn ui_progress_snapshot(
    runtime_progress_snapshot: &crate::converter::ffmpeg::progress::FfmpegProgressSnapshot,
) -> big_media_components::ffmpeg::progress::FfmpegProgressSnapshot {
    big_media_components::ffmpeg::progress::FfmpegProgressSnapshot {
        time_secs: runtime_progress_snapshot.time_secs,
        total_secs: runtime_progress_snapshot.total_secs,
        speed: runtime_progress_snapshot.speed,
        size_bytes: runtime_progress_snapshot.size_bytes,
        bitrate_kbps: runtime_progress_snapshot.bitrate_kbps,
        frame: runtime_progress_snapshot.frame,
        fps: runtime_progress_snapshot.fps,
        eta_secs: runtime_progress_snapshot.eta_secs,
        percent: runtime_progress_snapshot.percent,
        done: runtime_progress_snapshot.done,
    }
}

/// Build (but do **not** start) the one-item [`ConversionQueue`] for a quick
/// export, returning the queue plus the added item's id and resolved output path.
///
/// This is the shared core of both the fire-and-forget
/// [`run_single_export_blocking`] and the interactive in-player trim mode: the
/// latter keeps the returned queue alive so it can drive live progress off
/// `queue.receiver()` and abort with `queue.cancel()`. Multi-segment outputs
/// default to [`crate::converter::queue::SegmentOutputMode::Join`]; the caller
/// may switch to split via `queue.set_pending_segment_mode(..)` before
/// `queue.start()`.
///
/// `segments` is a list of `(start_secs, end_secs)` clip ranges (empty = whole
/// file).
#[must_use]
pub fn build_single_export_queue(
    input: PathBuf,
    output_dir: PathBuf,
    choice: &QuickExportChoice,
    segments: &[(f64, f64)],
) -> (ConversionQueue, u64, PathBuf) {
    use crate::converter::ffmpeg::TrimRange;
    use crate::converter::queue::SegmentOutputMode;

    let mut queue = ConversionQueue::new();
    queue.set_profile(single_file_export_profile(choice));
    if matches!(choice.format, QuickExportFormat::Audio { .. }) {
        queue.set_audio_only_mode(true);
    }
    queue.set_output_dir(output_dir);
    let id = queue.add_file(input);

    if !segments.is_empty() {
        if let Some(item) = queue.entry_mut(id) {
            item.segments = segments
                .iter()
                .map(|&(start, end)| TrimRange {
                    start: Some(start),
                    end: Some(end),
                })
                .collect();
            item.segment_mode = SegmentOutputMode::Join;
        }
    }

    let output = queue
        .items()
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.output.clone())
        .unwrap_or_default();
    (queue, id, output)
}

/// Reject malformed cut lists before spawning/probing; the worker additionally
/// validates against the authoritative probed duration.
pub fn validate_export_segments(segments: &[(f64, f64)]) -> Result<(), String> {
    if segments.len() > 10_000 || segments.iter().any(|&(a,b)| !a.is_finite() || !b.is_finite() || a < 0.0 || b <= a) {
        return Err("Each cut needs a finite start and a later end".into());
    }
    Ok(())
}

/// Run a single-file quick export to completion on the **calling thread** and
/// return the output path (or an error string).
///
/// Builds a one-item [`ConversionQueue`] from `choice` via
/// [`build_single_export_queue`], starts the conversion (which runs ffmpeg on its
/// own worker thread) and blocks on the queue's message channel until the job
/// finishes. Intended to be called from a background worker (e.g. the player's
/// `spawn_blocking_result`), never the GTK main thread — it blocks.
///
/// `segments` is a list of `(start_secs, end_secs)` clip ranges (empty = whole
/// file); when present the output keeps only those ranges joined in order.
///
/// # Errors
///
/// Returns the ffmpeg/queue error message when the conversion fails or the
/// worker channel closes before the job reports completion.
pub fn run_single_export_blocking(
    input: PathBuf,
    output_dir: PathBuf,
    choice: &QuickExportChoice,
    segments: &[(f64, f64)],
) -> Result<PathBuf, String> {
    validate_export_segments(segments)?;
    let (mut queue, id, output) = build_single_export_queue(input, output_dir, choice, segments);
    if output.as_os_str().is_empty() {
        return Err("export item vanished before start".to_string());
    }

    queue.start();
    let mut completed = false;
    loop {
        match queue.receiver().recv_blocking() {
            Ok(QueueMessage::Done { entry_id }) if entry_id == id => completed = true,
            Ok(QueueMessage::Cancelled { entry_id }) if entry_id == id => return Err("Export was cancelled".into()),
            Ok(QueueMessage::AllDone) => {
                if !completed { return Err("The queue ended without a completed output".into()); }
                let metadata = std::fs::symlink_metadata(&output).map_err(|_| "The published output is unavailable")?;
                if !metadata.is_file() || metadata.len() == 0 { return Err("The published output is not a nonempty regular file".into()); }
                return Ok(output);
            }
            Ok(QueueMessage::Failed { entry_id, error }) if entry_id == id => return Err(error),
            Ok(_) => {}
            Err(_) => return Err("export worker stopped before completion".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QuickExportChoice, QuickExportFormat, audio_container_for_codec, single_file_export_profile,
    };
    use crate::converter::profiles::VideoFilters;

    #[test]
    fn audio_container_maps_known_codecs_else_auto() {
        assert_eq!(audio_container_for_codec("mp3"), "mp3");
        assert_eq!(audio_container_for_codec("aac"), "m4a");
        assert_eq!(audio_container_for_codec("flac"), "flac");
        assert_eq!(audio_container_for_codec("opus"), "opus");
        assert_eq!(audio_container_for_codec("vorbis"), "ogg");
        assert_eq!(audio_container_for_codec("???"), "auto");
    }

    #[test]
    fn video_choice_sets_codec_quality_container_and_keeps_filters() {
        let filters = VideoFilters {
            brightness: 0.2,
            rotation: 90,
            ..VideoFilters::default()
        };
        let profile = single_file_export_profile(&QuickExportChoice {
            format: QuickExportFormat::Video {
                container: "mp4".into(),
                codec: "h265".into(),
                quality: "veryhigh".into(),
            },
            filters: filters.clone(),
        });
        assert_eq!(profile.video.codec, "h265");
        assert_eq!(profile.video.quality, "veryhigh");
        assert_eq!(profile.video.container, "mp4");
        assert_eq!(profile.output.container, "mp4");
        // Editor transform/colour carried through verbatim.
        assert_eq!(profile.video.filters.brightness, 0.2);
        assert_eq!(profile.video.filters.rotation, 90);
        // Video export does not re-encode audio by default.
        assert_eq!(profile.audio.handling, "copy");
    }

    #[test]
    fn keep_format_is_stream_copy_into_source_container_without_filters() {
        let choice = QuickExportChoice::keep_format(std::path::Path::new("/movies/Trip.MKV"));
        let profile = single_file_export_profile(&choice);
        // Source extension is preserved (lowercased) and the stream is copied.
        assert_eq!(profile.video.codec, "copy");
        assert_eq!(profile.video.container, "mkv");
        assert_eq!(profile.output.container, "mkv");
        // Default audio handling is copy → full lossless stream copy.
        assert_eq!(profile.audio.handling, "copy");
        // No filters travel on a copied stream (would make ffmpeg reject -vf):
        // every adjustment/transform stays at its identity default.
        let filters = &profile.video.filters;
        assert_eq!(filters.brightness, 0.0);
        assert_eq!(filters.contrast, 1.0);
        assert_eq!(filters.saturation, 1.0);
        assert_eq!(filters.hue, 0.0);
        assert_eq!(filters.rotation, 0);
        assert!(!filters.flip_h && !filters.flip_v);
        assert!(filters.crop.is_none());
    }

    #[test]
    fn keep_format_falls_back_to_mkv_for_missing_extension() {
        let choice = QuickExportChoice::keep_format(std::path::Path::new("/movies/clip"));
        match choice.format {
            QuickExportFormat::Video {
                container, codec, ..
            } => {
                assert_eq!(container, "mkv");
                assert_eq!(codec, "copy");
            }
            QuickExportFormat::Audio { .. } => panic!("keep_format must be a video copy"),
        }
    }

    #[test]
    fn keep_format_falls_back_to_mkv_for_empty_extension() {
        let choice = QuickExportChoice::keep_format(std::path::Path::new("/movies/clip."));
        match choice.format {
            QuickExportFormat::Video { container, .. } => assert_eq!(container, "mkv"),
            QuickExportFormat::Audio { .. } => panic!("keep_format must be a video copy"),
        }
    }

    #[cfg(not(miri))]
    #[test]
    fn build_queue_records_segments_join_and_output_path() {
        use crate::converter::queue::SegmentOutputMode;
        let choice = QuickExportChoice::keep_format(std::path::Path::new("/movies/Trip.mp4"));
        let (queue, id, output) = super::build_single_export_queue(
            std::path::PathBuf::from("/movies/Trip.mp4"),
            std::path::PathBuf::from("/movies"),
            &choice,
            &[(2.0, 5.0), (10.0, 12.5)],
        );
        assert!(!output.as_os_str().is_empty());
        let queue_entry = queue
            .items()
            .iter()
            .find(|queue_entry| queue_entry.id == id)
            .expect("queue entry");
        assert_eq!(queue_entry.segments.len(), 2);
        assert_eq!(queue_entry.segments[0].start, Some(2.0));
        assert_eq!(queue_entry.segments[1].end, Some(12.5));
        assert_eq!(queue_entry.segment_mode, SegmentOutputMode::Join);
    }

    #[test]
    fn audio_choice_encodes_and_derives_container() {
        let profile = single_file_export_profile(&QuickExportChoice {
            format: QuickExportFormat::Audio {
                codec: "mp3".into(),
                bitrate: "192k".into(),
            },
            filters: VideoFilters::default(),
        });
        assert_eq!(profile.audio.handling, "encode");
        assert_eq!(profile.audio.codec, "mp3");
        assert_eq!(profile.audio.bitrate, "192k");
        assert_eq!(profile.output.container, "mp3");
    }

    #[test]
    fn quick_export_profiles_use_user_visible_names() {
        let video_profile = single_file_export_profile(&QuickExportChoice {
            format: QuickExportFormat::Video {
                container: "mp4".into(),
                codec: "h264".into(),
                quality: "good".into(),
            },
            filters: VideoFilters::default(),
        });
        let audio_profile = single_file_export_profile(&QuickExportChoice {
            format: QuickExportFormat::Audio {
                codec: "flac".into(),
                bitrate: String::new(),
            },
            filters: VideoFilters::default(),
        });

        assert_eq!(video_profile.profile.name, "Quick Export");
        assert_eq!(audio_profile.profile.name, "Extract Audio");
    }
}
