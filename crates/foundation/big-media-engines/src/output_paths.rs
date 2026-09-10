//! Output filename helpers for media conversion queues.
//!
//! When the user chooses an output path that already exists or matches the
//! input, the converter inserts a `_converted` suffix (and a numeric tail after
//! the first collision) to avoid clobbering data. These helpers are pure path
//! manipulation: they probe path existence, but never write or remove files.

use std::path::{Path, PathBuf};

/// Return `output` unchanged when it is safe to use; otherwise append
/// `_converted` (and a numeric tail) until a free filename is found.
///
/// A path is considered safe when it differs from `input` and does not exist on
/// disk. When it is not, the function probes `<stem>_converted.<ext>` first,
/// then `<stem>_converted_2.<ext>` through `<stem>_converted_999.<ext>`.
/// Falls back to the bare `_converted` name if the numeric range is exhausted.
#[must_use]
pub fn dedup_output_path(output: &Path, input: &Path) -> PathBuf {
    if output != input && !output.exists() {
        return output.to_path_buf();
    }
    let stem = output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    let ext = output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    let dir = output.parent().unwrap_or(Path::new("."));
    let candidate = dir.join(format!("{stem}_converted.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    for index in 2..=999 {
        let path = dir.join(format!("{stem}_converted_{index}.{ext}"));
        if !path.exists() {
            return path;
        }
    }
    candidate
}

/// Build a `<stem>_partNN.<ext>` filename for one segment of a multi-output run.
///
/// `segment_index` is zero-based; the produced filename uses one-based
/// numbering zero-padded to at least the width of `total_segments` (minimum 2
/// digits). The result is then passed through [`dedup_output_path`] against
/// `input` to avoid clobbering.
#[must_use]
pub fn segment_output_path(
    base_output: &Path,
    segment_index: usize,
    total_segments: usize,
    input: &Path,
) -> PathBuf {
    let stem = base_output
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("output");
    let ext = base_output
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mp4");
    let dir = base_output.parent().unwrap_or(Path::new("."));
    let width = total_segments.to_string().len().max(2);
    let candidate = dir.join(format!(
        "{stem}_part{:0width$}.{ext}",
        segment_index + 1,
        width = width
    ));
    dedup_output_path(&candidate, input)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn dedup_output_path_keeps_non_existing_distinct_path() {
        let dir = temp_dir("big-media-output-distinct");
        let output = dir.join("out.mp4");
        let input = dir.join("input.mp4");

        assert_eq!(dedup_output_path(&output, &input), output);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dedup_output_path_adds_converted_suffix_for_same_or_existing_path() {
        let dir = temp_dir("big-media-output-dedup");
        let output = dir.join("out.mp4");
        std::fs::write(&output, b"exists").expect("write output");

        assert_eq!(
            dedup_output_path(&output, &output),
            dir.join("out_converted.mp4")
        );
        std::fs::write(dir.join("out_converted.mp4"), b"exists").expect("write candidate");
        assert_eq!(
            dedup_output_path(&output, &output),
            dir.join("out_converted_2.mp4")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn dedup_output_path_avoids_clobbering_existing_distinct_file() {
        let dir = temp_dir("big-media-output-existing-distinct");
        let output = dir.join("out.mp4");
        let input = dir.join("input.mp4");
        std::fs::write(&output, b"exists").expect("write output");
        assert_eq!(
            dedup_output_path(&output, &input),
            dir.join("out_converted.mp4")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn segment_output_path_formats_with_padding() {
        let path = segment_output_path(
            Path::new("/tmp/video.mp4"),
            1,
            12,
            Path::new("/tmp/input.mp4"),
        );
        assert_eq!(path, PathBuf::from("/tmp/video_part02.mp4"));
    }
}
