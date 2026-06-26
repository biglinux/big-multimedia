//! Client-side bridge from the Relm4 converter UI to `big-convertd`.
//!
//! The daemon owns durable FFmpeg execution outside the GTK process. This
//! module only serializes the current pending queue and submits it over the
//! shared microdaemon protocol; UI fallback remains in the shell when the
//! daemon is unavailable.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use big_os_kit::microdaemon::{
    DurableJobSnapshot, DurableJobSpec, MicrodaemonBody, MicrodaemonCommand, MicrodaemonEvent,
    MicrodaemonFrame, MicrodaemonId, decode_microdaemon_frame, encode_microdaemon_frame,
};
use serde::Serialize;

use crate::converter::app::ConverterMode;
use crate::converter::profiles::{CropRect, Profile};
use crate::converter::queue::{QueueItem, SegmentOutputMode, VideoOverrides};
use crate::converter::subtitle::SubtitleInput;

const DAEMON_IO_TIMEOUT: Duration = Duration::from_secs(3);

/// Snapshot of the queue state required by `big-convertd`.
#[derive(Debug, Clone)]
pub struct DaemonConversionRequest {
    /// Audio or video binding.
    pub mode: ConverterMode,
    /// Pending queue items to hand off.
    pub items: Vec<QueueItem>,
    /// Destination directory selected by the user.
    pub output_dir: PathBuf,
    /// Encoding profile selected in the sidebar.
    pub profile: Profile,
    /// Whether the daemon may delete source files after successful output.
    pub delete_source: bool,
    /// Audio-only output naming/routing.
    pub audio_only_mode: bool,
}

/// Accepted daemon job.
#[derive(Debug, Clone)]
pub struct DaemonJobAccepted {
    /// Stable daemon job id.
    pub job_id: String,
    /// Local queue item ids handed off to the daemon.
    pub item_ids: Vec<u64>,
}

/// Client-side daemon submit failure.
#[derive(Debug)]
pub struct DaemonSubmitError {
    message: String,
}

impl DaemonSubmitError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Human-readable diagnostic safe for logs.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for DaemonSubmitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DaemonSubmitError {}

impl From<io::Error> for DaemonSubmitError {
    fn from(error: io::Error) -> Self {
        Self::new(error.to_string())
    }
}

/// Submit the pending conversion batch to `big-convertd`.
///
/// # Errors
///
/// Returns an error when the daemon socket is absent, the protocol fails, or
/// the daemon rejects the job.
pub fn submit_conversion(
    request: DaemonConversionRequest,
) -> Result<DaemonJobAccepted, DaemonSubmitError> {
    let item_ids = request.items.iter().map(|item| item.id).collect::<Vec<_>>();
    let job = build_conversion_job_spec(&request)?;
    let mut stream = UnixStream::connect(big_media_runtime::convertd::default_socket_path())?;
    stream.set_read_timeout(Some(DAEMON_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(DAEMON_IO_TIMEOUT))?;
    write_command(&mut stream, MicrodaemonCommand::Submit { job })?;
    loop {
        match read_event(&mut stream)? {
            MicrodaemonEvent::JobAccepted { job_id } => {
                return Ok(DaemonJobAccepted { job_id, item_ids });
            }
            MicrodaemonEvent::Error { code, message, .. } => {
                return Err(DaemonSubmitError::new(format!("{code}: {message}")));
            }
            MicrodaemonEvent::Hello { .. }
            | MicrodaemonEvent::JobSnapshot { .. }
            | MicrodaemonEvent::JobList { .. }
            | MicrodaemonEvent::Progress { .. }
            | MicrodaemonEvent::JobStateChanged { .. }
            | MicrodaemonEvent::ShutdownAccepted => {}
        }
    }
}

/// Return the latest daemon snapshot for an accepted conversion job.
///
/// # Errors
///
/// Returns an error when the daemon cannot be reached or rejects the attach.
pub fn attach_conversion(job_id: String) -> Result<DurableJobSnapshot, DaemonSubmitError> {
    let event = request_single_event(MicrodaemonCommand::Attach {
        job_id: job_id.clone(),
    })?;
    match event {
        MicrodaemonEvent::JobSnapshot { snapshot } => Ok(snapshot),
        MicrodaemonEvent::Error { code, message, .. } => {
            Err(DaemonSubmitError::new(format!("{code}: {message}")))
        }
        other => Err(DaemonSubmitError::new(format!(
            "unexpected attach response for {job_id}: {other:?}"
        ))),
    }
}

/// Ask `big-convertd` to cancel a conversion job.
///
/// # Errors
///
/// Returns an error when the daemon cannot be reached or rejects the cancel.
pub fn cancel_conversion(job_id: String) -> Result<(), DaemonSubmitError> {
    let event = request_single_event(MicrodaemonCommand::Cancel {
        job_id: job_id.clone(),
    })?;
    match event {
        MicrodaemonEvent::JobStateChanged { .. } => Ok(()),
        MicrodaemonEvent::Error { code, message, .. } => {
            Err(DaemonSubmitError::new(format!("{code}: {message}")))
        }
        other => Err(DaemonSubmitError::new(format!(
            "unexpected cancel response for {job_id}: {other:?}"
        ))),
    }
}

fn request_single_event(
    command: MicrodaemonCommand,
) -> Result<MicrodaemonEvent, DaemonSubmitError> {
    let mut stream = UnixStream::connect(big_media_runtime::convertd::default_socket_path())?;
    stream.set_read_timeout(Some(DAEMON_IO_TIMEOUT))?;
    stream.set_write_timeout(Some(DAEMON_IO_TIMEOUT))?;
    write_command(&mut stream, command)?;
    read_event(&mut stream)
}

fn write_command(stream: &mut UnixStream, command: MicrodaemonCommand) -> io::Result<()> {
    let frame = MicrodaemonFrame::command(MicrodaemonId::BigConvertd, command);
    let encoded = encode_microdaemon_frame(&frame).map_err(io::Error::other)?;
    stream.write_all(&encoded)
}

fn read_event(stream: &mut UnixStream) -> Result<MicrodaemonEvent, DaemonSubmitError> {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix)?;
    let frame_len = u32::from_be_bytes(prefix) as usize;
    let mut bytes = Vec::with_capacity(4 + frame_len);
    bytes.extend_from_slice(&prefix);
    bytes.resize(4 + frame_len, 0);
    stream.read_exact(&mut bytes[4..])?;
    let frame = decode_microdaemon_frame(&bytes)
        .map_err(|error| DaemonSubmitError::new(error.to_string()))?;
    if frame.daemon_id != MicrodaemonId::BigConvertd {
        return Err(DaemonSubmitError::new(
            "response came from the wrong daemon",
        ));
    }
    match frame.body {
        MicrodaemonBody::Event { event } => Ok(event),
        MicrodaemonBody::Command { .. } => Err(DaemonSubmitError::new(
            "daemon returned a command frame instead of an event",
        )),
    }
}

