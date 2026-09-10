//! Queue worker pipeline: per-queue_entry disk-space + MP4 copy-mode pre-flight,
//! GPU-stage fallback (FullGpu → PartialGpu → Software), per-segment ffmpeg
//! invocation, optional concat (join mode) or numbered split, source delete
//! gated by ≥1/7 input/output ratio. Concat embeds per-segment merged subs
//! through the ffmpeg concat demuxer.

use crate::cast::gpu::{GpuDevice, GpuType};
use crate::converter::ffmpeg::{FfmpegJob, GpuStage, TrimRange};
use crate::converter::profiles::Profile;
use crate::converter::{ffmpeg::progress, subtitle};
use crate::i18n::i18n;
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

use super::job::run_job;
use super::support::{
    available_disk_space, ensure_entry_probed, is_resume_output_complete, promote_resume_output,
    resume_partial_output_path, segment_output_path,
};
use super::{ConversionQueueEntry, ConversionResumeContext, QueueMessage, SegmentOutputMode};
use crate::error::{Error, Result};

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
    let needed = input_meta.len() * 3 / 2;
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
        return true;
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

// 1/7 floor guards against deleting source when output is tiny/corrupted.
fn maybe_delete_source(queue_entry: &ConversionQueueEntry, produced_outputs: &[PathBuf]) {
    let input_size = std::fs::metadata(&queue_entry.input).map_or(0, |meta| meta.len());
    let output_size = produced_outputs
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok().map(|meta| meta.len()))
        .sum::<u64>();
    if input_size > 0 && output_size > input_size / 7 {
        match std::fs::remove_file(&queue_entry.input) {
            Ok(()) => info!("Deleted source: {}", queue_entry.input.display()),
            Err(e) => warn!("Delete source failed {}: {e}", queue_entry.input.display()),
        }
    } else {
        warn!("Skipped delete: output {output_size}B too small vs input {input_size}B");
    }
}

