//! On-disk cache for IPTV playlists fetched over HTTP.
//!
//! Fetching an M3U/Xtream list can be slow (5–30s on weak providers), so the
//! IPTV browser opens by showing the cached copy immediately and triggers a
//! background refresh. The refresh only replaces the cache if the new fetch
//! succeeds.
//!
//! Cache location: `$XDG_CACHE_HOME/big-video-player/iptv/<SHA-256 of the
//! provider identifier>.m3u`, readable by its owner only. Xtream lists are
//! stored without the account's credentials.
//!
//! Engine-free helper (no GTK widgets) used by [`super::VideoIptvBrowser`].

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once, PoisonError};

use crate::config;
use crate::video::redaction::redact_path;

/// The cache directory, cleared of files named the old way on first use.
fn cache_dir() -> Option<PathBuf> {
    static REMOVE_LEGACY: Once = Once::new();
    let dir = big_os_kit::xdg_dirs::cache_dir()?
        .join(config::video::CONFIG_DIR)
        .join("iptv");
    REMOVE_LEGACY.call_once(|| remove_legacy_files(&dir));
    Some(dir)
}

fn file_name(provider_id: &str) -> Option<String> {
    let digest = glib::compute_checksum_for_string(glib::ChecksumType::Sha256, provider_id)?;
    Some(format!("{digest}.m3u"))
}

fn is_cache_file_name(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_suffix(".m3u"))
        .is_some_and(|stem| {
            stem.len() == 64 && stem.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        })
}

/// Delete the lists cached under a slug of the provider's URL: they hold its
/// credentials, in files other users may read.
fn remove_legacy_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let legacy = Path::new(&name)
            .extension()
            .is_some_and(|extension| extension == "m3u")
            && !is_cache_file_name(&name);
        // The legacy name can hold the credentials: it stays out of the log.
        if legacy && let Err(error) = fs::remove_file(entry.path()) {
            log::warn!("iptv cache cleanup of a legacy list: {error}");
        }
    }
}

fn cache_path(provider_id: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(file_name(provider_id)?))
}

/// Read the cached playlist content if it exists.
#[must_use]
pub(super) fn read(provider_id: &str) -> Option<String> {
    fs::read_to_string(cache_path(provider_id)?).ok()
}

/// Provider identities whose list may hold credentials, retired by moving
/// them into the keyring. Held across each write, so a fetch that started
/// before the retirement cannot write that list again after it.
static RETIRED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Delete the cached playlist of `provider_id` for good: a fetch still running
/// under that identity does not write it again.
pub(super) fn retire(provider_id: &str) {
    let mut retired = RETIRED.lock().unwrap_or_else(PoisonError::into_inner);
    retired.insert(provider_id.to_owned());
    remove(provider_id);
}

/// Delete the cached playlist of `provider_id` (best-effort).
fn remove(provider_id: &str) {
    let Some(path) = cache_path(provider_id) else {
        return;
    };
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("iptv cache remove {}: {error}", redact_path(&path)),
    }
}

/// Write the fetched content to the cache (best-effort).
pub(super) fn write(provider_id: &str, content: &str) {
    let retired = RETIRED.lock().unwrap_or_else(PoisonError::into_inner);
    if retired.contains(provider_id) {
        return;
    }
    let Some(path) = cache_path(provider_id) else {
        log::warn!("iptv cache write: no XDG cache directory");
        return;
    };
    if let Err(e) = big_os_kit::storage::atomic_write_private(&path, content.as_bytes()) {
        // PII: redact user-home path.
        log::warn!("iptv cache write {}: {e}", redact_path(&path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_provider_has_its_file() {
        let shared = format!("m3u_List_https://h/{}", "a".repeat(100));
        let first = file_name(&format!("{shared}?password=one")).unwrap();
        let second = file_name(&format!("{shared}?password=two")).unwrap();
        assert_ne!(first, second);
        for name in [&first, &second] {
            assert!(is_cache_file_name(OsStr::new(name)), "{name}");
            assert!(!name.contains("password"), "{name}");
        }
        // Oracle: `printf %s abc | sha256sum`.
        assert_eq!(
            file_name("abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad.m3u"
        );
    }

    #[test]
    fn slug_named_lists_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let current = file_name("m3u_List_https://h/list.m3u").unwrap();
        let names = [
            current.as_str(),
            "m3u_list_https___h_get.php_username_u_password_p.m3u",
            "unnamed.m3u",
            "notes.txt",
        ];
        for name in names {
            fs::write(dir.path().join(name), b"#EXTM3U\n").unwrap();
        }
        remove_legacy_files(dir.path());
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, [current.as_str(), "notes.txt"]);
    }
}
