//! Last.fm 2.0 API scrobbling — needs api_key + secret + session_key.
//! Config: `~/.config/big-audio-player/lastfm.json` (legacy app dirname).

use crate::error::Result;
use big_os_kit::http_client::http_post_form_response;
use log::{info, warn};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";

#[derive(Clone)]
pub struct LastFmConfig {
    pub api_key: String,
    pub api_secret: String,
    pub session_key: String,
    pub username: String,
    pub enabled: bool,
}

impl std::fmt::Debug for LastFmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LastFmConfig").field("credentials", &"[redacted]")
            .field("enabled", &self.enabled).finish()
    }
}
impl LastFmConfig {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)?;
        let field = |name: &str| -> Result<String> {
            value.get(name).and_then(serde_json::Value::as_str)
                .filter(|text| text.len() <= 4096 && !text.chars().any(char::is_control))
                .map(str::to_owned).ok_or_else(|| crate::Error::internal("Invalid Last.fm configuration field"))
        };
        let enabled = match value.get("enabled") {
            None => false, Some(serde_json::Value::Bool(v)) => *v,
            _ => return Err(crate::Error::internal("Invalid Last.fm consent flag")),
        };
        Ok(Self { api_key: field("api_key")?, api_secret: field("api_secret")?,
            session_key: field("session_key")?, username: field("username")?, enabled })
    }
    /// Distinguishes absence from corrupt/oversized/symlink configuration.
    pub fn try_load() -> Result<Option<Self>> {
        super::accounts::load(super::accounts::Provider::LastFm)?
            .map(|bytes| Self::from_bytes(&bytes)).transpose()
    }
    /// Legacy read-only adapter. Editors must use try_load and refuse to
    /// overwrite a configuration that could not be read.
    pub fn load() -> Option<Self> { Self::try_load().ok().flatten() }
    /// Worker-only save to Secret Service; never writes plaintext credentials.
    pub fn save(&self) -> Result<()> {
        let reviewed = super::accounts::review(super::accounts::Provider::LastFm)?;
        if reviewed.kind != super::accounts::StorageKind::NotConfigured {
            return Err(crate::Error::internal("Editing existing credentials requires accounts::save_reviewed with the loaded revision"));
        }
        let bytes = serde_json::to_vec(&serde_json::json!({
            "api_key":self.api_key,"api_secret":self.api_secret,"session_key":self.session_key,
            "username":self.username,"enabled":self.enabled,
        }))?;
        let cancel = std::sync::atomic::AtomicBool::new(false);
        match super::accounts::save_reviewed(&reviewed,&bytes,&cancel)? {
            super::accounts::SaveOutcome::Protected => Ok(()),
            super::accounts::SaveOutcome::ProtectedCleanupPending => Err(crate::Error::internal("Account protected; old credential cleanup needs attention")),
        }
    }
}

fn provider_accepted(status: u16, bytes: &[u8], scrobble: bool) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else { return false; };
    if status != 200 || value.get("error").is_some() { return false; }
    if scrobble {
        return value.pointer("/scrobbles/@attr/accepted")
            .is_some_and(|accepted| accepted.as_str() == Some("1") || accepted.as_u64() == Some(1));
    }
    value.get("nowplaying").is_some()
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
pub fn scrobble(config: &LastFmConfig, artist: &str, track: &str, album: &str, duration_secs: i64, started_at: u64) {
    if !config.enabled || config.api_key.is_empty() || config.api_secret.is_empty() || config.session_key.is_empty() || artist.is_empty() || track.is_empty()
        || [artist, track, album].iter().any(|s| s.len() > 4096) {
        return;
    }

    // The caller supplies the actual start, not the time of submission. It is
    // also responsible for consent and actual listening eligibility.
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else { return; };
    if started_at == 0 || started_at > now.as_secs() || duration_secs <= 30 { return; }
    let timestamp = started_at.to_string();
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

    match http_post_form_response(API_URL, &body) {
        Ok(response) if provider_accepted(response.status, &response.bytes, true) => info!("Last.fm confirmed one scrobble"),
        Ok(_) => warn!("Last.fm did not confirm the scrobble"),
        Err(_) => warn!("Last.fm transport failed"),
    }
}

pub fn update_now_playing(config: &LastFmConfig, artist: &str, track: &str, album: &str) {
    if !config.enabled || config.api_key.is_empty() || config.api_secret.is_empty() || config.session_key.is_empty() || artist.is_empty() || track.is_empty()
        || [artist, track, album].iter().any(|s| s.len() > 4096) {
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

    match http_post_form_response(API_URL, &body) {
        Ok(response) if provider_accepted(response.status, &response.bytes, false) => {},
        _ => warn!("Last.fm did not confirm now-playing"),
    }
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

#[cfg(test)]
mod privacy_tests {
    use super::*;
    #[test]
    fn debug_does_not_disclose_secrets_and_missing_consent_is_off() {
        let config = LastFmConfig::from_bytes(br#"{"api_key":"private-key","api_secret":"private-secret","session_key":"private-session","username":"private-name"}"#).unwrap();
        assert!(!config.enabled);
        assert!(!format!("{config:?}").contains("private-"));
    }
    #[test]
    fn http_success_does_not_mean_provider_accepted() {
        assert!(!provider_accepted(200, br#"{"error":9,"message":"private details"}"#, true));
        assert!(!provider_accepted(200, br#"{"scrobbles":{"@attr":{"accepted":"0","ignored":"1"}}}"#, true));
        assert!(provider_accepted(200, br#"{"scrobbles":{"@attr":{"accepted":"1","ignored":"0"}}}"#, true));
    }
}
