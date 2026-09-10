//! Last.fm 2.0 API scrobbling — needs api_key + secret + session_key.
//! Config: `~/.config/big-audio-player/lastfm.json` (legacy app dirname).

use crate::error::Result;
use big_os_kit::http_client::http_post_form;
use log::{info, warn};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const CONFIG_DIR: &str = "big-audio-player";
const CONFIG_FILE: &str = "lastfm.json";

#[derive(Debug, Clone)]
pub struct LastFmConfig {
    pub api_key: String,
    pub api_secret: String,
    pub session_key: String,
    pub username: String,
    pub enabled: bool,
}

impl LastFmConfig {
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
            api_key: config_json.get("api_key")?.as_str()?.to_string(),
            api_secret: config_json.get("api_secret")?.as_str()?.to_string(),
            session_key: config_json.get("session_key")?.as_str()?.to_string(),
            username: config_json.get("username")?.as_str()?.to_string(),
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
            "api_key": self.api_key,
            "api_secret": self.api_secret,
            "session_key": self.session_key,
            "username": self.username,
            "enabled": self.enabled,
        });
        let json = serde_json::to_string_pretty(&config_json)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

/// `md5(key1value1key2value2...secret)` — keys must be alphabetically sorted
/// (BTreeMap iteration order satisfies this).
fn api_sig(params: &BTreeMap<&str, &str>, secret: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    for (parameter_key, parameter_value) in params {
        hasher.update(parameter_key.as_bytes());
        hasher.update(parameter_value.as_bytes());
    }
    hasher.update(secret.as_bytes());
    let signature_bytes = hasher.finalize();
    signature_bytes
        .iter()
        .fold(String::new(), |mut signature_hex, byte| {
            use std::fmt::Write;
            let _ = write!(signature_hex, "{byte:02x}");
            signature_hex
        })
}

/// Blocking — call from a worker thread.
pub fn scrobble(config: &LastFmConfig, artist: &str, track: &str, album: &str, duration_secs: i64) {
    if !config.enabled || artist.is_empty() || track.is_empty() {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    let duration_seconds_text = duration_secs.to_string();

    let mut params = BTreeMap::new();
    params.insert("method", "track.scrobble");
    params.insert("api_key", &config.api_key);
    params.insert("sk", &config.session_key);
    params.insert("artist", artist);
    params.insert("track", track);
    if !album.is_empty() {
        params.insert("album", album);
    }
    params.insert("timestamp", &timestamp);
    if duration_secs > 0 {
        params.insert("duration", &duration_seconds_text);
    }

    let api_signature = api_sig(&params, &config.api_secret);
    params.insert("api_sig", &api_signature);
    params.insert("format", "json");

    let body: String = params
        .iter()
        .map(|(parameter_key, parameter_value)| {
            format!(
                "{}={}",
                parameter_key,
                big_os_kit::url::encode_query_value(parameter_value),
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    match http_post_form(API_URL, &body) {
        Ok(200) => info!("Last.fm scrobble OK: {artist} - {track}"),
        Ok(code) => warn!("Last.fm scrobble failed: HTTP {code}"),
        Err(e) => warn!("Last.fm scrobble error: {e}"),
    }
}

pub fn update_now_playing(config: &LastFmConfig, artist: &str, track: &str, album: &str) {
    if !config.enabled || artist.is_empty() || track.is_empty() {
        return;
    }

    let mut params = BTreeMap::new();
    params.insert("method", "track.updateNowPlaying");
    params.insert("api_key", &config.api_key);
    params.insert("sk", &config.session_key);
    params.insert("artist", artist);
    params.insert("track", track);
    if !album.is_empty() {
        params.insert("album", album);
    }

    let api_signature = api_sig(&params, &config.api_secret);
    params.insert("api_sig", &api_signature);
    params.insert("format", "json");

    let body: String = params
        .iter()
        .map(|(parameter_key, parameter_value)| {
            format!(
                "{}={}",
                parameter_key,
                big_os_kit::url::encode_query_value(parameter_value),
            )
        })
        .collect::<Vec<_>>()
        .join("&");

    let _ = http_post_form(API_URL, &body);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_sig_known_values() {
        let params = BTreeMap::new();
        let api_signature = api_sig(&params, "");
        assert_eq!(api_signature, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn api_sig_with_params() {
        let mut params = BTreeMap::new();
        params.insert("method", "track.scrobble");
        params.insert("api_key", "testkey");
        let api_signature = api_sig(&params, "secret");
        assert_eq!(api_signature.len(), 32);
        assert!(api_signature.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
