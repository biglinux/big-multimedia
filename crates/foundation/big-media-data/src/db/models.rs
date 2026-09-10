#[derive(Debug, Clone)]
pub struct Track {
    pub id: i64,
    pub file_path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_secs: f64,
    pub file_size: Option<i64>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub format: Option<String>,
    pub art_hash: Option<String>,
    pub date_added: i64,
    pub last_played: Option<i64>,
    pub play_count: i32,
    pub rating: i32,
    pub modified_at: Option<i64>,
}

impl Track {
    #[must_use]
    pub fn new_external_path(file_path: impl Into<String>) -> Self {
        Self::new_external_path_with_title(file_path, None)
    }

    #[must_use]
    pub fn new_external_path_with_title(
        file_path: impl Into<String>,
        title: Option<String>,
    ) -> Self {
        Self {
            id: 0,
            file_path: file_path.into(),
            title,
            artist: None,
            album: None,
            album_artist: None,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: 0.0,
            file_size: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: None,
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    #[must_use]
    pub fn new_podcast_episode(
        id: i64,
        audio_url: impl Into<String>,
        episode_title: impl Into<String>,
        podcast_title: impl Into<String>,
        track_number: i32,
    ) -> Self {
        Self {
            id,
            file_path: audio_url.into(),
            title: Some(episode_title.into()),
            artist: Some(podcast_title.into()),
            album: None,
            album_artist: None,
            track_number: Some(track_number),
            disc_number: None,
            year: None,
            genre: Some("Podcast".to_string()),
            duration_secs: 0.0,
            file_size: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: None,
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    #[must_use]
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or_else(|| {
            std::path::Path::new(&self.file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&self.file_path)
        })
    }

    #[must_use]
    pub fn display_artist(&self) -> &str {
        self.artist.as_deref().unwrap_or("")
    }

    #[must_use]
    pub fn display_album(&self) -> &str {
        self.album.as_deref().unwrap_or("")
    }

    #[must_use]
    pub fn duration_display(&self) -> String {
        let total = self.duration_secs as u64;
        let min = total / 60;
        let sec = total % 60;
        format!("{min}:{sec:02}")
    }
}

// The playback-queue primitive lives in the GTK-free engines leaf; `Track` is
// local to this crate, so the `QueueEntry` impl belongs here (orphan rule).
impl big_media_engines::queue::QueueEntry for Track {
    fn queue_key(&self) -> &str {
        &self.file_path
    }
}

#[derive(Debug, Clone)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct WatchFolder {
    pub id: i64,
    pub path: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct SmartPlaylist {
    pub id: i64,
    pub name: String,
    pub rules_json: String,
    pub logic: String,
    pub sort_field: Option<String>,
    pub sort_dir: String,
    pub max_tracks: i32,
}

#[derive(Debug, Clone)]
pub struct RadioStation {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub favicon: String,
    pub group_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_track(
        title: Option<&str>,
        artist: Option<&str>,
        album: Option<&str>,
        duration: f64,
        file_path: &str,
    ) -> Track {
        Track {
            id: 1,
            file_path: file_path.to_string(),
            title: title.map(String::from),
            artist: artist.map(String::from),
            album: album.map(String::from),
            album_artist: None,
            track_number: None,
            disc_number: None,
            year: None,
            genre: None,
            duration_secs: duration,
            file_size: None,
            bitrate: None,
            sample_rate: None,
            channels: None,
            format: None,
            art_hash: None,
            date_added: 0,
            last_played: None,
            play_count: 0,
            rating: 0,
            modified_at: None,
        }
    }

    #[test]
    fn display_title_returns_title_when_present() {
        let t = make_track(Some("My Song"), None, None, 0.0, "/music/file.mp3");
        assert_eq!(t.display_title(), "My Song");
    }

    #[test]
    fn display_title_falls_back_to_file_stem() {
        let t = make_track(None, None, None, 0.0, "/music/cool_track.flac");
        assert_eq!(t.display_title(), "cool_track");
    }

    #[test]
    fn display_title_falls_back_to_full_path() {
        let t = make_track(None, None, None, 0.0, "");
        assert_eq!(t.display_title(), "");
    }

    #[test]
    fn display_title_empty_title_treated_as_some() {
        let t = make_track(Some(""), None, None, 0.0, "/music/fallback.mp3");
        assert_eq!(t.display_title(), "");
    }

    #[test]
    fn display_artist_returns_artist() {
        let t = make_track(None, Some("The Band"), None, 0.0, "x.mp3");
        assert_eq!(t.display_artist(), "The Band");
    }

    #[test]
    fn display_artist_empty_when_none() {
        let t = make_track(None, None, None, 0.0, "x.mp3");
        assert_eq!(t.display_artist(), "");
    }

    #[test]
    fn display_album_returns_album() {
        let t = make_track(None, None, Some("Greatest Hits"), 0.0, "x.mp3");
        assert_eq!(t.display_album(), "Greatest Hits");
    }

    #[test]
    fn display_album_empty_when_none() {
        let t = make_track(None, None, None, 0.0, "x.mp3");
        assert_eq!(t.display_album(), "");
    }

    #[test]
    fn duration_display_zero() {
        let t = make_track(None, None, None, 0.0, "x.mp3");
        assert_eq!(t.duration_display(), "0:00");
    }

    #[test]
    fn duration_display_seconds_only() {
        let t = make_track(None, None, None, 42.0, "x.mp3");
        assert_eq!(t.duration_display(), "0:42");
    }

    #[test]
    fn duration_display_minutes_and_seconds() {
        let t = make_track(None, None, None, 185.0, "x.mp3");
        assert_eq!(t.duration_display(), "3:05");
    }

    #[test]
    fn duration_display_pads_seconds() {
        let t = make_track(None, None, None, 61.0, "x.mp3");
        assert_eq!(t.duration_display(), "1:01");
    }

    #[test]
    fn duration_display_over_an_hour() {
        let t = make_track(None, None, None, 3661.0, "x.mp3");
        assert_eq!(t.duration_display(), "61:01");
    }

    #[test]
    fn duration_display_truncates_fractional() {
        let t = make_track(None, None, None, 59.9, "x.mp3");
        assert_eq!(t.duration_display(), "0:59");
    }

    #[test]
    fn duration_display_exact_minute() {
        let t = make_track(None, None, None, 120.0, "x.mp3");
        assert_eq!(t.duration_display(), "2:00");
    }

    #[test]
    fn new_external_path_defaults_blank_metadata() {
        let t = Track::new_external_path("/tmp/song.mp3");
        assert_eq!(t.id, 0);
        assert_eq!(t.file_path, "/tmp/song.mp3");
        assert!(t.title.is_none());
        assert_eq!(t.duration_secs, 0.0);
    }

    #[test]
    fn new_podcast_episode_sets_expected_fields() {
        let t = Track::new_podcast_episode(-2, "https://x/ep.mp3", "Episode", "Feed", 2);
        assert_eq!(t.id, -2);
        assert_eq!(t.title.as_deref(), Some("Episode"));
        assert_eq!(t.artist.as_deref(), Some("Feed"));
        assert_eq!(t.track_number, Some(2));
        assert_eq!(t.genre.as_deref(), Some("Podcast"));
    }
}