fn resolve_segment_output(
    queue_entry: &ConversionQueueEntry,
    segment_idx: usize,
    total_segments: usize,
    needs_concat: bool,
    split_mode: bool,
    temp_files: &mut Vec<PathBuf>,
) -> PathBuf {
    if needs_concat {
        let stem = queue_entry
            .output
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy();
        let ext = queue_entry
            .output
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let dir = queue_entry.output.parent().unwrap_or(Path::new("."));
        let temp_file = dir.join(format!(".{stem}_seg{segment_idx}.{ext}"));
        temp_files.push(temp_file.clone());
        temp_file
    } else if total_segments > 1 && split_mode {
        segment_output_path(
            &queue_entry.output,
            segment_idx,
            total_segments,
            &queue_entry.input,
        )
    } else {
        queue_entry.output.clone()
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

fn skip_completed_resume_outputs(
    queue_entry: &ConversionQueueEntry,
    final_outputs: &[PathBuf],
    sender: &async_channel::Sender<QueueMessage>,
) -> bool {
    if final_outputs.is_empty()
        || !final_outputs
            .iter()
            .all(|path| is_resume_output_complete(path))
    {
        return false;
    }
    info!(
        "Skipping completed conversion output for {}",
        queue_entry.input.display()
    );
    let progress = progress::FfmpegProgressSnapshot {
        percent: 100.0,
        done: true,
        ..Default::default()
    };
    let _ = sender.send_blocking(QueueMessage::Progress {
        entry_id: queue_entry.id,
        progress_snapshot: progress,
    });
    true
}

fn output_path_for_resume(
    final_output: &Path,
    resume_context: Option<&ConversionResumeContext>,
    entry_id: u64,
    segment_idx: usize,
) -> PathBuf {
    resume_context.map_or_else(
        || final_output.to_path_buf(),
        |context| resume_partial_output_path(final_output, &context.job_key, entry_id, segment_idx),
    )
}

fn promote_output_for_resume(
    partial_output: &Path,
    final_output: &Path,
    resume_context: Option<&ConversionResumeContext>,
) -> Result<()> {
    if resume_context.is_some() {
        promote_resume_output(partial_output, final_output).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!(
                    "promote {} to {}: {error}",
                    partial_output.display(),
                    final_output.display()
                ),
            ))
        })?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_stages(
    queue_entry: &ConversionQueueEntry,
    segment: &TrimRange,
    seg_output: &Path,
    stages: &[(GpuStage, GpuDevice)],
    profile: &Profile,
    subtitle_streams: &[subtitle::SubtitleStream],
    audio_only_mode: bool,
    cancel: &Arc<AtomicBool>,
    sender: &async_channel::Sender<QueueMessage>,
    resume_context: Option<&ConversionResumeContext>,
    segment_idx: usize,
) -> bool {
    let ffmpeg_output =
        output_path_for_resume(seg_output, resume_context, queue_entry.id, segment_idx);
    for (stage_idx, (stage, stage_gpu)) in stages.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        if stage_idx > 0 {
            let stage_name = match stage {
                GpuStage::PartialGpu => "partial-gpu",
                GpuStage::Software => "software",
                GpuStage::FullGpu => "full-gpu",
            };
            let _ = sender.send_blocking(QueueMessage::GpuFallback {
                entry_id: queue_entry.id,
                stage: stage_name,
            });
        }
        let job = FfmpegJob {
            input: queue_entry.input.clone(),
            output: ffmpeg_output.clone(),
            profile: profile.clone(),
            gpu: stage_gpu.clone(),
            stage: *stage,
            trim: segment.clone(),
            subtitle: queue_entry.subtitle.clone(),
            audio_track: queue_entry.audio_track,
            video_track: queue_entry.video_track,
            crop: queue_entry.crop.clone(),
            video_overrides: queue_entry.video_overrides.clone(),
            input_audio_channels: queue_entry.input_audio_channels,
            subtitle_streams: subtitle_streams.to_vec(),
            skip_video_filters: queue_entry.skip_video_filters,
            audio_only: audio_only_mode,
        };
        match run_job(&job, queue_entry, cancel, sender) {
            Ok(()) => match promote_output_for_resume(&ffmpeg_output, seg_output, resume_context) {
                Ok(()) => return true,
                Err(error) => {
                    warn!(
                        "Promote output failed for {}: {error}",
                        seg_output.display()
                    );
                    let _ = std::fs::remove_file(&ffmpeg_output);
                }
            },
            Err(error) => {
                warn!(
                    "Stage {stage:?} failed for {}: {error}",
                    queue_entry.input.display()
                );
                let _ = std::fs::remove_file(&ffmpeg_output);
            }
        }
    }
    false
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
    for queue_entry in &mut items {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // Close the probe-vs-start race: if the background probe worker has not
        // answered for this queue_entry yet, fill its metadata inline here (worker
        // thread, never the main loop) so the ffmpeg command is built from real
        // duration / audio-channel / HEVC-skip values, not add_file defaults.
        ensure_entry_probed(queue_entry, &profile.video.codec);
        info!(
            "Converting: {} → {}",
            queue_entry.input.display(),
            queue_entry.output.display()
        );
        if !has_enough_disk_space(queue_entry, sender)
            || !is_mp4_copy_compatible(queue_entry, profile, sender)
        {
            continue;
        }
        if let Some(parent) = queue_entry.output.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            let _ = sender.send_blocking(QueueMessage::Failed {
                entry_id: queue_entry.id,
                error: format!("{}: {error}", i18n("Create output dir")),
            });
            continue;
        }

        let subtitle_streams = subtitle::probe_subtitles(&queue_entry.input);
        let stages = build_stage_list(gpu);
        let effective_segments = if queue_entry.segments.is_empty() {
            vec![TrimRange::default()]
        } else {
            queue_entry.segments.clone()
        };
        let total_segments = effective_segments.len();
        let split_mode = queue_entry.segment_mode == SegmentOutputMode::Split;
        let needs_concat = total_segments > 1 && !split_mode;
        let final_outputs =
            resolve_final_outputs(queue_entry, total_segments, needs_concat, split_mode);
        if resume_context.is_some()
            && skip_completed_resume_outputs(queue_entry, &final_outputs, sender)
        {
            if delete_source {
                maybe_delete_source(queue_entry, &final_outputs);
            }
            let _ = sender.send_blocking(QueueMessage::Done {
                entry_id: queue_entry.id,
            });
            continue;
        }

        let mut temp_files: Vec<PathBuf> = Vec::new();
        let mut produced_outputs: Vec<PathBuf> = Vec::new();
        let mut succeeded = false;

        for (seg_idx, segment) in effective_segments.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let seg_output = resolve_segment_output(
                queue_entry,
                seg_idx,
                total_segments,
                needs_concat,
                split_mode,
                &mut temp_files,
            );
            if resume_context.is_some() && is_resume_output_complete(&seg_output) {
                info!("Skipping completed segment {}", seg_output.display());
                produced_outputs.push(seg_output);
                succeeded = true;
                continue;
            }
            let seg_ok = run_stages(
                queue_entry,
                segment,
                &seg_output,
                &stages,
                profile,
                &subtitle_streams,
                audio_only_mode,
                cancel,
                sender,
                resume_context.as_ref(),
                seg_idx,
            );
            if !seg_ok {
                for temp_file in &temp_files {
                    let _ = std::fs::remove_file(temp_file);
                }
                succeeded = false;
                break;
            }
            produced_outputs.push(seg_output);
            succeeded = true;
        }

        if succeeded && needs_concat && !cancel.load(Ordering::Relaxed) {
            let subtitle_temp_dir = std::env::temp_dir().join("big-video-converter");
            let _ = std::fs::create_dir_all(&subtitle_temp_dir);
            let merged_subs = crate::converter::subtitle::build_joined_subtitle_files(
                &queue_entry.input,
                &queue_entry.segments,
                &subtitle_temp_dir,
            );
            let concat_output = output_path_for_resume(
                &queue_entry.output,
                resume_context.as_ref(),
                queue_entry.id,
                0,
            );
            match concat_segments(&temp_files, &concat_output, &merged_subs) {
                Err(error) => {
                    warn!(
                        "Concat failed for {}: {error}",
                        queue_entry.output.display()
                    );
                    let _ = std::fs::remove_file(&concat_output);
                    succeeded = false;
                }
                _ => {
                    if let Err(error) = promote_output_for_resume(
                        &concat_output,
                        &queue_entry.output,
                        resume_context.as_ref(),
                    ) {
                        warn!(
                            "Promote concat output failed for {}: {error}",
                            queue_entry.output.display()
                        );
                        let _ = std::fs::remove_file(&concat_output);
                        succeeded = false;
                    }
                }
            }
            for temp_file in &temp_files {
                let _ = std::fs::remove_file(temp_file);
            }
            if succeeded {
                produced_outputs = vec![queue_entry.output.clone()];
            }
        }

        if cancel.load(Ordering::Relaxed) {
            let _ = sender.send_blocking(QueueMessage::Cancelled {
                entry_id: queue_entry.id,
            });
            break;
        }
        if succeeded {
            if delete_source {
                maybe_delete_source(queue_entry, &produced_outputs);
            }
            let _ = sender.send_blocking(QueueMessage::Done {
                entry_id: queue_entry.id,
            });
        } else {
            let _ = sender.send_blocking(QueueMessage::Failed {
                entry_id: queue_entry.id,
                error: i18n("All GPU stages failed"),
            });
        }
    }

    // Leftover items remain Pending; the coordinator transitions them to Cancelled.
}

