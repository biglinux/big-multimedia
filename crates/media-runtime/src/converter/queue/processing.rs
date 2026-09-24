//! Private staged exports. Every expected stream and duration is checked before
//! no-replace publication. Published files and originals are never error cleanup.
//! Split publication is per-file, not an all-files atomic transaction.
use super::job::run_job;
use super::receipts::{self, Receipt};
use super::support::{available_disk_space, ensure_entry_probed, segment_output_path};
use super::validation::{Expectation, validate_output_cancellable, validate_ranges};
use super::{ConversionQueueEntry, ConversionResumeContext, QueueMessage, SegmentOutputMode};
use crate::cast::gpu::{GpuDevice, GpuType};
use crate::converter::ffmpeg::{FfmpegJob, GpuStage, TrimRange};
use crate::converter::profiles::Profile;
use crate::converter::{ffmpeg::progress, subtitle};
use crate::error::{Error, Result};
use crate::i18n::i18n;
use big_os_kit::storage::StagedFile;
use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};
use log::warn;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

// Pre-flight: 1.5× input as headroom. Indeterminate size/space → let
// ffmpeg fail later instead of blocking here.
fn require_disk_space(entry: &ConversionQueueEntry) -> Result<()> {
    let Some(parent) = entry.output.parent() else {
        return Ok(());
    };
    let input = std::fs::metadata(&entry.input)?;
    let needed = input.len().saturating_add(input.len() / 2);
    let Some(available) = available_disk_space(parent) else {
        return Ok(());
    };
    if available < needed {
        return Err(failure(&format!(
            "{}: {} ~{}, {} {}",
            i18n("Insufficient disk space"),
            i18n("need"),
            decimal_compact(needed),
            i18n("available"),
            decimal_compact(available)
        )));
    }
    Ok(())
}

