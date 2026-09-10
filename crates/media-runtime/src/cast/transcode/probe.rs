use std::path::Path;

use big_app_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

/// Probe video dimensions from a media file using ffprobe (sandboxed).
/// Returns (width, height), defaulting to (1920, 1080) on failure or when the
/// bubblewrap sandbox is unavailable.
pub(super) fn probe_video_dimensions(file_path: &str) -> (u32, u32) {
    let args = ffprobe_dimensions_argv(file_path);
    let Some(sandboxed) =
        crate::cast::sandbox::wrap_args("ffprobe", &args, &[Path::new(file_path)], &[])
    else {
        return (1920, 1080);
    };
    let output = BigSubprocessSpec::builder()
        .program("bwrap")
        .args(sandboxed)
        .stderr(BigSubprocessOutputMode::Null)
        .build()
        .run();

    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = s.trim().split(',').collect();
            if parts.len() >= 2 {
                let w = parts[0].parse().unwrap_or(1920);
                let h = parts[1].parse().unwrap_or(1080);
                (w, h)
            } else {
                (1920, 1080)
            }
        }
        _ => (1920, 1080),
    }
}

/// ffprobe argv for the video stream's width/height. `--` terminates flag
/// parsing so a hostile path starting with `-` is read as a positional file;
/// kept separate from the sandbox wrapping so it stays unit-testable.
fn ffprobe_dimensions_argv(file_path: &str) -> Vec<&str> {
    vec![
        "-v",
        "quiet",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height",
        "-of",
        "csv=p=0",
        "--",
        file_path,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ffprobe_dimensions_argv_separates_hostile_path() {
        let hostile = "-f lavfi -i color=red";
        let argv = ffprobe_dimensions_argv(hostile);
        let idx = argv.iter().position(|a| *a == hostile).expect("path argv");
        assert!(idx > 0);
        assert_eq!(argv[idx - 1], "--", "argv={argv:?}");
    }
}
