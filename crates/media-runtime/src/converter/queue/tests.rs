//! Queue unit tests: friendly ffmpeg-error mapping, ConversionQueue state
//! transitions, segment ordering/clearing, QueueMessage handling.

use super::job::friendly_ffmpeg_error;
use super::support::{
    ensure_entry_probed, is_resume_output_complete, promote_resume_output,
    resume_partial_output_path, segment_output_path,
};
use super::*;
use crate::converter::ffmpeg::TrimRange;
use crate::converter::file_validator::format_size;
use std::path::{Path, PathBuf};

#[test]
fn friendly_error_no_such_file() {
    let lines = vec!["No such file or directory".into()];
    assert_eq!(friendly_ffmpeg_error(&lines, 1), "Input file not found");
}

#[test]
fn friendly_error_permission_denied() {
    let lines = vec!["Permission denied".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Permission denied — check file permissions"
    );
}

#[test]
fn friendly_error_disk_full() {
    let lines = vec!["write error: No space left on device".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Disk full — free up space and try again"
    );
}

#[test]
fn friendly_error_corrupted_input() {
    let lines = vec!["Invalid data found when processing input".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "File is corrupted or has an unsupported format"
    );
}

#[test]
fn friendly_error_codec_not_supported() {
    let lines = vec!["Codec not currently supported in container".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Unsupported codec — try a different format"
    );
}

#[test]
fn friendly_error_encoder_not_found() {
    let lines = vec!["Encoder hevc_nvenc not found".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Required encoder not available — check FFmpeg installation"
    );
}

#[test]
fn friendly_error_unknown_encoder() {
    let lines = vec!["Unknown encoder 'libfoo'".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Unknown codec — install required codec support"
    );
}

#[test]
fn friendly_error_unknown_decoder() {
    let lines = vec!["Unknown decoder 'libbar'".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Unknown codec — install required codec support"
    );
}

#[test]
fn friendly_error_already_exists() {
    let lines = vec!["File '/tmp/out.mp4' already exists. Overwrite?".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Output file already exists"
    );
}

#[test]
fn friendly_error_could_not_open() {
    let lines = vec!["Could not open input file".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Could not open file — check path and permissions"
    );
}

#[test]
fn friendly_error_failed_to_open() {
    let lines = vec!["Failed to open file '/dev/null'".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Could not open file — check path and permissions"
    );
}

#[test]
fn friendly_error_fallback_last_error_line() {
    let lines = vec![
        "some info output".into(),
        "Conversion failed with error xyz".into(),
    ];
    let worker_message = friendly_ffmpeg_error(&lines, 2);
    assert_eq!(worker_message, "Conversion failed with error xyz");
}

#[test]
fn friendly_error_fallback_exit_code() {
    let lines = vec!["some harmless output".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 137),
        "FFmpeg exited with code 137"
    );
}

#[test]
fn friendly_error_empty_lines() {
    assert_eq!(friendly_ffmpeg_error(&[], 1), "FFmpeg exited with code 1");
}

#[test]
fn friendly_error_case_insensitive() {
    let lines = vec!["PERMISSION DENIED accessing /root/file.mp4".into()];
    assert_eq!(
        friendly_ffmpeg_error(&lines, 1),
        "Permission denied — check file permissions"
    );
}

#[test]
fn queue_new_starts_empty() {
    let q = ConversionQueue::new();
    assert_eq!(q.total(), 0);
    assert!(q.items().is_empty());
}

#[test]
fn queue_add_file_increments_id() {
    let mut q = ConversionQueue::new();
    let id1 = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let id2 = q.add_file(PathBuf::from("/tmp/b.mp4"));
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
    assert_eq!(q.total(), 2);
}

#[test]
fn queue_add_file_sets_pending() {
    let mut q = ConversionQueue::new();
    let _ = q.add_file(PathBuf::from("/tmp/a.mp4"));
    assert_eq!(q.items()[0].status, ConversionStatus::Pending);
    assert_eq!(q.items()[0].segment_mode, SegmentOutputMode::Join);
}

#[test]
fn add_file_entry_starts_unprobed_with_defaults() {
    // The probe-vs-start guard relies on a freshly added queue_entry being `!probed`
    // with default metadata, so the converter worker re-probes it inline.
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let queue_entry = q
        .items()
        .iter()
        .find(|i| i.id == id)
        .expect("queue_entry present");
    assert!(!queue_entry.probed, "new queue_entry must start unprobed");
    assert_eq!(queue_entry.duration_secs, 0.0);
    assert_eq!(queue_entry.input_audio_channels, 0);
    assert!(!queue_entry.skip_video_filters);
}

