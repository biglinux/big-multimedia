//! Private staged exports. Every expected stream and duration is checked before
//! no-replace publication. Published files and originals are never error cleanup.
//! Split publication is per-file, not an all-files atomic transaction.
use crate::cast::gpu::{GpuDevice, GpuType};
use crate::converter::ffmpeg::{FfmpegJob, GpuStage, TrimRange};
use crate::converter::profiles::Profile;
use crate::converter::{ffmpeg::progress, subtitle};
use crate::i18n::i18n;
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use big_os_kit::subprocess::{BigSubprocessSpec, BigSubprocessOutputMode};
use big_os_kit::storage::{StagedFile, CommitOutcome};
use super::job::run_job;
use super::support::{available_disk_space, ensure_entry_probed, segment_output_path};
use super::{ConversionQueueEntry, ConversionResumeContext, QueueMessage, SegmentOutputMode};
use crate::error::{Error, Result};
use super::validation::{Expectation, validate_ranges, validate_output};

// Pre-flight: 1.5× input as headroom. Indeterminate size/space → let
// ffmpeg fail later instead of blocking here.
fn has_enough_disk_space(
    queue_entry: &ConversionQueueEntry,
    sender: &async_channel::Sender<QueueMessage>,
) -> bool {
    let Some(parent) = queue_entry.output.parent() else {
        return true;
    };
    let Ok(input_meta) = std::fs::metadata(&queue_entry.input) else {
        return true;
    };
    let needed = input_meta.len().saturating_add(input_meta.len() / 2);
    let Some(available) = available_disk_space(parent) else {
        return true;
    };
    if available >= needed {
        return true;
    }
    let _ = sender.send_blocking(QueueMessage::Failed {
        entry_id: queue_entry.id,
        error: format!(
            "{}: {} ~{}, {} {}",
            i18n("Insufficient disk space"),
            i18n("need"),
            decimal_compact(needed),
            i18n("available"),
            decimal_compact(available),
        ),
    });
    false
}

// MP4 + copy mode: ffmpeg would re-mux-refuse non-MP4-compatible codecs;
// catch them up front so the user gets a clear error.
fn is_mp4_copy_compatible(
    queue_entry: &ConversionQueueEntry,
    profile: &Profile,
    sender: &async_channel::Sender<QueueMessage>,
) -> bool {
    if profile.video.codec != "copy" {
        return true;
    }
    let out_ext = queue_entry
        .output
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    if out_ext != "mp4" {
        return true;
    }
    let Some(media_metadata) = progress::probe_media_metadata(&queue_entry.input) else {
        let _ = sender.send_blocking(QueueMessage::Failed { entry_id: queue_entry.id, error: i18n("Could not verify source streams") });
        return false;
    };
    let Some(streams) = media_metadata.get("streams").and_then(|v| v.as_array()) else {
        return true;
    };
    let incompat = streams.iter().find(|stream| {
        let codec = stream
            .get("codec_name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match stream.get("codec_type").and_then(|v| v.as_str()) {
            Some(codec_type) => !is_mp4_copy_compatible_stream(codec_type, codec),
            _ => false,
        }
    });
    let Some(bad) = incompat else {
        return true;
    };
    let codec = bad
        .get("codec_name")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
    let _ = sender.send_blocking(QueueMessage::Failed {
        entry_id: queue_entry.id,
        error: format!(
            "{} '{codec}' {}",
            i18n("Codec"),
            i18n("is not compatible with MP4 container in copy mode"),
        ),
    });
    false
}

fn build_stage_list(gpu: &GpuDevice) -> Vec<(GpuStage, GpuDevice)> {
    let cpu = || GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU".into(),
        device_path: String::new(),
    };
    if matches!(gpu.gpu_type, GpuType::Cpu) {
        vec![(GpuStage::Software, cpu())]
    } else {
        vec![
            (GpuStage::FullGpu, gpu.clone()),
            (GpuStage::PartialGpu, gpu.clone()),
            (GpuStage::Software, cpu()),
        ]
    }
}

fn resolve_final_outputs(
    queue_entry: &ConversionQueueEntry,
    total_segments: usize,
    needs_concat: bool,
    split_mode: bool,
) -> Vec<PathBuf> {
    if needs_concat || total_segments == 1 || !split_mode {
        return vec![queue_entry.output.clone()];
    }
    (0..total_segments)
        .map(|segment_idx| {
            segment_output_path(
                &queue_entry.output,
                segment_idx,
                total_segments,
                &queue_entry.input,
            )
        })
        .collect()
}


