//! ListenBrainz API v1 — only needs a user token from
//! <https://listenbrainz.org/profile/>.
//! Config: `~/.config/big-audio-player/listenbrainz.json` (legacy dirname).

use crate::error::Result;
use big_os_kit::http_client::http_post_json;
use log::{info, warn};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const API_URL: &str = "https://api.listenbrainz.org/1/submit-listens";
const CONFIG_DIR: &str = "big-audio-player";
const CONFIG_FILE: &str = "listenbrainz.json";

#[derive(Clone)]
pub struct ListenBrainzConfig {
    pub user_token: String,
    pub enabled: bool,
}

impl std::fmt::Debug for ListenBrainzConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenBrainzConfig")
            .field("user_token", &"[redacted]")
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl ListenBrainzConfig {
    fn config_path() -> Option<PathBuf> {
        Some(big_os_kit::xdg_dirs::config_dir()?.join(CONFIG_DIR).join(CONFIG_FILE))
    }

    fn from_json(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        Some(Self {
            user_token: value.get("user_token")?.as_str()?.to_string(),
            // Possession of a token is not consent to publish listening history.
            enabled: value.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(false),
        })
    }

    pub fn load() -> Option<Self> {
        Self::from_json(&std::fs::read_to_string(Self::config_path()?).ok()?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path().ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::NotFound, "user configuration directory is unavailable",
        ))?;
        self.save_to_path(&path)
    }

    fn save_to_path(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(&serde_json::json!({
            "user_token": self.user_token,
            "enabled": self.enabled,
        }))?;
        big_os_kit::storage::atomic_write_private(path, &json)?;
        Ok(())
    }
}

/// Blocking — call from a worker thread. `listen_type=single`.
pub fn scrobble(
    config: &ListenBrainzConfig,
    artist: &str,
    track: &str,
    album: &str,
    duration_secs: i64,
) {
    if !config.enabled || config.user_token.is_empty() || artist.is_empty() || track.is_empty() {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let payload = build_scrobble_payload(artist, track, album, duration_secs, timestamp);

    let auth = format!("Token {}", config.user_token);
    match http_post_json(
        API_URL,
        &payload.to_string(),
        Some(("Authorization", &auth)),
    ) {
        Ok(200) => info!("ListenBrainz listen submitted"),
        Ok(code) => warn!("ListenBrainz scrobble failed: HTTP {code}"),
        Err(e) => warn!("ListenBrainz scrobble error: {e}"),
    }
}

pub fn update_now_playing(config: &ListenBrainzConfig, artist: &str, track: &str, album: &str) {
    if !config.enabled || config.user_token.is_empty() || artist.is_empty() || track.is_empty() {
        return;
    }

    let mut additional = serde_json::Map::new();
    if !album.is_empty() {
        additional.insert(
            "release_name".into(),
            serde_json::Value::String(album.to_string()),
        );
    }

    let payload = serde_json::json!({
        "listen_type": "playing_now",
        "payload": [{
            "track_metadata": {
                "artist_name": artist,
                "track_name": track,
                "additional_info": additional,
            }
        }]
    });

    let auth = format!("Token {}", config.user_token);
    let _ = http_post_json(
        API_URL,
        &payload.to_string(),
        Some(("Authorization", &auth)),
    );
}

/// Shared production payload builder, exercised directly by unit tests.
fn build_scrobble_payload(
    artist: &str,
    track: &str,
    album: &str,
    duration_secs: i64,
    timestamp: u64,
) -> serde_json::Value {
    let mut additional = serde_json::Map::new();
    if !album.is_empty() {
        additional.insert(
            "release_name".into(),
            serde_json::Value::String(album.to_string()),
        );
    }
    if duration_secs > 0 {
        additional.insert(
            "duration_ms".into(),
            serde_json::Value::Number(duration_secs.saturating_mul(1000).into()),
        );
    }

    serde_json::json!({
        "listen_type": "single",
        "payload": [{
            "listened_at": timestamp,
            "track_metadata": {
                "artist_name": artist,
                "track_name": track,
                "additional_info": additional,
            }
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_consent_does_not_enable_listening_history() {
        assert!(!ListenBrainzConfig::from_json(r#"{"user_token":"abc"}"#).unwrap().enabled);
        assert!(!ListenBrainzConfig::from_json(r#"{"user_token":"abc","enabled":false}"#).unwrap().enabled);
        assert!(ListenBrainzConfig::from_json(r#"{"user_token":"abc","enabled":true}"#).unwrap().enabled);
    }

    #[cfg(unix)]
    #[test]
    fn actual_config_writer_is_atomic_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("listenbrainz.json");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let config = ListenBrainzConfig { user_token: "secret-token".into(), enabled: false };
        config.save_to_path(&path).unwrap();
        let loaded = ListenBrainzConfig::from_json(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded.user_token, config.user_token);
        assert!(!loaded.enabled);
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o077, 0);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(!format!("{config:?}").contains("secret-token"));
    }

    #[test]
    fn duration_conversion_cannot_overflow() {
        let payload = build_scrobble_payload("A", "T", "", i64::MAX, 1);
        assert_eq!(payload["payload"][0]["track_metadata"]["additional_info"]["duration_ms"], i64::MAX);
    }

    #[test]
    fn scrobble_payload_includes_album_and_duration() {
        let payload = build_scrobble_payload("Artist", "Track", "Album", 240, 1_700_000_000);
        assert_eq!(payload["listen_type"], "single");
        let entry = &payload["payload"][0];
        assert_eq!(entry["listened_at"], 1_700_000_000);
        let meta = &entry["track_metadata"];
        assert_eq!(meta["artist_name"], "Artist");
        assert_eq!(meta["track_name"], "Track");
        assert_eq!(meta["additional_info"]["release_name"], "Album");
        assert_eq!(meta["additional_info"]["duration_ms"], 240_000);
    }

    #[test]
    fn scrobble_payload_omits_empty_album() {
        let payload = build_scrobble_payload("Artist", "Track", "", 0, 100);
        let additional_metadata = &payload["payload"][0]["track_metadata"]["additional_info"];
        assert!(additional_metadata.get("release_name").is_none());
        assert!(additional_metadata.get("duration_ms").is_none());
    }

    #[test]
    fn scrobble_guard_disabled_config() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: false,
        };
        scrobble(&listenbrainz_config, "A", "T", "Al", 100);
    }

    #[test]
    fn scrobble_guard_empty_token() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: String::new(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "A", "T", "Al", 100);
    }

    #[test]
    fn scrobble_guard_empty_artist() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "", "T", "Al", 100);
    }

    #[test]
    fn scrobble_guard_empty_track() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "A", "", "Al", 100);
    }

    #[test]
    fn now_playing_guard_disabled() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: false,
        };
        update_now_playing(&listenbrainz_config, "A", "T", "Al");
    }

    #[test]
    fn now_playing_guard_empty_fields() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: true,
        };
        update_now_playing(&listenbrainz_config, "", "T", "Al");
        update_now_playing(&listenbrainz_config, "A", "", "Al");
    }
}