#[test]
fn ensure_entry_probed_skips_already_probed_entry() {
    // Already-probed items must NOT be re-probed: the path does not exist, so a
    // stray ffprobe would zero duration_secs and clear the fields. The early
    // `if queue_entry.probed { return }` guard keeps the real values intact.
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/does-not-exist-probe-guard.mp4"));
    let queue_entry = q.entry_mut(id).expect("queue_entry present");
    queue_entry.probed = true;
    queue_entry.duration_secs = 42.0;
    queue_entry.input_audio_channels = 6;
    queue_entry.skip_video_filters = true;

    ensure_entry_probed(queue_entry, "h264");

    assert_eq!(
        queue_entry.duration_secs, 42.0,
        "duration must survive (no re-probe)"
    );
    assert_eq!(queue_entry.input_audio_channels, 6);
    assert!(queue_entry.skip_video_filters);
    assert!(queue_entry.probed);
}

#[test]
fn segment_output_path_formats_with_padding() {
    let out = segment_output_path(
        Path::new("/tmp/video.mp4"),
        1,
        12,
        Path::new("/tmp/input.mp4"),
    );
    assert_eq!(out, PathBuf::from("/tmp/video_part02.mp4"));
}

#[test]
fn resume_partial_output_path_keeps_muxer_extension_and_sanitizes_job_key() {
    let out =
        resume_partial_output_path(Path::new("/tmp/video final.mp4"), "job/with spaces", 7, 2);
    assert_eq!(
        out,
        PathBuf::from("/tmp/.big-convertd-job_with_spaces-i7-s2.part.mp4")
    );
}

#[test]
fn promote_resume_output_renames_partial_and_keeps_existing_final() {
    let dir =
        std::env::temp_dir().join(format!("big-converter-resume-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create test dir");
    let partial = dir.join(".part.mp4");
    let final_output = dir.join("final.mp4");

    std::fs::write(&partial, b"video").expect("write partial");
    promote_resume_output(&partial, &final_output).expect("promote partial");
    assert!(!partial.exists());
    assert!(is_resume_output_complete(&final_output));
    assert_eq!(std::fs::read(&final_output).expect("read final"), b"video");

    std::fs::write(&partial, b"new").expect("write second partial");
    promote_resume_output(&partial, &final_output).expect("existing final wins");
    assert_eq!(std::fs::read(&final_output).expect("read final"), b"video");
    assert!(!partial.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn queue_format_bytes_uses_shared_decimal_contract() {
    assert_eq!(format_size(500), "500 B");
    assert_eq!(format_size(2048), "2 KB");
    assert_eq!(format_size(5_242_880), "5.0 MB");
}

#[test]
fn queue_sort_pending_segments_chronological_orders_by_start() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let queue_entry = q.entry_mut(id).expect("queue queue_entry must exist");
    queue_entry.segments = vec![
        TrimRange {
            start: Some(40.0),
            end: Some(50.0),
        },
        TrimRange {
            start: Some(10.0),
            end: Some(20.0),
        },
        TrimRange {
            start: Some(30.0),
            end: Some(35.0),
        },
    ];

    q.sort_pending_segments_chronological();
    let starts: Vec<f64> = q.items()[0]
        .segments
        .iter()
        .map(|segment| segment.start.unwrap_or(-1.0))
        .collect();
    assert_eq!(starts, vec![10.0, 30.0, 40.0]);
}

#[test]
fn queue_clear_pending_segments_only_affects_pending_items() {
    let mut q = ConversionQueue::new();
    let id_pending = q.add_file(PathBuf::from("/tmp/pending.mp4"));
    let id_done = q.add_file(PathBuf::from("/tmp/done.mp4"));

    q.entry_mut(id_pending)
        .expect("pending queue_entry")
        .segments = vec![TrimRange {
        start: Some(1.0),
        end: Some(2.0),
    }];
    q.entry_mut(id_done).expect("done queue_entry").segments = vec![TrimRange {
        start: Some(3.0),
        end: Some(4.0),
    }];
    q.entry_mut(id_done).expect("done queue_entry").status = ConversionStatus::Done;

    q.clear_pending_segments();

    assert!(
        q.items()
            .iter()
            .find(|queue_entry| queue_entry.id == id_pending)
            .expect("pending exists")
            .segments
            .is_empty()
    );
    assert_eq!(
        q.items()
            .iter()
            .find(|queue_entry| queue_entry.id == id_done)
            .expect("done exists")
            .segments
            .len(),
        1
    );
}

#[test]
fn queue_remove_entry_existing() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    assert!(q.remove_entry(id));
    assert_eq!(q.total(), 0);
}

#[test]
fn queue_remove_entry_nonexistent() {
    let mut q = ConversionQueue::new();
    assert!(!q.remove_entry(999));
}

#[test]
fn queue_clear_pending_keeps_done() {
    let mut q = ConversionQueue::new();
    let _ = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let id2 = q.add_file(PathBuf::from("/tmp/b.mp4"));
    q.entry_mut(id2).expect("done queue_entry").status = ConversionStatus::Done;
    q.clear_pending();
    assert_eq!(q.total(), 1);
    assert_eq!(q.items()[0].id, id2);
}

#[test]
fn queue_clear_all() {
    let mut q = ConversionQueue::new();
    let _ = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let _ = q.add_file(PathBuf::from("/tmp/b.mp4"));
    q.clear_all();
    assert_eq!(q.total(), 0);
}

#[test]
fn queue_entry_mut_returns_correct_entry() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/test.mp4"));
    let queue_entry = q.entry_mut(id).expect("queue queue_entry should exist");
    assert_eq!(queue_entry.input, PathBuf::from("/tmp/test.mp4"));
}

