// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared media-track label formatting.
//!
//! Both `mpv_track_label` and `ffprobe_stream_label` are pure: they do
//! no I/O and do no translation. Callers pass already-translated label
//! text through `TrackLabelText`.

/// Translated literals used by [`mpv_track_label`].
///
/// Holding them in one struct keeps the function signature short and
/// makes it impossible to forget the `external` variant in a translation
/// catalog.
#[derive(Debug, Clone, Copy)]
pub struct TrackLabelText<'a> {
    /// Generic word used in fallback labels, typically `"Track"`.
    pub track: &'a str,
    /// Word that marks an external (sidecar) subtitle or audio source.
    pub external: &'a str,
}

/// Format an mpv-style track label, e.g. `"1: Director's commentary [eng] (aac) (external)"`.
///
/// All four metadata fields are optional. When every metadata field is
/// empty, the label degrades to `"<track> <id>"` using
/// [`TrackLabelText::track`].
#[must_use]
pub fn mpv_track_label(
    id: i64,
    title: &str,
    language: &str,
    codec: &str,
    external: bool,
    text: TrackLabelText<'_>,
) -> String {
    let mut parts = Vec::new();

    if !title.is_empty() {
        parts.push(title.to_string());
    }
    if !language.is_empty() {
        parts.push(format!("[{language}]"));
    }
    if !codec.is_empty() {
        parts.push(format!("({codec})"));
    }
    if external {
        parts.push(format!("({})", text.external));
    }

    if parts.is_empty() {
        format!("{} {id}", text.track)
    } else {
        format!("{id}: {}", parts.join(" "))
    }
}

/// Format an ffprobe-style stream label, e.g. `"3: English (aac)"`.
///
/// `sequential` is the visible row number, `track_label` is the
/// translated `"Track"` fallback used when the stream has neither a
/// title nor a language tag.
#[must_use]
pub fn ffprobe_stream_label(
    sequential: usize,
    title: &str,
    language: &str,
    codec: &str,
    track_label: &str,
) -> String {
    let name = if !title.is_empty() {
        title.to_string()
    } else if !language.is_empty() {
        language.to_string()
    } else {
        format!("{track_label} {sequential}")
    };
    format!("{sequential}: {name} ({codec})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> TrackLabelText<'static> {
        TrackLabelText {
            track: "Track",
            external: "external",
        }
    }

    #[test]
    fn mpv_track_label_formats_full_track_metadata() {
        assert_eq!(
            mpv_track_label(1, "Director Commentary", "eng", "aac", false, labels()),
            "1: Director Commentary [eng] (aac)"
        );
    }

    #[test]
    fn mpv_track_label_marks_external() {
        assert_eq!(
            mpv_track_label(2, "Subs", "por", "srt", true, labels()),
            "2: Subs [por] (srt) (external)"
        );
    }

    #[test]
    fn mpv_label_falls_back_to_id() {
        assert_eq!(mpv_track_label(5, "", "", "", false, labels()), "Track 5");
    }

    #[test]
    fn ffprobe_label_prefers_title_then_language() {
        assert_eq!(
            ffprobe_stream_label(1, "English", "eng", "aac", "Track"),
            "1: English (aac)"
        );
        assert_eq!(
            ffprobe_stream_label(2, "", "por", "ac3", "Track"),
            "2: por (ac3)"
        );
        assert_eq!(
            ffprobe_stream_label(3, "", "", "opus", "Track"),
            "3: Track 3 (opus)"
        );
    }
}