fn build_conversion_job_spec(
    request: &DaemonConversionRequest,
) -> Result<DurableJobSpec, DaemonSubmitError> {
    if request.items.is_empty() {
        return Err(DaemonSubmitError::new("no pending conversion items"));
    }
    let arguments = serde_json::to_value(ConversionDaemonArguments {
        schema_version: 1,
        items: request
            .items
            .iter()
            .map(ConversionDaemonItem::from)
            .collect(),
        output_dir: request.output_dir.clone(),
        profile: request.profile.clone(),
        audio_only_mode: request.audio_only_mode,
        delete_source: request.delete_source,
    })
    .map_err(|error| DaemonSubmitError::new(error.to_string()))?;
    Ok(DurableJobSpec {
        job_key: conversion_job_key(request.mode),
        job_class: conversion_job_class(request.mode, request.items.len()).to_owned(),
        is_durable: true,
        arguments,
    })
}

fn conversion_job_class(mode: ConverterMode, item_count: usize) -> &'static str {
    if item_count > 1 {
        "batch-conversion"
    } else if mode.is_audio() {
        "audio-conversion"
    } else {
        "video-conversion"
    }
}

fn conversion_job_key(mode: ConverterMode) -> String {
    let prefix = if mode.is_audio() {
        "audio-conversion"
    } else {
        "video-conversion"
    };
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{prefix}-{}-{nanos}", std::process::id())
}

#[derive(Serialize)]
struct ConversionDaemonArguments {
    schema_version: u32,
    items: Vec<ConversionDaemonItem>,
    output_dir: PathBuf,
    profile: Profile,
    audio_only_mode: bool,
    delete_source: bool,
}

#[derive(Serialize)]
struct ConversionDaemonItem {
    input: PathBuf,
    segments: Vec<crate::converter::ffmpeg::TrimRange>,
    segment_mode: SegmentOutputMode,
    subtitle: SubtitleInput,
    audio_track: Option<u32>,
    video_track: Option<u32>,
    crop: Option<CropRect>,
    video_overrides: Option<VideoOverrides>,
    input_audio_channels: u32,
    skip_video_filters: bool,
}

