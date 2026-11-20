use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use log::error;

use super::{Inner, keys};

const LIMIT: usize = 1024 * 1024;
use big_os_kit::storage;
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Mutex, OnceLock, Weak};

type Shared = Arc<RwLock<Inner>>;
static STORES: OnceLock<Mutex<HashMap<PathBuf, Weak<RwLock<Inner>>>>> = OnceLock::new();
// Includes memory-only handles as well as path-backed stores. Weak registration
// never keeps a closed settings document alive and performs no filesystem I/O.
static OBSERVED: OnceLock<Mutex<Vec<Weak<RwLock<Inner>>>>> = OnceLock::new();
fn observe(inner: &Shared) {
    let mut items = OBSERVED
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    items.retain(|item| item.strong_count() > 0);
    items.push(Arc::downgrade(inner));
}

pub(super) fn parse(content: &[u8]) -> io::Result<HashMap<String, String>> {
    let text = std::str::from_utf8(content)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "settings are not UTF-8"))?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid settings line"))?;
        let key = key.trim_end();
        let rejected = match super::schema::normalized(key, value) {
            Some(value) => values.insert(key.into(), value.into()).is_some(),
            None => true,
        };
        if rejected {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or duplicate setting",
            ));
        }
    }
    Ok(values)
}

pub(super) fn serialize(values: &HashMap<String, String>) -> io::Result<Vec<u8>> {
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    let mut bytes = Vec::new();
    for (key, value) in entries {
        if !super::schema::valid(key, value)
            || key.len() + value.len() + 2 > LIMIT.saturating_sub(bytes.len())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or excessive settings",
            ));
        }
        bytes.extend_from_slice(key.as_bytes());
        bytes.push(b'=');
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(b'\n');
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavePhase {
    Saved,
    Pending,
    Saving,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveStatus {
    pub phase: SavePhase,
    pub revision: u64,
    pub persisted_revision: u64,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Settings {
    inner: Arc<RwLock<Inner>>,
}

impl Settings {
    #[must_use]
    pub fn for_audio() -> Self {
        Self::new(crate::config::audio::CONFIG_DIR)
    }

    #[must_use]
    pub fn for_video() -> Self {
        Self::new(crate::config::video::CONFIG_DIR)
    }

    /// Stores at `~/.config/<app_name>/settings.conf`.
    #[must_use]
    pub fn new(app_name: &str) -> Self {
        let Some(config_dir) = big_os_kit::xdg_dirs::config_dir() else {
            return Self::memory_only();
        };
        Self::from_path(config_dir.join(app_name).join("settings.conf"))
    }

    /// Isolated unsaved state; cannot silently fall back to a shared temp path.
    #[must_use]
    pub fn memory_only() -> Self {
        let inner = Arc::new(RwLock::new(Inner {
            settings_values: keys::defaults(),
            path: PathBuf::new(),
            dirty: false,
            changed: HashSet::new(),
            load_error: None,
            revision: 0,
            persisted_revision: 0,
            save_queued: false,
            phase: SavePhase::Failed,
            save_error: Some("No private settings directory is available".into()),
            disk_lock: Arc::new(Mutex::new(())),
        }));
        observe(&inner);
        Self { inner }
    }

    pub(super) fn from_path(mut path: PathBuf) -> Self {
        // Normalize existing parents. Nonexistent parents retain an absolute
        // lexical path; this registry is not a universal hard-link alias map.
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
            && let Ok(parent) = fs::canonicalize(parent)
        {
            path = parent.join(name);
        }
        let stores = STORES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = stores
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        stores.retain(|_, value| value.strong_count() > 0);
        if let Some(inner) = stores.get(&path).and_then(Weak::upgrade) {
            return Self { inner };
        }
        let loaded = storage::read_regular_limited(&path, LIMIT);
        let mut values = keys::defaults();
        let load_error = match loaded.and_then(|bytes| bytes.map(|bytes| parse(&bytes)).transpose())
        {
            Ok(saved) => {
                values.extend(saved.unwrap_or_default());
                None
            }
            Err(error) => Some(error.to_string()),
        };
        let phase = if load_error.is_some() {
            SavePhase::Failed
        } else {
            SavePhase::Saved
        };
        let save_error = load_error.clone();
        let inner: Shared = Arc::new(RwLock::new(Inner {
            settings_values: values,
            path: path.clone(),
            dirty: false,
            changed: HashSet::new(),
            load_error,
            revision: 0,
            persisted_revision: 0,
            save_queued: false,
            phase,
            save_error,
            disk_lock: Arc::new(Mutex::new(())),
        }));
        observe(&inner);
        stores.insert(path, Arc::downgrade(&inner));
        Self { inner }
    }

    #[cfg(test)]
    pub(crate) fn defaults() -> std::collections::HashMap<String, String> {
        keys::defaults()
    }

    #[inline]
    #[allow(unused_variables, clippy::unused_self)]
    fn debug_check_key(&self, key: &str) {
        #[cfg(debug_assertions)]
        {
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !inner.settings_values.contains_key(key) {
                log::warn!("Settings: unknown key {key:?} — add it to the catalog in keys.rs");
            }
        }
    }

    #[must_use]
    pub fn string(&self, key: &str) -> String {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.settings_values.get(key).cloned().unwrap_or_default()
    }

    #[must_use]
    pub fn boolean(&self, key: &str) -> bool {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .is_some_and(|value| value == "true")
    }

    #[must_use]
    pub fn int(&self, key: &str) -> i32 {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0)
    }

    #[must_use]
    pub fn double(&self, key: &str) -> f64 {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner
            .settings_values
            .get(key)
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(0.0)
    }

    pub fn set_string(&self, key: &str, value: &str) -> bool {
        if !super::schema::valid(key, value) {
            // Callers treat writes as fire-and-forget; this is the one place a
            // rejected preference is visible.
            log::warn!("settings: rejected {key}={value:?}");
            return false;
        }
        self.debug_check_key(key);
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner
            .settings_values
            .get(key)
            .is_some_and(|old| old == value)
        {
            return true;
        }
        inner.settings_values.insert(key.into(), value.into());
        inner.changed.insert(key.into());
        inner.dirty = true;
        inner.revision = inner.revision.wrapping_add(1);
        inner.phase = SavePhase::Pending;
        drop(inner);
        let _ = self.request_flush();
        true
    }
    pub fn set_boolean(&self, key: &str, value: bool) -> bool {
        self.set_string(key, if value { "true" } else { "false" })
    }
    pub fn set_int(&self, key: &str, value: i32) -> bool {
        self.set_string(key, &value.to_string())
    }
    pub fn set_double(&self, key: &str, value: f64) -> bool {
        value.is_finite() && self.set_string(key, &value.to_string())
    }

    #[must_use]
    pub fn get_bool(&self, key: keys::BoolKey) -> bool {
        self.boolean(key.0)
    }

    /// The voice sensitivity when the noise reduction keeps only speech.
    #[must_use]
    pub fn noise_voice_only(&self) -> Option<u8> {
        self.get_bool(keys::NOISE_VOICE_ONLY).then(|| {
            u8::try_from(self.get_int(keys::NOISE_VOICE_SENSITIVITY).clamp(1, 5)).unwrap_or(3)
        })
    }

    /// Range of the Maximum Volume preference, in percent.
    pub const VOLUME_MAX_PERCENT_RANGE: std::ops::RangeInclusive<i32> = 100..=200;

    /// The volume ceiling in percent: the Maximum Volume preference kept within
    /// [`Self::VOLUME_MAX_PERCENT_RANGE`].
    #[must_use]
    pub fn volume_max_percent(&self) -> i32 {
        let range = Self::VOLUME_MAX_PERCENT_RANGE;
        self.get_int(keys::VOLUME_MAX)
            .clamp(*range.start(), *range.end())
    }

    /// The audio player's saved volume in percent, within 0 and
    /// [`Self::volume_max_percent`]; 0 stays 0.
    #[must_use]
    pub fn audio_volume_percent(&self) -> f64 {
        f64::from(
            self.get_int(keys::AUDIO_VOLUME)
                .clamp(0, self.volume_max_percent()),
        )
    }

    /// Store the audio player's volume, rounded to a whole percent.
    pub fn put_audio_volume_percent(&self, percent: f64) -> bool {
        percent.is_finite() && self.put_int(keys::AUDIO_VOLUME, percent.round() as i32)
    }

    /// The video player's saved volume in percent, within 0 and
    /// [`Self::volume_max_percent`]; 0 stays 0.
    #[must_use]
    pub fn video_volume_percent(&self) -> f64 {
        (self.get_double(keys::VOLUME) * 100.0).clamp(0.0, f64::from(self.volume_max_percent()))
    }

    /// Store the video player's volume, rounded to a whole percent and kept as
    /// a fraction of 100%.
    pub fn put_video_volume_percent(&self, percent: f64) -> bool {
        self.put_double(keys::VOLUME, percent.round() / 100.0)
    }

    #[must_use]
    pub fn get_int(&self, key: keys::IntKey) -> i32 {
        self.int(key.0)
    }

    #[must_use]
    pub fn get_double(&self, key: keys::DoubleKey) -> f64 {
        self.double(key.0)
    }

    #[must_use]
    pub fn get_string(&self, key: keys::StringKey) -> String {
        self.string(key.0)
    }

    pub fn put_bool(&self, key: keys::BoolKey, value: bool) -> bool {
        self.set_boolean(key.0, value)
    }

    pub fn put_int(&self, key: keys::IntKey, value: i32) -> bool {
        self.set_int(key.0, value)
    }

    pub fn put_double(&self, key: keys::DoubleKey, value: f64) -> bool {
        self.set_double(key.0, value)
    }

    pub fn put_string(&self, key: keys::StringKey, value: &str) -> bool {
        self.set_string(key.0, value)
    }

    /// Whether any live settings owner has edits not confirmed as durable.
    /// In a single-process host this covers all mounted players.
    pub fn any_unsaved() -> bool {
        let refs = OBSERVED
            .get()
            .map(|items| {
                items
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .unwrap_or_default();
        refs.iter().filter_map(Weak::upgrade).any(|owner| {
            let state = owner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.dirty
        })
    }

    pub fn save_status(&self) -> SaveStatus {
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        SaveStatus {
            phase: inner.phase,
            revision: inner.revision,
            persisted_revision: inner.persisted_revision,
            error: inner.save_error.clone(),
        }
    }

    /// Non-blocking, coalesced admission. User-visible status remains Pending /
    /// Saving until the actual disk transaction completes. A dropped dialog does
    /// not cancel an admitted write. No mutex guarding GUI state spans disk I/O.
    pub fn request_flush(&self) -> io::Result<()> {
        {
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inner.save_queued || !inner.dirty {
                return Ok(());
            }
            inner.save_queued = true;
            inner.phase = SavePhase::Pending;
        }
        let settings = self.clone();
        let result = big_os_kit::background::submit(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| settings.try_flush()))
                    .unwrap_or_else(|_| {
                        Err(io::Error::other("settings persistence task panicked"))
                    });
            let retry_latest = {
                let mut inner = settings
                    .inner
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                inner.save_queued = false;
                if let Err(error) = &result {
                    inner.phase = SavePhase::Failed;
                    inner.save_error = Some(error.to_string());
                }
                // Do not spin on a failing disk. A retry is explicit or follows
                // a later edit. Only a newer revision after success is coalesced.
                result.is_ok() && inner.dirty
            };
            if retry_latest {
                let _ = settings.request_flush();
            }
        });
        if let Err(error) = &result {
            {
                let mut inner = self
                    .inner
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                inner.save_queued = false;
                inner.phase = SavePhase::Failed;
                inner.save_error = Some(error.to_string());
            }
            big_os_kit::background::changed();
        }
        result
    }

    /// Worker-only save transaction (atomic, not fsynced: the kernel writes it
    /// back in its own time). Snapshot/commit/finalize separates the disk lock
    /// from the fast memory lock. Another process (a second window) may have
    /// saved meanwhile: the file as it is now keeps every key this process did
    /// not change, and this process takes those values in memory. A file that
    /// no longer parses is left as it is.
    pub fn try_flush(&self) -> io::Result<()> {
        let disk_lock = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .disk_lock
            .clone();
        let _transaction = disk_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (path, values, changed, revision) = {
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inner.path.as_os_str().is_empty() || inner.load_error.is_some() {
                inner.phase = SavePhase::Failed;
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "settings are not backed by a safely readable private file",
                ));
            }
            if !inner.dirty {
                return Ok(());
            }
            inner.phase = SavePhase::Saving;
            (
                inner.path.clone(),
                inner.settings_values.clone(),
                inner.changed.clone(),
                inner.revision,
            )
        };
        let result = (|| -> io::Result<HashMap<String, String>> {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let _file_lock = storage::WriteLock::try_acquire(&path)?;
            let mut merged = keys::defaults();
            if let Some(bytes) = storage::read_regular_limited(&path, LIMIT)? {
                merged.extend(parse(&bytes)?);
            }
            for key in &changed {
                if let Some(value) = values.get(key) {
                    merged.insert(key.clone(), value.clone());
                }
            }
            storage::atomic_write_private(&path, &serialize(&merged)?)?;
            Ok(merged)
        })();
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match result {
            Ok(written) => {
                let Inner {
                    changed,
                    settings_values,
                    ..
                } = &mut *inner;
                // A key set again while the file was written stays changed.
                changed.retain(|key| settings_values.get(key) != written.get(key));
                for (key, value) in written {
                    if !changed.contains(&key) {
                        settings_values.insert(key, value);
                    }
                }
                inner.persisted_revision = revision;
                inner.dirty = inner.revision != revision;
                inner.phase = if inner.dirty {
                    SavePhase::Pending
                } else {
                    SavePhase::Saved
                };
                inner.save_error = None;
                Ok(())
            }
            Err(error) => {
                inner.phase = SavePhase::Failed;
                inner.save_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    /// Compatibility entry point: queues work; never claims synchronous success.
    pub fn flush(&self) {
        if let Err(error) = self.request_flush() {
            error!("settings save not queued: {error}");
        }
    }

    /// Keys [`Self::reset_to_defaults`] leaves as they are: what the person
    /// collected, where they left off, how they laid out the library, and the
    /// live levels of the player's own controls, none of them a preference.
    pub const KEPT_ON_RESTORE: [&'static str; 21] = [
        keys::VIDEO_LIBRARY_FOLDERS.0,
        keys::CUSTOM_KEYBINDINGS.0,
        keys::VOLUME.0,
        keys::AUDIO_VOLUME.0,
        keys::PLAYLIST_SHUFFLE.0,
        keys::PLAYLIST_LAST_FOLDER.0,
        keys::PLAYLIST_REORDER_HINT_SEEN.0,
        keys::FILES_LAST_DIR.0,
        keys::NOISE_STRENGTH.0,
        keys::NOISE_MODEL.0,
        keys::NOISE_MONO.0,
        keys::NOISE_VOICE_ONLY.0,
        keys::NOISE_VOICE_SENSITIVITY.0,
        keys::EQ_BANDS.0,
        keys::EQ_PRESET.0,
        keys::APPEARANCE_WINDOW_WIDTH.0,
        keys::APPEARANCE_WINDOW_HEIGHT.0,
        keys::AUDIO_APPEARANCE_WINDOW_WIDTH.0,
        keys::AUDIO_APPEARANCE_WINDOW_HEIGHT.0,
        keys::LIBRARY_GROUPING.0,
        keys::PLAYLIST_COLUMN_WIDTHS.0,
    ];

    /// Put every preference back to its default, keeping [`Self::KEPT_ON_RESTORE`].
    pub fn reset_to_defaults(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, value) in keys::defaults() {
            if !Self::KEPT_ON_RESTORE.contains(&key.as_str()) {
                inner.changed.insert(key.clone());
                inner.settings_values.insert(key, value);
            }
        }
        inner.dirty = true;
        inner.revision = inner.revision.wrapping_add(1);
        inner.phase = SavePhase::Pending;
        drop(inner);
        self.flush();
    }
}
