use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use log::error;

use super::{Inner, defaults, keys};
use crate::error::Result;

const LIMIT: usize = 1024 * 1024;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, Weak};
use std::io;
use big_os_kit::storage::{self, CommitOutcome};

type Shared = Arc<RwLock<Inner>>;
static STORES: OnceLock<Mutex<HashMap<PathBuf, Weak<RwLock<Inner>>>>> = OnceLock::new();

fn parse(content: &[u8]) -> io::Result<HashMap<String, String>> {
    let text = std::str::from_utf8(content).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "settings are not UTF-8"))?;
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let (key, value) = line.split_once('=').ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid settings line"))?;
        let key = key.trim();
        if !super::schema::valid(key, value) || values.insert(key.into(), value.into()).is_some() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or duplicate setting"));
        }
    }
    Ok(values)
}

fn serialize(values: &HashMap<String, String>) -> io::Result<Vec<u8>> {
    let mut entries: Vec<_> = values.iter().collect(); entries.sort_by_key(|(key, _)| *key);
    let mut bytes = Vec::new();
    for (key, value) in entries {
        if !super::schema::valid(key, value) || key.len() + value.len() + 2 > LIMIT.saturating_sub(bytes.len()) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or excessive settings"));
        }
        bytes.extend_from_slice(key.as_bytes()); bytes.push(b'=');
        bytes.extend_from_slice(value.as_bytes()); bytes.push(b'\n');
    }
    Ok(bytes)
}

