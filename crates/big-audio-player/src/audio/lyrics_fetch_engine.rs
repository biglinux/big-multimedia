//! Pure (GTK-free) lyrics fetch for the Relm4 `AudioShell`.
//!
//! GTK-free so the shell can run it off-thread. Embedded tag lyrics first
//! (no network); LRCLIB + lyrics.ovh as a fallback. The shell wraps it in
//! `gio::spawn_blocking` and pushes the plain-text result into the panel.

use std::sync::PoisonError;

use crate::db::DbHandle;
use crate::db::queries;
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
        if let Some(track) = track
            && let (Some(artist), Some(title)) = (&track.artist, &track.title)
        {
            lyrics = lrclib::fetch_lyrics_with_fallback(artist, title, track.album.as_deref());
        }
    }

    lyrics.map(|raw| strip_lrc_timestamps(&raw))
}

/// The rest of `line` after its leading `[mm:ss]` or `[mm:ss.xx]` stamp.
fn after_lrc_stamp(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let minutes = rest.bytes().take_while(u8::is_ascii_digit).count();
    let rest = rest
        .get(minutes..)?
        .strip_prefix(':')
        .filter(|_| minutes > 0)?;
    let (seconds, mut rest) = rest.split_at_checked(2)?;
    if !seconds.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if !(2..=3).contains(&digits) {
            return None;
        }
        rest = &fraction[digits..];
    }
    rest.strip_prefix(']')
}

/// Strip the leading LRC time stamps of each line for plain-text display,
/// dropping lines that held only stamps and keeping blank ones. Text with no
/// line starting with a stamp is returned unchanged.
fn strip_lrc_timestamps(raw: &str) -> String {
    if !raw
        .lines()
        .any(|line| after_lrc_stamp(line.trim_start()).is_some())
    {
        return raw.to_string();
    }
    let mut lines = Vec::new();
    for line in raw.split('\n') {
        let (mut text, mut stamped) = (line.trim(), false);
        while let Some(rest) = after_lrc_stamp(text) {
            (text, stamped) = (rest, true);
        }
        let text = text.trim();
        if !(stamped && text.is_empty()) {
            lines.push(text);
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    #[cfg(not(miri))]
    use super::fetch_lyrics_blocking;
    use super::strip_lrc_timestamps;
    #[cfg(not(miri))]
    use crate::db::{DbHandle, schema};
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

    #[test]
    fn brackets_in_lyric_text_kept() {
        let lrc = "[00:01.00]Hello [x] world\n[00:01.00][00:20.00]Chorus\n  Untimed [y]  ";
        assert_eq!(
            strip_lrc_timestamps(lrc),
            "Hello [x] world\nChorus\nUntimed [y]"
        );
        assert_eq!(strip_lrc_timestamps("Hello [x] world"), "Hello [x] world");
    }

    /// xorshift64, so every run walks the same inputs.
    struct Rng(u64);

    impl Rng {
        fn below(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % bound as u64) as usize
        }

        fn digits(&mut self, count: usize) -> String {
            (0..count)
                .map(|_| char::from(b'0' + self.below(10) as u8))
                .collect()
        }
    }

    /// `[mm:ss]` or `[mm:ss.xx]`, as the LRC format writes a line's time.
    fn stamp(rng: &mut Rng) -> String {
        let width = 1 + rng.below(3);
        let minutes = rng.digits(width);
        let seconds = format!("{:02}", rng.below(60));
        let fraction = match rng.below(3) {
            0 => String::new(),
            1 => format!(".{}", rng.digits(2)),
            _ => format!(".{}", rng.digits(3)),
        };
        format!("[{minutes}:{seconds}{fraction}]")
    }

    /// Lyric text that never starts with a stamp, brackets and colons included.
    fn text(rng: &mut Rng) -> String {
        const TOKENS: [&str; 12] = [
            "Hello", "world", " ", "[x]", "[chorus]", "]", "[", ":", "00", "[1:2]", "日本", "é",
        ];
        (0..rng.below(6))
            .map(|_| TOKENS[rng.below(TOKENS.len())])
            .collect()
    }

    #[test]
    fn stamped_lines_keep_their_text() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..5000 {
            let (mut lines, mut expected, mut any_stamp) = (Vec::new(), Vec::new(), false);
            for _ in 0..=rng.below(5) {
                let body = text(&mut rng);
                if rng.below(3) == 0 {
                    lines.push(body.clone());
                    expected.push(body.trim().to_owned());
                    continue;
                }
                any_stamp = true;
                let stamps: String = (0..=rng.below(3)).map(|_| stamp(&mut rng)).collect();
                lines.push(format!("{stamps}{body}"));
                if !body.trim().is_empty() {
                    expected.push(body.trim().to_owned());
                }
            }
            let raw = lines.join("\n");
            let wanted = if any_stamp {
                expected.join("\n")
            } else {
                raw.clone()
            };
            assert_eq!(strip_lrc_timestamps(&raw), wanted, "input {raw:?}");
        }
    }

    #[cfg(not(miri))]
    #[test]
    fn sidecar_lrc_wins_as_plain_text() -> std::result::Result<(), Box<dyn std::error::Error>> {
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
