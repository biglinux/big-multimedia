//! Pure (GTK-free) lyrics fetch for the Relm4 `AudioShell`.
//!
//! Ported out of the god-object `lyrics_panel` (an `impl Window` method) so the
//! shell can run it off-thread without a GTK widget. Embedded tag lyrics first
//! (no network); LRCLIB + lyrics.ovh as a fallback. The shell wraps it in
//! `gio::spawn_blocking` and pushes the plain-text result into the panel.

use std::sync::PoisonError;

use crate::db::queries;
use crate::db::DbHandle;
use crate::library::{lrclib, metadata};

/// Resolve plain-text lyrics for `file_path`: embedded tag lyrics first, then a
/// network lookup keyed on the library track's artist/title/album. LRC
/// `[mm:ss.xx]` timestamps are stripped for plain-text display. `None` when no
/// lyrics are found.
pub(crate) fn fetch_lyrics_blocking(db: &DbHandle, file_path: &str) -> Option<String> {
    // Embedded tag lyrics first (no network round-trip).
    let mut lyrics = metadata::extract_lyrics(file_path);

    if lyrics.is_none() {
        // Brief DB lock for the track lookup; released before the network call.
        let track = {
            let conn = db.lock().unwrap_or_else(PoisonError::into_inner);
            queries::get_track_by_path(&conn, file_path).ok().flatten()
        };
        if let Some(track) = track {
            if let (Some(artist), Some(title)) = (&track.artist, &track.title) {
                lyrics = lrclib::fetch_lyrics_with_fallback(artist, title, track.album.as_deref());
            }
        }
    }

    lyrics.map(|raw| strip_lrc_timestamps(&raw))
}

/// Strip LRC `[mm:ss.xx]` timestamps for plain-text display, preserving blank
/// lines. Text without `[` brackets is returned unchanged.
fn strip_lrc_timestamps(raw: &str) -> String {
    if !raw.contains('[') {
        return raw.to_string();
    }
    raw.lines()
        .filter_map(|line| {
            let t = line.trim();
            if t.is_empty() {
                return Some(String::new());
            }
            if let Some(pos) = t.find(']') {
                let after = t[pos + 1..].trim();
                if after.is_empty() {
                    return None;
                }
                return Some(after.to_string());
            }
            Some(t.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::strip_lrc_timestamps;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(strip_lrc_timestamps("hello\nworld"), "hello\nworld");
    }

    #[test]
    fn lrc_timestamps_stripped_blanks_kept() {
        let lrc = "[00:01.00]line one\n\n[00:05.00]line two";
        assert_eq!(strip_lrc_timestamps(lrc), "line one\n\nline two");
    }

    #[test]
    fn timestamp_only_lines_dropped() {
        let lrc = "[00:01.00]\n[00:02.00]kept";
        assert_eq!(strip_lrc_timestamps(lrc), "kept");
    }
}