fn failure(message: &str) -> Error { Error::ffmpeg("ffmpeg", None, message.to_owned()) }

fn convert_one(entry: &mut ConversionQueueEntry, profile: &Profile, gpu: &GpuDevice,
    cancel: &Arc<AtomicBool>, sender: &async_channel::Sender<QueueMessage>, audio_only: bool) -> Result<()> {
    if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
    ensure_entry_probed(entry, &profile.video.codec);
    let source = std::fs::metadata(&entry.input)?;
    if !source.is_file() { return Err(failure(&i18n("Export requires a regular local source file"))); }
    let input = progress::probe_media_metadata(&entry.input)
        .ok_or_else(|| failure(&i18n("Could not verify the source media")))?;
    entry.duration_secs = super::validation::number(input.pointer("/format/duration")).filter(|n| *n > 0.0).unwrap_or(0.0);
    // Probe failure is not an empty successful media record.
    let segments = if entry.segments.is_empty() { vec![TrimRange::default()] } else { entry.segments.clone() };
    validate_ranges(&segments, entry.duration_secs)?;
    let speed = profile.audio.effects.speed;
    if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
        return Err(failure(&i18n("Choose a playback speed between 0.25 and 4")));
    }
    if !audio_only && profile.audio.handling != "none" && (speed - 1.0).abs() > 0.001 {
        return Err(failure(&i18n("Changing audio speed in a video needs matching video timing; use normal speed or audio-only export")));
    }
    let needs_concat = segments.len() > 1 && entry.segment_mode == SegmentOutputMode::Join;
    let split = entry.segment_mode == SegmentOutputMode::Split;
    let finals = resolve_final_outputs(entry, segments.len(), needs_concat, split);
    if finals.iter().any(|p| std::fs::symlink_metadata(p).is_ok()) {
        return Err(failure(&i18n("An output already exists. Choose a new name; it will not be replaced")));
    }
    let parent = entry.output.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let subtitles = subtitle::probe_subtitles(&entry.input);
    let stages = build_stage_list(gpu);
    let mut temporary = Vec::new();
    let mut expected_total = 0.0;
    let mut common_expectation = None;
    for (index, segment) in segments.iter().enumerate() {
        if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
        let desired = if split { &finals[index] } else { &entry.output };
        let stage = StagedFile::new(desired)?;
        let mut expectation = Expectation::for_entry(entry, profile, audio_only, &input, &subtitles)?;
        expectation.duration = segment.effective_duration(entry.duration_secs);
        if audio_only && profile.audio.handling != "copy" { expectation.duration /= speed; }
        expected_total += expectation.duration;
        if !expected_total.is_finite() { return Err(failure(&i18n("Invalid total export duration"))); }
        let mut last = None;
        let mut success = false;
        for (attempt, (gpu_stage, device)) in stages.iter().enumerate() {
            if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
            if attempt > 0 {
                let name = match gpu_stage { GpuStage::FullGpu => "full-gpu", GpuStage::PartialGpu => "partial-gpu", GpuStage::Software => "software" };
                let _ = sender.try_send(QueueMessage::GpuFallback { entry_id: entry.id, stage: name });
            }
            let job = FfmpegJob {
                input: entry.input.clone(), output: stage.path().to_path_buf(),
                profile: profile.clone(), gpu: device.clone(), stage: *gpu_stage,
                trim: segment.clone(), subtitle: entry.subtitle.clone(),
                audio_track: entry.audio_track, video_track: entry.video_track,
                crop: entry.crop.clone(), video_overrides: entry.video_overrides.clone(),
                input_audio_channels: entry.input_audio_channels, subtitle_streams: subtitles.clone(),
                skip_video_filters: entry.skip_video_filters, audio_only,
            };
            match run_job(&job, entry, cancel, sender).and_then(|()| validate_output(stage.path(), &expectation)) {
                Ok(()) => { success = true; break; }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => last = Some(error),
            }
        }
        if !success { return Err(last.unwrap_or_else(|| failure(&i18n("No conversion stage completed")))); }
        common_expectation = Some(expectation);
        temporary.push(stage);
    }
    let mut publications = Vec::new();
    if needs_concat {
        let joined = StagedFile::new(&entry.output)?;
        concat_segments(&temporary, joined.path(), cancel)?;
        let mut expected = common_expectation.ok_or_else(|| failure(&i18n("No output was generated")))?;
        expected.duration = expected_total;
        validate_output(joined.path(), &expected)?;
        publications.push((joined, entry.output.clone()));
        // Each segment and its private directory is removed by its own guard.
    } else {
        for (stage, target) in temporary.into_iter().zip(finals) { publications.push((stage, target)); }
    }
    let after = std::fs::metadata(&entry.input)?;
    #[cfg(unix)] {
        use std::os::unix::fs::MetadataExt;
        if source.dev() != after.dev() || source.ino() != after.ino() {
            return Err(failure(&i18n("Source was replaced during export; no result was published")));
        }
    }
    if source.len() != after.len() || source.modified()? != after.modified()? {
        return Err(failure(&i18n("Source changed during export; no result was published")));
    }
    for (stage, target) in publications {
        if cancel.load(Ordering::Acquire) { return Err(Error::Cancelled); }
        match stage.publish(&target)? {
            CommitOutcome::Durable => {},
            CommitOutcome::PublishedNotSynced(_) => {
                // Publication already happened. Never retry over or delete it.
                return Err(failure(&i18n("Output was created, but durability could not be confirmed. Keep the source and verify the output")));
            }
        }
    }
    Ok(())
}

