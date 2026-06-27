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
    #[cfg(not(miri))]
    use super::fetch_lyrics_blocking;
    use super::strip_lrc_timestamps;
    #[cfg(not(miri))]
    use crate::db::{schema, DbHandle};
    #[cfg(not(miri))]
    use big_media_runtime::Result as MediaResult;
    #[cfg(not(miri))]
    use rusqlite::Connection;
    #[cfg(not(miri))]
    use std::fs;
    #[cfg(not(miri))]
    use std::sync::{Arc, Mutex};
    #[cfg(not(miri))]
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(not(miri))]
    struct LyricsFixtureFolder {
        path: std::path::PathBuf,
    }

    #[cfg(not(miri))]
    impl LyricsFixtureFolder {
        fn create() -> std::result::Result<Self, Box<dyn std::error::Error>> {
            let unique_suffix = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            let path = std::env::temp_dir().join(format!(
                "big-audio-lyrics-fetch-{}-{unique_suffix}",
                std::process::id()
            ));
            fs::create_dir_all(&path)?;
            Ok(Self { path })
        }
    }

    #[cfg(not(miri))]
    impl Drop for LyricsFixtureFolder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(not(miri))]
    fn in_memory_library_database() -> MediaResult<DbHandle> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::run_migrations(&connection)?;
        Ok(Arc::new(Mutex::new(connection)))
    }

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

    #[cfg(not(miri))]
    #[test]
    fn fetch_lyrics_prefers_sidecar_lrc_and_returns_plain_text(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let library_database = in_memory_library_database()?;
        let lyrics_fixture_folder = LyricsFixtureFolder::create()?;
        let audio_path = lyrics_fixture_folder.path.join("song.mp3");
        let sidecar_path = lyrics_fixture_folder.path.join("song.lrc");
        fs::write(&audio_path, [])?;
        fs::write(
            &sidecar_path,
            "[00:01.00]first line\n[00:02.00]\n[00:03.00]second line",
        )?;

        let lyrics = fetch_lyrics_blocking(&library_database, &audio_path.to_string_lossy());
        assert_eq!(
            lyrics.as_deref(),
            Some("first line\nsecond line"),
            "sidecar LRC should bypass network lookup and be stripped to plain text"
        );

        Ok(())
    }
}