impl From<&QueueItem> for ConversionDaemonItem {
    fn from(item: &QueueItem) -> Self {
        Self {
            input: item.input.clone(),
            segments: item.segments.clone(),
            segment_mode: item.segment_mode,
            subtitle: item.subtitle.clone(),
            audio_track: item.audio_track,
            video_track: item.video_track,
            crop: item.crop.clone(),
            video_overrides: item.video_overrides.clone(),
            input_audio_channels: item.input_audio_channels,
            skip_video_filters: item.skip_video_filters,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::profiles::{
        AudioConfig, OutputConfig, ProfileMeta, SubtitleConfig, VideoConfig,
    };
    use crate::converter::queue::ItemStatus;

    fn test_profile() -> Profile {
        Profile {
            profile: ProfileMeta::default(),
            video: VideoConfig::default(),
            audio: AudioConfig::default(),
            subtitles: SubtitleConfig::default(),
            output: OutputConfig::default(),
        }
    }

    fn test_item() -> QueueItem {
        QueueItem {
            id: 42,
            input: PathBuf::from("/tmp/input.mp4"),
            output: PathBuf::from("/tmp/output.mp4"),
            status: ItemStatus::Pending,
            duration_secs: 0.0,
            profile_name: "test".to_owned(),
            segments: vec![crate::converter::ffmpeg::TrimRange {
                start: Some(1.0),
                end: Some(2.0),
            }],
            segment_mode: SegmentOutputMode::Split,
            subtitle: SubtitleInput::ExternalFile(PathBuf::from("/tmp/input.srt")),
            audio_track: Some(2),
            video_track: Some(1),
            crop: Some(CropRect {
                w: 640,
                h: 360,
                x: 10,
                y: 20,
            }),
            video_overrides: Some(VideoOverrides {
                brightness: 0.1,
                contrast: 1.1,
                saturation: 1.2,
                hue: 0.0,
                rotation: 90,
                flip_h: true,
                flip_v: false,
            }),
            input_audio_channels: 2,
            skip_video_filters: true,
            probed: true,
        }
    }

    #[test]
    fn job_spec_preserves_per_item_metadata() {
        let spec = build_conversion_job_spec(&DaemonConversionRequest {
            mode: ConverterMode::Video,
            items: vec![test_item()],
            output_dir: PathBuf::from("/tmp/out"),
            profile: test_profile(),
            delete_source: true,
            audio_only_mode: false,
        })
        .expect("job spec");

        assert_eq!(spec.job_class, "video-conversion");
        assert!(spec.is_durable);
        assert_eq!(
            spec.arguments["items"][0]["segments"][0]["start"],
            serde_json::json!(1.0)
        );
        assert_eq!(
            spec.arguments["items"][0]["segment_mode"],
            serde_json::json!("split")
        );
        assert_eq!(
            spec.arguments["items"][0]["subtitle"]["external-file"],
            serde_json::json!("/tmp/input.srt")
        );
        assert_eq!(
            spec.arguments["items"][0]["audio_track"],
            serde_json::json!(2)
        );
        assert_eq!(
            spec.arguments["items"][0]["video_overrides"]["flip_h"],
            serde_json::json!(true)
        );
        assert_eq!(spec.arguments["delete_source"], serde_json::json!(true));
    }

    #[test]
    fn empty_request_is_rejected() {
        let err = build_conversion_job_spec(&DaemonConversionRequest {
            mode: ConverterMode::Video,
            items: vec![],
            output_dir: PathBuf::from("/tmp/out"),
            profile: test_profile(),
            delete_source: false,
            audio_only_mode: false,
        })
        .expect_err("empty queue must error");
        assert!(err.message().contains("no pending"));
    }

    #[test]
    fn job_class_distinguishes_batch_audio_video() {
        // More than one item is always a batch, regardless of mode.
        assert_eq!(
            conversion_job_class(ConverterMode::Video, 2),
            "batch-conversion"
        );
        assert_eq!(
            conversion_job_class(ConverterMode::Audio, 5),
            "batch-conversion"
        );
        // Single item routes by mode.
        assert_eq!(
            conversion_job_class(ConverterMode::Audio, 1),
            "audio-conversion"
        );
        assert_eq!(
            conversion_job_class(ConverterMode::Video, 1),
            "video-conversion"
        );
    }

    #[test]
    fn job_key_prefix_follows_mode() {
        assert!(conversion_job_key(ConverterMode::Audio).starts_with("audio-conversion-"));
        assert!(conversion_job_key(ConverterMode::Video).starts_with("video-conversion-"));
    }

    #[test]
    fn submit_error_exposes_message_via_accessor_and_display() {
        let err = DaemonSubmitError::new("boom");
        assert_eq!(err.message(), "boom");
        assert!(format!("{err}").contains("boom"));
    }
}
