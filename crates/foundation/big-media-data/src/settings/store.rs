use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use log::error;

use super::{Inner, defaults, keys};
use crate::error::Result;

const LIMIT: usize = 1024 * 1024;
use big_os_kit::storage::{self, CommitOutcome};
use std::collections::HashMap;
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

fn parse(content: &[u8]) -> io::Result<HashMap<String, String>> {
    let text = std::str::from_utf8(content)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "settings are not UTF-8"))?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid settings line"))?;
        let key = key.trim();
        if !super::schema::valid(key, value) || values.insert(key.into(), value.into()).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid or duplicate setting",
            ));
        }
    }
    Ok(values)
}

fn serialize(values: &HashMap<String, String>) -> io::Result<Vec<u8>> {
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
    DurabilityPending,
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

impl Default for Settings {
    fn default() -> Self {
        Self::new(crate::config::runtime::CONFIG_DIR)
    }
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
            settings_values: defaults::defaults(),
            path: PathBuf::new(),
            dirty: false,
            disk_bytes: None,
            load_error: None,
            durability_pending: false,
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
        let mut values = defaults::defaults();
        let (disk_bytes, load_error) = match loaded {
            Ok(Some(bytes)) => match parse(&bytes) {
                Ok(saved) => {
                    values.extend(saved);
                    (Some(bytes), None)
                }
                Err(error) => (Some(bytes), Some(error.to_string())),
            },
            Ok(None) => (None, None),
            Err(error) => (None, Some(error.to_string())),
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
            disk_bytes,
            load_error,
            durability_pending: false,
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
        defaults::defaults()
    }

    #[inline]
    #[allow(unused_variables, clippy::unused_self)]
    fn debug_check_key(&self, key: &str) {
        #[cfg(debug_assertions)]
        {
            if key.starts_with("dlg-") {
                return;
            }
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !inner.settings_values.contains_key(key) {
                log::warn!("Settings: unknown key {key:?} — consider adding to defaults()");
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
    pub fn boolean_or(&self, key: &str, default: bool) -> bool {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.settings_values.get(key) {
            Some(value) => value == "true",
            None => default,
        }
    }

    #[must_use]
    pub fn int(&self, key: &str) -> i32 {
        self.int_or(key, 0)
    }

    /// Unlike [`Self::int`], keeps zero distinguishable from "never stored",
    /// which matters for volume-like settings where 0 is a real user choice.
    #[must_use]
    pub fn int_or(&self, key: &str, default: i32) -> i32 {
        self.debug_check_key(key);
        let inner = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match inner.settings_values.get(key) {
            Some(value) => value.parse().unwrap_or(default),
            None => default,
        }
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

    #[must_use]
    pub fn get_bool_or(&self, key: keys::BoolKey, default: bool) -> bool {
        self.boolean_or(key.0, default)
    }

    #[must_use]
    pub fn get_int(&self, key: keys::IntKey) -> i32 {
        self.int(key.0)
    }

    #[must_use]
    pub fn get_int_or(&self, key: keys::IntKey, default: i32) -> i32 {
        self.int_or(key.0, default)
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
    /// In a single-process host this intentionally covers all mounted players.
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
            state.dirty || state.durability_pending
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
            if inner.save_queued || (!inner.dirty && !inner.durability_pending) {
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
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            inner.save_queued = false;
            inner.phase = SavePhase::Failed;
            inner.save_error = Some(error.to_string());
        }
        result
    }

    /// Worker-only durable transaction. Snapshot/commit/finalize separates the
    /// disk lock from the fast memory lock. The generation check preserves edits
    /// made while an older generation was on disk.
    pub fn try_flush(&self) -> io::Result<CommitOutcome> {
        let disk_lock = self
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .disk_lock
            .clone();
        let _transaction = disk_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (path, expected, values, revision, dirty, durability, load_error) = {
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
            if !inner.dirty && !inner.durability_pending {
                return Ok(CommitOutcome::Durable);
            }
            inner.phase = SavePhase::Saving;
            (
                inner.path.clone(),
                inner.disk_bytes.clone(),
                inner.settings_values.clone(),
                inner.revision,
                inner.dirty,
                inner.durability_pending,
                inner.load_error.clone(),
            )
        };
        let mut published_bytes = None;
        let result = (|| {
            if path.as_os_str().is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "settings are in memory only",
                ));
            }
            if load_error.is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "settings could not be read safely; not replacing them",
                ));
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let _file_lock = storage::WriteLock::try_acquire(&path)?;
            if storage::read_regular_limited(&path, LIMIT)? != expected {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "settings changed in another process; reopen before saving",
                ));
            }
            if !dirty && durability {
                return Ok(match storage::sync_parent(&path) {
                    Ok(()) => CommitOutcome::Durable,
                    Err(error) => CommitOutcome::PublishedNotSynced(error),
                });
            }
            let bytes = serialize(&values)?;
            let receipt = storage::atomic_write_private_receipt(&path, &bytes)?;
            published_bytes = Some(bytes);
            Ok(receipt)
        })();
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &result {
            Ok(receipt) => {
                if let Some(bytes) = published_bytes {
                    inner.disk_bytes = Some(bytes);
                    inner.persisted_revision = revision;
                    inner.dirty = inner.revision != revision;
                }
                inner.durability_pending = matches!(receipt, CommitOutcome::PublishedNotSynced(_));
                inner.phase = if inner.dirty {
                    SavePhase::Pending
                } else if inner.durability_pending {
                    SavePhase::DurabilityPending
                } else {
                    SavePhase::Saved
                };
                inner.save_error = match receipt {
                    CommitOutcome::PublishedNotSynced(_) => {
                        Some("Published, but durable storage is not confirmed".into())
                    }
                    CommitOutcome::Durable => None,
                };
            }
            Err(error) => {
                inner.phase = SavePhase::Failed;
                inner.save_error = Some(error.to_string());
            }
        }
        result
    }

    /// Compatibility entry point: queues work; never claims synchronous success.
    pub fn flush(&self) {
        if let Err(error) = self.request_flush() {
            error!("settings save not queued: {error}");
        }
    }

    pub fn migrate_audio_appearance(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner
            .settings_values
            .get("audio-appearance-migrated")
            .is_some_and(|s| s == "true")
        {
            return;
        }
        let defaults = defaults::defaults();
        for (audio, video) in [
            ("audio-ui-color-source", "ui-color-source"),
            ("audio-ui-bg-color", "ui-bg-color"),
            ("audio-ui-font-color", "ui-font-color"),
            ("audio-ui-icon-color", "ui-icon-color"),
            ("audio-ui-dialog-color", "ui-dialog-color"),
        ] {
            // Never overwrite an already personalized audio value.
            if inner.settings_values.get(audio) == defaults.get(audio)
                && let Some(value) = inner.settings_values.get(video).cloned()
            {
                inner.settings_values.insert(audio.into(), value);
            }
        }
        inner
            .settings_values
            .insert("audio-appearance-migrated".into(), "true".into());
        inner.dirty = true;
        inner.revision = inner.revision.wrapping_add(1);
        inner.phase = SavePhase::Pending;
        drop(inner);
        self.flush();
    }

    pub fn reset_to_defaults(&self) {
        let mut inner = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, value) in defaults::defaults() {
            if matches!(
                key.as_str(),
                "window-width" | "window-height" | "window-maximized"
            ) {
                continue;
            }
            inner.settings_values.insert(key, value);
        }
        inner.dirty = true;
        inner.revision = inner.revision.wrapping_add(1);
        inner.phase = SavePhase::Pending;
        drop(inner);
        self.flush();
    }

    pub fn export_to(&self, path: &str) -> Result<()> {
        let bytes = {
            let inner = self
                .inner
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            serialize(&inner.settings_values)?
        };
        let target = std::path::Path::new(path);
        let staged = storage::StagedFile::new(target)?;
        fs::write(staged.path(), &bytes)?;
        staged.publish(target)?.into_io_result()?;
        Ok(())
    }

    /// Worker-only import. Refuse to replace edits made while reading the file.
    pub fn import_from(&self, path: &str) -> Result<()> {
        let revision = self.save_status().revision;
        let bytes = storage::read_regular_limited(std::path::Path::new(path), LIMIT)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "import file missing"))?;
        let imported = parse(&bytes)?;
        {
            let mut inner = self
                .inner
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inner.revision != revision {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "settings changed while import was being read",
                )
                .into());
            }
            inner.settings_values.extend(imported);
            inner.revision = inner.revision.wrapping_add(1);
            inner.dirty = true;
            inner.phase = SavePhase::Pending;
        }
        self.try_flush()?.into_io_result()?;
        Ok(())
    }
}