fn commit(inner: &mut Inner, values: HashMap<String, String>) -> io::Result<CommitOutcome> {
    if inner.path.as_os_str().is_empty() { return Err(io::Error::new(io::ErrorKind::NotFound, "settings are in memory only; no configuration directory")); }
    if let Some(error) = &inner.load_error { return Err(io::Error::new(io::ErrorKind::InvalidData, format!("settings were not loaded safely; not replacing the file: {error}"))); }
    let bytes = serialize(&values)?;
    if let Some(parent) = inner.path.parent() { fs::create_dir_all(parent)?; }
    let _lock = storage::WriteLock::try_acquire(&inner.path)?;
    let current = storage::read_regular_limited(&inner.path, LIMIT)?;
    if current != inner.disk_bytes {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "settings changed in another process; reopen before saving"));
    }
    let receipt = storage::atomic_write_private_receipt(&inner.path, &bytes)?;
    // Both Ok variants have published: update the expected disk revision now.
    // Another local mutation cannot race this sync operation: Inner is write-locked.
    inner.settings_values = values;
    inner.disk_bytes = Some(bytes);
    inner.dirty = false;
    inner.durability_pending = matches!(&receipt, CommitOutcome::PublishedNotSynced(_));
    Ok(receipt)
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
        Self { inner: Arc::new(RwLock::new(Inner {
            settings_values: defaults::defaults(), path: PathBuf::new(), dirty: false,
            disk_bytes: None, load_error: None, durability_pending: false,
        })) }
    }

    pub(super) fn from_path(mut path: PathBuf) -> Self {
        // Normalize existing parents. Nonexistent parents retain an absolute
        // lexical path; this registry is not a universal hard-link alias map.
        if let (Some(parent), Some(name)) = (path.parent(), path.file_name()) {
            if let Ok(parent) = fs::canonicalize(parent) { path = parent.join(name); }
        }
        let stores = STORES.get_or_init(|| Mutex::new(HashMap::new()));
        let mut stores = stores.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        stores.retain(|_, value| value.strong_count() > 0);
        if let Some(inner) = stores.get(&path).and_then(Weak::upgrade) { return Self { inner }; }
        let loaded = storage::read_regular_limited(&path, LIMIT);
        let mut values = defaults::defaults();
        let (disk_bytes, load_error) = match loaded {
            Ok(Some(bytes)) => match parse(&bytes) {
                Ok(saved) => { values.extend(saved); (Some(bytes), None) }
                Err(error) => (Some(bytes), Some(error.to_string())),
            },
            Ok(None) => (None, None),
            Err(error) => (None, Some(error.to_string())),
        };
        let inner: Shared = Arc::new(RwLock::new(Inner {
            settings_values: values, path: path.clone(), dirty: false, disk_bytes,
            load_error, durability_pending: false,
        }));
        stores.insert(path, Arc::downgrade(&inner));
        Self { inner }
    }

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
            .and_then(|value| value.parse::<f64>().ok()).filter(|value| value.is_finite())
            .unwrap_or(0.0)
    }

    pub fn set_string(&self, key: &str, value: &str) -> bool {
        if !super::schema::valid(key, value) { return false; }
        self.debug_check_key(key);
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.settings_values.get(key).is_some_and(|old| old == value) { return true; }
        inner.settings_values.insert(key.into(), value.into()); inner.dirty = true;
        true
    }
    pub fn set_boolean(&self, key: &str, value: bool) -> bool { self.set_string(key, if value { "true" } else { "false" }) }
    pub fn set_int(&self, key: &str, value: i32) -> bool { self.set_string(key, &value.to_string()) }
    pub fn set_double(&self, key: &str, value: f64) -> bool { value.is_finite() && self.set_string(key, &value.to_string()) }

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

    /// Blocking durable save. Run on a worker when called during interaction.
    /// A publication receipt distinguishes a completed write from fsync failure.
    pub fn try_flush(&self) -> io::Result<CommitOutcome> {
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if !inner.dirty {
            if inner.durability_pending {
                return Ok(match storage::sync_parent(&inner.path) {
                    Ok(()) => { inner.durability_pending = false; CommitOutcome::Durable }
                    Err(error) => CommitOutcome::PublishedNotSynced(error),
                });
            }
            return Ok(CommitOutcome::Durable);
        }
        let values = inner.settings_values.clone();
        commit(&mut inner, values)
    }

    pub fn flush(&self) {
        match self.try_flush() {
            Ok(CommitOutcome::Durable) => {},
            Ok(CommitOutcome::PublishedNotSynced(error)) => error!("Settings applied; durability unconfirmed: {error}"),
            Err(error) => error!("Settings not saved; changes remain pending: {error}"),
        }
    }

    pub fn migrate_audio_appearance(&self) {
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.settings_values.get("audio-appearance-migrated").is_some_and(|s| s == "true") { return; }
        let defaults = defaults::defaults();
        for (audio, video) in [
            ("audio-ui-color-source", "ui-color-source"), ("audio-ui-bg-color", "ui-bg-color"),
            ("audio-ui-font-color", "ui-font-color"), ("audio-ui-icon-color", "ui-icon-color"),
            ("audio-ui-dialog-color", "ui-dialog-color"),
        ] {
            // Never overwrite an already personalized audio value.
            if inner.settings_values.get(audio) == defaults.get(audio) {
                if let Some(value) = inner.settings_values.get(video).cloned() {
                    inner.settings_values.insert(audio.into(), value);
                }
            }
        }
        inner.settings_values.insert("audio-appearance-migrated".into(), "true".into());
        inner.dirty = true;
        drop(inner); self.flush();
    }

    pub fn reset_to_defaults(&self) {
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        for (key, value) in defaults::defaults() {
            if matches!(key.as_str(), "window-width" | "window-height" | "window-maximized") { continue; }
            inner.settings_values.insert(key, value);
        }
        inner.dirty = true;
        drop(inner); self.flush();
    }

    pub fn export_to(&self, path: &str) -> Result<()> {
        let bytes = {
            let inner = self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            serialize(&inner.settings_values)?
        };
        let target = std::path::Path::new(path);
        let staged = storage::StagedFile::new(target)?;
        fs::write(staged.path(), &bytes)?;
        staged.publish(target)?.into_io_result()?;
        Ok(())
    }

    pub fn import_from(&self, path: &str) -> Result<()> {
        let bytes = storage::read_regular_limited(std::path::Path::new(path), LIMIT)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "import file missing"))?;
        let imported = parse(&bytes)?; // validate ALL entries before mutation
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut values = inner.settings_values.clone(); values.extend(imported);
        commit(&mut inner, values)?.into_io_result()?;
        Ok(())
    }
}
