//! ListenBrainz API v1 — only needs a user token from
//! <https://listenbrainz.org/profile/>.
//! Config: `~/.config/big-audio-player/listenbrainz.json` (legacy dirname).

use crate::error::Result;
use big_os_kit::http_client::http_post_json;
use log::{info, warn};
#[cfg(test)]
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const API_URL: &str = "https://api.listenbrainz.org/1/submit-listens";

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
    fn from_json(text: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let enabled = match value.get("enabled") {
            None => false, Some(serde_json::Value::Bool(value)) => *value, _ => return None,
        };
        Some(Self {
            user_token: value.get("user_token")?.as_str()?.to_string(),
            // Possession of a token is not consent to publish listening history.
            enabled,
        })
    }

    pub fn try_load() -> Result<Option<Self>> {
        let Some(bytes) = super::accounts::load(super::accounts::Provider::ListenBrainz)? else { return Ok(None); };
        let config = Self::from_json(std::str::from_utf8(&bytes)?)
            .ok_or_else(|| crate::Error::internal("Invalid ListenBrainz configuration"))?;
        if config.user_token.len() > 4096 || config.user_token.chars().any(char::is_control) {
            return Err(crate::Error::internal("Invalid ListenBrainz token"));
        }
        Ok(Some(config))
    }
    /// Legacy read-only adapter. Editing flows must use try_load.
    pub fn load() -> Option<Self> { Self::try_load().ok().flatten() }

    /// Explicit worker-only save. Secret Service failure never falls back to a file.
    pub fn save(&self) -> Result<()> {
        let provider = super::accounts::Provider::ListenBrainz;
        let reviewed = super::accounts::review(provider)?;
        if reviewed.kind != super::accounts::StorageKind::NotConfigured {
            return Err(crate::Error::internal("Editing existing credentials requires accounts::save_reviewed with the loaded revision"));
        }
        let bytes = serde_json::to_vec(&serde_json::json!({"user_token":self.user_token,"enabled":self.enabled}))?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        match super::accounts::save_reviewed(&reviewed,&bytes,&cancel)? {
            super::accounts::SaveOutcome::Protected => Ok(()),
            super::accounts::SaveOutcome::ProtectedCleanupPending => Err(crate::Error::internal("Account protected; old credential cleanup needs attention")),
        }
    }

    #[cfg(test)]
    fn save_to_path(&self, path: &Path) -> Result<()> {
        if self.user_token.len() > 4096 || self.user_token.chars().any(char::is_control) {
            return Err(crate::Error::internal("Invalid ListenBrainz token"));
        }
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
    started_at: u64,
) {
    if !config.enabled || config.user_token.is_empty() || artist.is_empty() || track.is_empty()
        || [artist, track, album].iter().any(|s| s.len() > 4096) {
        return;
    }

    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else { return; };
    if started_at == 0 || started_at > now.as_secs() { return; }
    let timestamp = started_at;

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
    if !config.enabled || config.user_token.is_empty() || artist.is_empty() || track.is_empty()
        || [artist, track, album].iter().any(|s| s.len() > 4096) {
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
        scrobble(&listenbrainz_config, "A", "T", "Al", 100, 1);
    }

    #[test]
    fn scrobble_guard_empty_token() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: String::new(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "A", "T", "Al", 100, 1);
    }

    #[test]
    fn scrobble_guard_empty_artist() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "", "T", "Al", 100, 1);
    }

    #[test]
    fn scrobble_guard_empty_track() {
        let listenbrainz_config = ListenBrainzConfig {
            user_token: "tok".to_string(),
            enabled: true,
        };
        scrobble(&listenbrainz_config, "A", "", "Al", 100, 1);
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
