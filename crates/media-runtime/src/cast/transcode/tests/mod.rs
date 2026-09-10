#![allow(clippy::field_reassign_with_default)]

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::cast::gpu::GpuType;

use super::{TranscodeFilters, TranscodeSession, TranscodeStage};

mod args;
mod audio;
mod filters;
mod video;

fn make_session(filters: TranscodeFilters) -> TranscodeSession {
    TranscodeSession {
        input_path: "/tmp/test.mkv".to_string(),
        gpu_type: GpuType::Cpu,
        device_path: String::new(),
        filters,
        child: None,
        _stop: Arc::new(AtomicBool::new(false)),
        extracted_sub_path: None,
        extracted_sub_track: None,
        video_width: 1920,
        video_height: 1080,
        temp_dir: None,
    }
}

#[test]
fn scratch_path_is_inside_owned_temporary_dir_and_outside_world_writable_temp() {
    // Stage 5 H-2 invariant: cast scratch files live inside an owned 0700
    // directory, never directly under `/tmp/cast-*` where another local
    // user could pre-create a symlink.
    let mut session = make_session(TranscodeFilters::default());
    let path = session
        .scratch_path("invariant.log")
        .expect("scratch dir creation must succeed in a normal test env");
    let parent = path.parent().expect("scratch path always has parent");
    assert!(
        parent != std::path::Path::new("/tmp"),
        "scratch file `{path:?}` must not sit directly in /tmp"
    );

    let meta = std::fs::metadata(parent).expect("scratch dir metadata");
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o700, "scratch dir mode must be 0700 (was {mode:#o})");
}