pub(crate) fn run_conversion_queue_worker(mut items: Vec<ConversionQueueEntry>, profile: &Profile,
    gpu: &GpuDevice, cancel: &Arc<AtomicBool>, sender: &async_channel::Sender<QueueMessage>,
    resume_context: Option<ConversionResumeContext>, delete_source: bool, audio_only_mode: bool) {
    // Existence alone is not a resume receipt. Until a versioned input+plan+output
    // journal is available, refuse pre-existing destinations instead of silently
    // counting an unrelated or truncated file as a completed job.
    if resume_context.is_some() { info!("Resume verifies jobs normally; existing outputs are not inferred as complete"); }
    if delete_source {
        let _ = sender.send_blocking(QueueMessage::StderrLine { entry_id: 0,
            line: i18n("Originals are kept. Verify the exported files before deleting anything.") });
    }
    for entry in &mut items {
        if cancel.load(Ordering::Acquire) { break; }
        if !has_enough_disk_space(entry, sender) || !is_mp4_copy_compatible(entry, profile, sender) { continue; }
        match convert_one(entry, profile, gpu, cancel, sender, audio_only_mode) {
            Ok(()) => { let _ = sender.send_blocking(QueueMessage::Done { entry_id: entry.id }); }
            Err(Error::Cancelled) => {
                let _ = sender.send_blocking(QueueMessage::Cancelled { entry_id: entry.id }); break;
            }
            Err(error) => {
                warn!("Export did not complete: {error}");
                let _ = sender.send_blocking(QueueMessage::Failed { entry_id: entry.id,
                    error: format!("{}: {error}", i18n("Export incomplete. Any files already published were kept; the source is unchanged")) });
            }
        }
    }
}

