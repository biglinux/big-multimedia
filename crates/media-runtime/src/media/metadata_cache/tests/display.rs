use crate::media::metadata_cache::{meta_summary, resolution_label};
use crate::media::playlist::VideoMeta;

#[test]
fn meta_summary_full() {
    let meta = VideoMeta {
        duration_secs: Some(3661.0),
        resolution: Some("1920×1080".into()),
        codec: Some("h264".into()),
        audio_tracks: 2,
        subtitle_tracks: 1,
    };
    let s = meta_summary(&meta);
    assert!(s.contains("1:01:01"));
    assert!(s.contains("1920×1080"));
    assert!(s.contains("h264"));
    assert!(s.contains("2 audio tracks"));
    assert!(s.contains("1 subtitle"));
}

#[test]
fn meta_summary_empty() {
    let meta = VideoMeta::default();
    assert!(meta_summary(&meta).is_empty());
}

#[test]
fn meta_summary_single_audio_singular() {
    let meta = VideoMeta {
        audio_tracks: 1,
        ..Default::default()
    };
    let s = meta_summary(&meta);
    assert!(s.contains("1 audio track"));
    assert!(!s.contains("audio tracks"));
}

#[test]
fn meta_summary_zero_duration_hidden() {
    let meta = VideoMeta {
        duration_secs: Some(0.0),
        ..Default::default()
    };
    assert!(meta_summary(&meta).is_empty());
}

#[test]
fn meta_summary_multi_subs() {
    let meta = VideoMeta {
        subtitle_tracks: 3,
        ..Default::default()
    };
    assert!(meta_summary(&meta).contains("3 subtitles"));
}

#[test]
fn resolution_label_4k() {
    let meta = VideoMeta {
        resolution: Some("3840×2160".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "4K");
}

#[test]
fn resolution_label_1080p() {
    let meta = VideoMeta {
        resolution: Some("1920x1080".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "1080p");
}

#[test]
fn resolution_label_720p() {
    let meta = VideoMeta {
        resolution: Some("1280×720".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "720p");
}

#[test]
fn resolution_label_480p() {
    let meta = VideoMeta {
        resolution: Some("854x480".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "480p");
}

#[test]
fn resolution_label_small() {
    let meta = VideoMeta {
        resolution: Some("320x240".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "240p");
}

#[test]
fn resolution_label_none_when_missing() {
    assert!(resolution_label(&VideoMeta::default()).is_none());
}

#[test]
fn resolution_label_none_for_malformed() {
    let meta = VideoMeta {
        resolution: Some("bad".into()),
        ..Default::default()
    };
    assert!(resolution_label(&meta).is_none());
}

#[test]
fn resolution_label_1440p() {
    let meta = VideoMeta {
        resolution: Some("2560x1440".into()),
        ..Default::default()
    };
    assert_eq!(resolution_label(&meta).unwrap(), "1440p");
}