#[test]
fn queue_cancel_flag() {
    let q = ConversionQueue::new();
    assert!(!q.is_cancelled());
    q.cancel();
    assert!(q.is_cancelled());
}

#[test]
fn queue_count_by_status() {
    let mut q = ConversionQueue::new();
    let id1 = q.add_file(PathBuf::from("/tmp/a.mp4"));
    let _ = q.add_file(PathBuf::from("/tmp/b.mp4"));
    let _ = q.add_file(PathBuf::from("/tmp/c.mp4"));
    q.entry_mut(id1).expect("first queue_entry").status = ConversionStatus::Done;
    assert_eq!(q.count(&ConversionStatus::Pending), 2);
    assert_eq!(q.count(&ConversionStatus::Done), 1);
    assert_eq!(q.count(&ConversionStatus::Failed("err".into())), 0);
}

#[test]
fn queue_set_profile_and_output_dir() {
    let mut q = ConversionQueue::new();
    let mut profile = q.profile().clone();
    profile.video.codec = "h265".into();
    q.set_profile(profile);
    assert_eq!(q.profile().video.codec, "h265");

    q.set_output_dir(PathBuf::from("/tmp/output"));
    assert_eq!(q.output_dir(), Path::new("/tmp/output"));
}

#[test]
fn queue_output_path_uses_configured_output_dir() {
    let mut q = ConversionQueue::new();
    q.set_output_dir(PathBuf::from("/tmp/big-output"));
    let id = q.add_file(PathBuf::from("/home/user/video.mp4"));
    let queue_entry = q
        .items()
        .iter()
        .find(|queue_entry| queue_entry.id == id)
        .expect("queue queue_entry");
    assert!(queue_entry.output.starts_with("/tmp/big-output"));
}

#[test]
fn queue_drain_worker_messages_done() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    q.entry_mut(id).expect("queue queue_entry").status = ConversionStatus::Converting;
    let _ = q.sender.send_blocking(QueueMessage::Done { entry_id: id });
    q.drain_worker_messages();
    assert_eq!(q.items()[0].status, ConversionStatus::Done);
}

#[test]
fn queue_drain_worker_messages_failed() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    q.entry_mut(id).expect("queue queue_entry").status = ConversionStatus::Converting;
    let _ = q.sender.send_blocking(QueueMessage::Failed {
        entry_id: id,
        error: "test error".into(),
    });
    q.drain_worker_messages();
    assert_eq!(
        q.items()[0].status,
        ConversionStatus::Failed("test error".into())
    );
}

#[test]
fn queue_drain_worker_messages_cancelled() {
    let mut q = ConversionQueue::new();
    let id = q.add_file(PathBuf::from("/tmp/a.mp4"));
    q.entry_mut(id).expect("queue queue_entry").status = ConversionStatus::Converting;
    let _ = q
        .sender
        .send_blocking(QueueMessage::Cancelled { entry_id: id });
    q.drain_worker_messages();
    assert_eq!(q.items()[0].status, ConversionStatus::Cancelled);
}

#[test]
fn item_status_eq_failed_different_messages() {
    let s1 = ConversionStatus::Failed("err1".into());
    let s2 = ConversionStatus::Failed("err2".into());
    assert_ne!(s1, s2);
}

#[test]
fn item_status_eq_pending() {
    assert_eq!(ConversionStatus::Pending, ConversionStatus::Pending);
    assert_ne!(ConversionStatus::Pending, ConversionStatus::Done);
}