fn build_stage_list(gpu: &GpuDevice, codec: &str, audio_only: bool) -> Vec<(GpuStage, GpuDevice)> {
    let cpu = || GpuDevice {
        gpu_type: GpuType::Cpu,
        name: "CPU".into(),
        device_path: String::new(),
    };
    if audio_only || gpu.gpu_type.video_encoder(codec).is_none() {
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

fn recheck_plan(
    entry: &ConversionQueueEntry,
    profile: &Profile,
    segments: &[TrimRange],
    source: &str,
    expected_plan: &str,
    audio_only: bool,
    cancel: &AtomicBool,
) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    if receipts::hash_file(&entry.input, cancel)? != source
        || receipts::plan(entry, profile, segments, source, audio_only, cancel)? != expected_plan
    {
        return Err(failure(&i18n(
            "The source or external subtitles changed. Existing exports were kept; review the plan again.",
        )));
    }
    Ok(())
}

fn failure(message: &str) -> Error {
    Error::ffmpeg("ffmpeg", None, message.to_owned())
}

fn convert_one(
    entry: &mut ConversionQueueEntry,
    profile: &Profile,
    gpu: &GpuDevice,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<QueueMessage>,
    audio_only: bool,
    resume: bool,
) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    ensure_entry_probed(entry);
    let source = std::fs::metadata(&entry.input)?;
    if !source.is_file() {
        return Err(failure(&i18n(
            "Export requires a regular local source file",
        )));
    }
    let source_hash = receipts::hash_file(&entry.input, cancel)?;
    let input = progress::probe_media_metadata_cancellable(&entry.input, cancel)?;
    entry.duration_secs = super::validation::number(input.pointer("/format/duration"))
        .filter(|n| *n > 0.0)
        .unwrap_or(0.0);
    // Probe failure is not an empty successful media record.
    let segments = if entry.segments.is_empty() {
        vec![TrimRange::default()]
    } else {
        entry.segments.clone()
    };
    validate_ranges(&segments, entry.duration_secs)?;
    let speed = profile.audio.effects.speed;
    if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
        return Err(failure(&i18n("Choose a playback speed between 0.25 and 4")));
    }
    if !audio_only && profile.audio.handling != "none" && (speed - 1.0).abs() > 0.001 {
        return Err(failure(&i18n(
            "Changing audio speed in a video needs matching video timing; use normal speed or audio-only export",
        )));
    }
    let needs_concat = segments.len() > 1 && entry.segment_mode == SegmentOutputMode::Join;
    let split = entry.segment_mode == SegmentOutputMode::Split;
    let finals = resolve_final_outputs(entry, segments.len(), needs_concat, split);
    if finals.len() > 128 {
        return Err(failure(&i18n(
            "Export at most 128 separate outputs per job.",
        )));
    }
    let parent = entry
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let subtitles = subtitle::probe_subtitles(&entry.input);
    let fingerprint = receipts::plan(entry, profile, &segments, &source_hash, audio_only, cancel)?;
    let base_expectation = Expectation::for_entry(entry, profile, audio_only, &input, &subtitles)?;
    let mut expectations = Vec::with_capacity(segments.len());
    let mut expected_total = 0.0;
    for segment in &segments {
        let mut expected = base_expectation.clone();
        expected.duration = segment.effective_duration(entry.duration_secs);
        if audio_only && profile.audio.handling != "copy" {
            expected.duration /= speed;
        }
        expected_total += expected.duration;
        if !expected_total.is_finite() {
            return Err(failure(&i18n("Invalid total export duration")));
        }
        expectations.push(expected);
    }
    let output_expectations = if needs_concat {
        let mut expected = base_expectation.clone();
        expected.duration = expected_total;
        vec![expected]
    } else {
        expectations.clone()
    };
    let mut receipts = Vec::new();
    let mut reused = Vec::new();
    // Hold all per-output locks before conversion; this also rejects duplicate
    // target names without doing expensive conversion work.
    for (target, expected) in finals.iter().zip(&output_expectations) {
        let receipt = Receipt::open(target, &fingerprint)?;
        reused.push(receipt.completed(expected, cancel, resume)?);
        receipts.push(receipt);
    }
    if reused.iter().all(|&done| done) {
        recheck_plan(
            entry,
            profile,
            &segments,
            &source_hash,
            &fingerprint,
            audio_only,
            cancel,
        )?;
        let _ = sender.try_send(QueueMessage::StderrLine {
            entry_id: entry.id,
            line: i18n("Verified completed export reused: source, plan and output receipts match."),
        });
        return Ok(());
    }
    require_disk_space(entry)?;
    let stages = build_stage_list(gpu, &profile.video.codec, audio_only);
    let mut temporary = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if split && reused[index] {
            continue;
        }
        let desired = if split { &finals[index] } else { &entry.output };
        let stage = StagedFile::new(desired)?;
        let expectation = &expectations[index];
        let mut last = None;
        let mut success = false;
        for (attempt, (gpu_stage, device)) in stages.iter().enumerate() {
            if cancel.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if attempt > 0 {
                let name = match gpu_stage {
                    GpuStage::FullGpu => "full-gpu",
                    GpuStage::PartialGpu => "partial-gpu",
                    GpuStage::Software => "software",
                };
                let _ = sender.try_send(QueueMessage::GpuFallback {
                    entry_id: entry.id,
                    stage: name,
                });
            }
            let job = FfmpegJob {
                input: entry.input.clone(),
                output: stage.path().to_path_buf(),
                profile: profile.clone(),
                gpu: device.clone(),
                stage: *gpu_stage,
                trim: segment.clone(),
                subtitle: entry.subtitle.clone(),
                audio_track: entry.audio_track,
                video_track: entry.video_track,
                crop: entry.crop.clone(),
                video_overrides: entry.video_overrides.clone(),
                input_audio_channels: entry.input_audio_channels,
                subtitle_streams: subtitles.clone(),
                audio_only,
            };
            match run_job(&job, entry, cancel, sender)
                .and_then(|()| validate_output_cancellable(stage.path(), expectation, cancel))
            {
                Ok(()) => {
                    success = true;
                    break;
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(error) => last = Some(error),
            }
        }
        if !success {
            return Err(last.unwrap_or_else(|| failure(&i18n("No conversion stage completed"))));
        }
        temporary.push((index, stage));
    }
    let mut publications = Vec::new();
    if needs_concat {
        let joined = StagedFile::new(&entry.output)?;
        concat_segments(&temporary, joined.path(), cancel)?;
        validate_output_cancellable(joined.path(), &output_expectations[0], cancel)?;
        publications.push((0, joined));
        // Each segment and its private directory is removed by its own guard.
    } else {
        publications.extend(temporary);
    }
    let after = std::fs::metadata(&entry.input)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if source.dev() != after.dev() || source.ino() != after.ino() {
            return Err(failure(&i18n(
                "Source was replaced during export; no result was published",
            )));
        }
    }
    if source.len() != after.len() || source.modified()? != after.modified()? {
        return Err(failure(&i18n(
            "Source changed during export; no result was published",
        )));
    }
    recheck_plan(
        entry,
        profile,
        &segments,
        &source_hash,
        &fingerprint,
        audio_only,
        cancel,
    )?;
    for (index, stage) in publications {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        receipts[index].publish(&stage, cancel)?;
    }
    Ok(())
}