fn concat_line(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or_else(|| failure(&i18n("This concat list needs UTF-8 paths")))?;
    if path.bytes().any(|b| matches!(b, 0 | b'\r' | b'\n')) { return Err(failure(&i18n("Invalid concat path"))); }
    Ok(format!("file '{}'\n", path.replace('\'', "'\\''")))
}
fn concat_segments(segments: &[StagedFile], output: &Path, cancel: &AtomicBool) -> Result<()> {
    use std::io::Write;
    let list = StagedFile::new(&output.with_file_name("concat.ffconcat"))?;
    {
        let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(list.path())?;
        for segment in segments { file.write_all(concat_line(&std::fs::canonicalize(segment.path())?)?.as_bytes())?; }
    }
    // The files already contain per-segment timing/subtitle selection. Mapping
    // all streams from the concat demuxer avoids duplicate manually merged subs.
    let result = BigSubprocessSpec::builder().program("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(list.path()).args(["-map", "0", "-c", "copy", "--"]).arg(output)
        .stdout(BigSubprocessOutputMode::Null).timeout(Duration::from_secs(2 * 60 * 60)).build()
        .run_cancellable(cancel).map_err(|e| match e {
            big_os_kit::subprocess::BigSubprocessError::Cancelled => Error::Cancelled,
            _ => failure(&i18n("Could not complete concatenation")),
        })?;
    if result.status.success() { Ok(()) } else { Err(failure(&i18n("Could not complete concatenation"))) }
}

fn is_mp4_copy_compatible_stream(codec_type: &str, codec: &str) -> bool {
    const MP4_VIDEO_CODECS: &[&str] = &["h264", "hevc", "h265", "mpeg4", "av1"];
    const MP4_AUDIO_CODECS: &[&str] = &["aac", "mp3", "ac3", "eac3", "flac", "opus", "alac"];

    match codec_type {
        "video" => MP4_VIDEO_CODECS.contains(&codec),
        "audio" => MP4_AUDIO_CODECS.contains(&codec),
        _ => true,
    }
}

fn decimal_compact(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod processing_tests {
    use super::*;

    #[test]
    fn decimal_compact_scales_by_binary_thresholds() {
        // Uses 1024-based units but decimal labels; the boundary is `>= unit`.
        assert_eq!(decimal_compact(0), "0 B");
        assert_eq!(decimal_compact(1023), "1023 B");
        assert_eq!(decimal_compact(1024), "1 KB");
        assert_eq!(decimal_compact(1024 * 1024 - 1), "1024 KB");
        assert_eq!(decimal_compact(1024 * 1024), "1.0 MB");
        assert_eq!(decimal_compact(3 * 1024 * 1024 + 512 * 1024), "3.5 MB");
        assert_eq!(decimal_compact(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(decimal_compact(2 * 1024 * 1024 * 1024), "2.0 GB");
    }

    #[test]
    fn mp4_copy_compatibility_per_stream_type() {
        // Known MP4-friendly video/audio codecs pass; foreign ones fail.
        assert!(is_mp4_copy_compatible_stream("video", "h264"));
        assert!(is_mp4_copy_compatible_stream("video", "av1"));
        assert!(!is_mp4_copy_compatible_stream("video", "vp9"));
        assert!(is_mp4_copy_compatible_stream("audio", "aac"));
        assert!(is_mp4_copy_compatible_stream("audio", "opus"));
        assert!(!is_mp4_copy_compatible_stream("audio", "pcm_s16le"));
        // Non-AV stream types (subtitle/data) are passed through as compatible.
        assert!(is_mp4_copy_compatible_stream("subtitle", "mov_text"));
        assert!(is_mp4_copy_compatible_stream("data", "bin_data"));
    }

    #[test]
    fn build_stage_list_branches_on_gpu_type() {
        let cpu = GpuDevice {
            gpu_type: GpuType::Cpu,
            name: "CPU".into(),
            device_path: String::new(),
        };
        let cpu_stages = build_stage_list(&cpu);
        assert_eq!(cpu_stages.len(), 1);
        assert_eq!(cpu_stages[0].0, GpuStage::Software);

        let gpu = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "RTX".into(),
            device_path: "/dev/dri/renderD128".into(),
        };
        let gpu_stages = build_stage_list(&gpu);
        assert_eq!(gpu_stages.len(), 3);
        assert_eq!(gpu_stages[0].0, GpuStage::FullGpu);
        assert_eq!(gpu_stages[1].0, GpuStage::PartialGpu);
        assert_eq!(gpu_stages[2].0, GpuStage::Software);
        // The hardware stages keep the real device; the fallback is CPU.
        assert!(matches!(gpu_stages[0].1.gpu_type, GpuType::Nvidia));
        assert!(matches!(gpu_stages[2].1.gpu_type, GpuType::Cpu));
    }

    fn queue_entry(output: &str) -> ConversionQueueEntry {
        ConversionQueueEntry {
            id: 1,
            input: PathBuf::from("/tmp/bbt_resolve_src/in.mkv"),
            output: PathBuf::from(output),
            status: super::super::types::ConversionStatus::Pending,
            duration_secs: 0.0,
            profile_name: String::new(),
            segments: Vec::new(),
            segment_mode: SegmentOutputMode::Split,
            subtitle: crate::converter::subtitle::SubtitleInput::None,
            audio_track: None,
            video_track: None,
            crop: None,
            video_overrides: None,
            input_audio_channels: 2,
            skip_video_filters: false,
            probed: true,
        }
    }

    #[test]
    fn resolve_final_outputs_lists_one_or_per_segment() {
        let it = queue_entry("/tmp/bbt_resolve_dst/movie.mp4");
        // Concat, single segment, or join-mode all collapse to the one output.
        assert_eq!(
            resolve_final_outputs(&it, 3, true, true),
            vec![it.output.clone()]
        );
        assert_eq!(
            resolve_final_outputs(&it, 1, false, true),
            vec![it.output.clone()]
        );
        assert_eq!(
            resolve_final_outputs(&it, 3, false, false),
            vec![it.output.clone()]
        );
        // Split, multi-segment → one path per segment.
        let parts = resolve_final_outputs(&it, 3, false, true);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[0],
            PathBuf::from("/tmp/bbt_resolve_dst/movie_part01.mp4")
        );
        assert_eq!(
            parts[2],
            PathBuf::from("/tmp/bbt_resolve_dst/movie_part03.mp4")
        );
    }
}