fn concat_segments(
    segments: &[PathBuf],
    output: &Path,
    merged_subs: &[(PathBuf, String)],
) -> Result<()> {
    use std::io::Write;

    let dir = output.parent().unwrap_or(Path::new("."));
    let concat_list = dir.join(".concat_list.txt");

    {
        let mut file = std::fs::File::create(&concat_list)?;
        for segment in segments {
            writeln!(file, "file '{}'", segment.display())?;
        }
    }

    let mut command = BigSubprocessSpec::builder()
        .program("ffmpeg")
        .args(["-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&concat_list);

    for (sub_path, _) in merged_subs {
        command = command.args(["-i"]).arg(sub_path);
    }

    command = command.args(["-c", "copy"]);

    if !merged_subs.is_empty() {
        command = command.args(["-map", "0"]);
        for (index, (_, lang)) in merged_subs.iter().enumerate() {
            command = command.args(["-map", &format!("{}:0", index + 1)]);
            command = command.args([
                &format!("-metadata:s:s:{index}"),
                &format!("language={lang}"),
            ]);
        }
        command = command.args(["-c:s", "srt"]);
    }

    let out = command
        .arg("--")
        .arg(output)
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_mins(1))
        .build()
        .run()
        .map_err(|e| Error::ffmpeg("ffmpeg", None, e.to_string()))?;

    let _ = std::fs::remove_file(&concat_list);
    for (sub_path, _) in merged_subs {
        let _ = std::fs::remove_file(sub_path);
    }

    if out.status.success() {
        Ok(())
    } else {
        Err(Error::ffmpeg(
            "ffmpeg",
            out.status.code(),
            "concat segments failed",
        ))
    }
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
    fn resolve_segment_output_picks_temp_concat_split_or_passthrough() {
        let it = queue_entry("/tmp/bbt_resolve_dst/movie.mp4");

        // needs_concat → hidden temp file in the output dir, recorded for cleanup.
        let mut temp = Vec::new();
        let concat = resolve_segment_output(&it, 0, 2, true, false, &mut temp);
        assert_eq!(
            concat,
            PathBuf::from("/tmp/bbt_resolve_dst/.movie_seg0.mp4")
        );
        assert_eq!(temp, vec![concat.clone()]);

        // Single segment, no concat → the real output path, nothing recorded.
        let mut temp2 = Vec::new();
        let single = resolve_segment_output(&it, 0, 1, false, false, &mut temp2);
        assert_eq!(single, it.output);
        assert!(temp2.is_empty());

        // Multi-segment split → per-part path (zero-padded part index).
        let mut temp3 = Vec::new();
        let part = resolve_segment_output(&it, 0, 2, false, true, &mut temp3);
        assert_eq!(part, PathBuf::from("/tmp/bbt_resolve_dst/movie_part01.mp4"));
        assert!(temp3.is_empty());
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