pub(crate) fn run_conversion_queue_worker(
    mut items: Vec<ConversionQueueEntry>,
    profile: &Profile,
    gpu: &GpuDevice,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<QueueMessage>,
    resume_context: Option<ConversionResumeContext>,
    delete_source: bool,
    audio_only_mode: bool,
) {
    // Resume is allowed only through exact persistent receipts. The namespace
    // is a policy marker; it is not substituted for source or output digests.
    let resume = resume_context
        .as_ref()
        .is_some_and(|context| context.job_key == "verified-receipt-v1");
    if delete_source {
        let _ = sender.send_blocking(QueueMessage::StderrLine {
            entry_id: 0,
            line: i18n("Originals are kept. Verify the exported files before deleting anything."),
        });
    }
    for entry in &mut items {
        if cancel.load(Ordering::Acquire) {
            break;
        }
        match convert_one(entry, profile, gpu, cancel, sender, audio_only_mode, resume) {
            Ok(()) => {
                let _ = sender.send_blocking(QueueMessage::Done { entry_id: entry.id });
            }
            Err(Error::Cancelled) => {
                let _ = sender.send_blocking(QueueMessage::Cancelled { entry_id: entry.id });
                break;
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
    let path = path
        .to_str()
        .ok_or_else(|| failure(&i18n("This concat list needs UTF-8 paths")))?;
    if path.bytes().any(|b| matches!(b, 0 | b'\r' | b'\n')) {
        return Err(failure(&i18n("Invalid concat path")));
    }
    Ok(format!("file '{}'\n", path.replace('\'', "'\\''")))
}
fn concat_segments(
    segments: &[(usize, StagedFile)],
    output: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    use std::io::Write;
    let list = StagedFile::new(&output.with_file_name("concat.ffconcat"))?;
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(list.path())?;
        for (_, segment) in segments {
            file.write_all(concat_line(&std::fs::canonicalize(segment.path())?)?.as_bytes())?;
        }
    }
    // The files already contain per-segment timing/subtitle selection. Mapping
    // all streams from the concat demuxer avoids duplicate manually merged subs.
    let result = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(list.path())
        .args(["-map", "0", "-c", "copy", "--"])
        .arg(output)
        .stdout(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_hours(2))
        .build()
        .run_cancellable(cancel)
        .map_err(|e| match e {
            big_os_kit::subprocess::BigSubprocessError::Cancelled => Error::Cancelled,
            _ => failure(&i18n("Could not complete concatenation")),
        })?;
    if result.status.success() {
        Ok(())
    } else {
        Err(failure(&i18n("Could not complete concatenation")))
    }
}

#[cfg(test)]
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
        let cpu_stages = build_stage_list(&cpu, "h264", false);
        assert_eq!(cpu_stages.len(), 1);
        assert_eq!(cpu_stages[0].0, GpuStage::Software);

        let gpu = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "RTX".into(),
            device_path: "/dev/dri/renderD128".into(),
        };
        let gpu_stages = build_stage_list(&gpu, "h264", false);
        assert_eq!(gpu_stages.len(), 3);
        assert_eq!(gpu_stages[0].0, GpuStage::FullGpu);
        assert_eq!(gpu_stages[1].0, GpuStage::PartialGpu);
        assert_eq!(gpu_stages[2].0, GpuStage::Software);
        // The hardware stages keep the real device; the fallback is CPU.
        assert!(matches!(gpu_stages[0].1.gpu_type, GpuType::Nvidia));
        assert!(matches!(gpu_stages[2].1.gpu_type, GpuType::Cpu));
        for (codec, audio_only) in [
            ("vp9", false),
            ("prores", false),
            ("copy", false),
            ("h264", true),
        ] {
            let stages = build_stage_list(&gpu, codec, audio_only);
            assert_eq!(stages.len(), 1, "{codec} audio={audio_only}");
            assert_eq!(stages[0].0, GpuStage::Software);
            assert_eq!(stages[0].1.gpu_type, GpuType::Cpu);
        }
    }

    #[test]
    #[cfg(not(miri))]
    #[ignore = "requires FFmpeg with libx265, libx264, libvpx-vp9 and ProRes"]
    fn native_hevc10_exports_preserve_filters_and_verified_outputs() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("hevc10.mkv");
        let generated = BigSubprocessSpec::builder().program("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=red:size=640x360:duration=0.12:rate=25,drawbox=x=320:y=0:w=320:h=360:color=blue:t=fill", "-pix_fmt", "yuv420p10le", "-c:v", "libx265", "-x265-params", "pools=1:frame-threads=1:log-level=error"])
            .arg(&input).timeout(Duration::from_secs(15)).build().run().unwrap();
        assert!(generated.status.success(), "{}", generated.stderr_lossy());
        let cancel = Arc::new(AtomicBool::new(false));
        let gpu = GpuDevice {
            gpu_type: GpuType::Nvidia,
            name: "Unavailable test GPU".into(),
            device_path: "9999".into(),
        };
        let (tx, rx) = async_channel::unbounded();
        for (codec, container, pixel_format) in [
            ("h264", "mp4", "auto"),
            ("h264", "mkv", "yuv444p10le"),
            ("vp9", "webm", "auto"),
            ("prores", "mov", "auto"),
            ("copy", "mkv", "auto"),
        ] {
            let mut profile: Profile = toml::from_str("").unwrap();
            profile.video.codec = codec.into();
            profile.video.pixel_format = pixel_format.into();
            profile.video.filters.flip_h = codec != "copy";
            profile.output.container = container.into();
            let mut entry = queue_entry("unused");
            entry.input = input.clone();
            entry.output = temp
                .path()
                .join(format!("export-{codec}-{pixel_format}.{container}"));
            entry.probed = false;
            convert_one(&mut entry, &profile, &gpu, &cancel, &tx, false, true)
                .unwrap_or_else(|e| panic!("{codec}: {e}"));
            let messages: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
            let attempts = messages
                .iter()
                .filter(|m| matches!(m, QueueMessage::Started { .. }))
                .count();
            assert_eq!(attempts, if codec == "h264" { 3 } else { 1 }, "{codec}");
            if codec == "h264" {
                let metadata =
                    progress::probe_media_metadata_cancellable(&entry.output, &cancel).unwrap();
                let expected_format = if pixel_format == "auto" {
                    "yuv420p"
                } else {
                    pixel_format
                };
                assert_eq!(metadata["streams"][0]["pix_fmt"], expected_format);
            }
            let decoded = BigSubprocessSpec::builder()
                .program("ffmpeg")
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
                .arg(&entry.output)
                .args(["-frames:v", "1", "-pix_fmt", "rgb24", "-f", "rawvideo", "-"])
                .timeout(Duration::from_secs(10))
                .build()
                .run()
                .unwrap();
            assert!(
                decoded.status.success(),
                "{codec}: {}",
                decoded.stderr_lossy()
            );
            assert_eq!(decoded.stdout.len(), 640 * 360 * 3);
            let left = (180 * 640 + 160) * 3;
            let right = (180 * 640 + 480) * 3;
            let (red, blue) = if codec == "copy" {
                (left, right)
            } else {
                (right, left)
            };
            assert!(
                decoded.stdout[red] > 200 && decoded.stdout[red + 2] < 30,
                "{codec}: red"
            );
            assert!(
                decoded.stdout[blue + 2] > 200 && decoded.stdout[blue] < 30,
                "{codec}: blue"
            );
            let digest = receipts::hash_file(&entry.output, &cancel).unwrap();
            convert_one(&mut entry, &profile, &gpu, &cancel, &tx, false, true).unwrap();
            assert!(
                !std::iter::from_fn(|| rx.try_recv().ok())
                    .any(|m| matches!(m, QueueMessage::Started { .. })),
                "resume should reuse the verified {codec} output"
            );
            profile.video.filters.flip_h = !profile.video.filters.flip_h;
            assert!(
                convert_one(&mut entry, &profile, &gpu, &cancel, &tx, false, true).is_err(),
                "changed {codec} plan must not reuse output"
            );
            assert_eq!(receipts::hash_file(&entry.output, &cancel).unwrap(), digest);
            eprintln!(
                "{codec} {pixel_format}: {attempts} attempts; pixels, verified resume and changed-plan refusal passed"
            );
        }
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
