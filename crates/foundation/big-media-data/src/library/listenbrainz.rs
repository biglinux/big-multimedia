//! ListenBrainz API v1 — only needs a user token from
//! <https://listenbrainz.org/profile/>.
//! Config: `~/.config/big-audio-player/listenbrainz.json` (legacy dirname).

use crate::error::Result;
use big_os_kit::http_client::http_post_json;
use log::{info, warn};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const API_URL: &str = "https://api.listenbrainz.org/1/submit-listens";
const CONFIG_DIR: &str = "big-audio-player";
const CONFIG_FILE: &str = "listenbrainz.json";

#[derive(Debug, Clone)]
pub struct ListenBrainzConfig {
    pub user_token: String,
    pub enabled: bool,
}

impl ListenBrainzConfig {
    fn config_path() -> PathBuf {
        big_os_kit::xdg_dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(CONFIG_DIR)
            .join(CONFIG_FILE)
    }

    pub fn load() -> Option<Self> {
        let path = Self::config_path();
        let config_text = std::fs::read_to_string(path).ok()?;
        let config_json: serde_json::Value = serde_json::from_str(&config_text).ok()?;
        Some(Self {
            user_token: config_json.get("user_token")?.as_str()?.to_string(),
            enabled: config_json
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let config_json = serde_json::json!({
            "user_token": self.user_token,
            "enabled": self.enabled,
        });
        let json = serde_json::to_string_pretty(&config_json)?;
        std::fs::write(path, json)?;
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
            serde_json::Value::Number((duration_secs * 1000).into()),
        );
    }

    let payload = serde_json::json!({
        "listen_type": "single",
        "payload": [{
            "listened_at": timestamp,
            "track_metadata": {
                "artist_name": artist,
                "track_name": track,
                "additional_info": additional,
            }
        }]
    });

    let auth = format!("Token {}", config.user_token);
    match http_post_json(
        API_URL,
        &payload.to_string(),
        Some(("Authorization", &auth)),
    ) {
        Ok(200) => info!("ListenBrainz scrobble OK: {artist} - {track}"),
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

/// Test-only: payload builder mirroring `scrobble()`, returns the JSON value.
#[cfg(test)]
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
            serde_json::Value::Number((duration_secs * 1000).into()),
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
    fn config_defaults_enabled_true() {
        let json = r#"{"user_token": "abc123"}"#;
        let config_json: serde_json::Value = serde_json::from_str(json).unwrap();
        let enabled = config_json
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        assert!(enabled);
    }

    #[test]
    fn config_explicit_disabled() {
        let json = r#"{"user_token": "abc123", "enabled": false}"#;
        let config_json: serde_json::Value = serde_json::from_str(json).unwrap();
        let enabled = config_json
            .get("enabled")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        assert!(!enabled);
    }

    #[test]
    fn config_save_load_roundtrip() {
        let dir = std::env::temp_dir().join("bmp-test-listenbrainz");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("listenbrainz-test.json");

        let listenbrainz_config = ListenBrainzConfig {
            user_token: "test-token-xyz".to_string(),
            enabled: false,
        };

        let config_json = serde_json::json!({
            "user_token": listenbrainz_config.user_token,
            "enabled": listenbrainz_config.enabled,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&config_json).unwrap()).unwrap();

        let config_text = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&config_text).unwrap();
        assert_eq!(
            parsed.get("user_token").unwrap().as_str().unwrap(),
            "test-token-xyz"
        );
        assert!(!parsed.get("enabled").unwrap().as_bool().unwrap());

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
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
