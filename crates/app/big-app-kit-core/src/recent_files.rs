// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Recent-file JSONL store shared by desktop apps.

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// Single entry inside a recent collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntry {
    /// File path (UTF-8 with lossy conversion).
    pub path: String,
    /// Last-access time as a Unix timestamp.
    pub timestamp: u64,
}

/// Recent store value used across BigLinux apps.
#[derive(Debug, Clone)]
pub struct RecentStore {
    state_dir: PathBuf,
    max_entries: usize,
}

impl RecentStore {
    /// Construct a [`RecentStore`] populated from the caller-supplied fields.
    ///
    /// All setters/builder methods can still adjust the result before it is
    /// passed to the GTK layer.
    #[must_use]
    pub fn new(state_dir: impl Into<PathBuf>, max_entries: usize) -> Self {
        Self {
            state_dir: state_dir.into(),
            max_entries: max_entries.clamp(1, 10_000),
        }
    }

    /// Return the current `path for kind` value held by this [`RecentStore`].
    #[must_use]
    pub fn path_for_kind(&self, kind: &str) -> PathBuf {
        let filename = if kind.is_empty() {
            "recent.jsonl".to_string()
        } else {
            format!("recent-{kind}.jsonl")
        };
        self.state_dir.join(filename)
    }

    /// Worker-oriented bounded read. Use try_load when distinguishing corruption
    /// from absence matters; mutation paths always propagate read errors.
    #[must_use]
    pub fn load(&self, kind: &str) -> Vec<RecentEntry> {
        self.try_load(kind).unwrap_or_default()
    }
    /// Bounded regular-file read with explicit errors; never erase malformed history.
    pub fn try_load(&self, kind: &str) -> Result<Vec<RecentEntry>, String> {
        self.validate(kind)?;
        let Some(bytes) =
            big_os_kit::storage::read_regular_limited(&self.path_for_kind(kind), 1024 * 1024)
                .map_err(|_| "Could not read recent history")?
        else {
            return Ok(Vec::new());
        };
        let text = std::str::from_utf8(&bytes).map_err(|_| "Recent history is not UTF-8")?;
        let mut entries = Vec::new();
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            entries.push(parse_recent_line(line).ok_or("Invalid recent history")?);
            if entries.len() > 10_000 {
                return Err("Recent history is too large".into());
            }
        }
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.timestamp));
        entries.truncate(self.max_entries);
        Ok(entries)
    }
    fn validate(&self, kind: &str) -> Result<(), String> {
        if !self.state_dir.is_absolute()
            || kind.len() > 64
            || !kind
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err("Invalid recent-history location".into());
        }
        Ok(())
    }

    /// Move `file_path` to the top of the `kind` history, dropping the oldest
    /// entries past the limit. Only existing local files are recorded: streams,
    /// disc URIs and vanished paths do not canonicalize and are skipped.
    pub fn add(&self, file_path: &str, kind: &str, timestamp: u64) -> Result<(), String> {
        self.validate(kind)?;
        let Ok(canonical) = fs::canonicalize(file_path) else {
            return Ok(());
        };
        let canonical = canonical
            .to_str()
            .ok_or("Recent history requires a UTF-8 path")?
            .to_owned();
        fs::create_dir_all(&self.state_dir).map_err(|_| "Could not create recent directory")?;
        let path = self.path_for_kind(kind);
        let _lock = big_os_kit::storage::WriteLock::try_acquire(&path)
            .map_err(|_| "Recent history busy")?;
        let mut entries = self.try_load(kind)?;
        entries.retain(|entry| entry.path != canonical);
        entries.insert(
            0,
            RecentEntry {
                path: canonical,
                timestamp,
            },
        );
        entries.truncate(self.max_entries);
        let content = entries
            .iter()
            .map(|entry| json!({"path": entry.path, "ts": entry.timestamp}).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        if content.len() > 1024 * 1024 {
            return Err("Recent history exceeds limit".into());
        }
        big_os_kit::storage::atomic_write_private(&path, content.as_bytes())
            .map_err(|_| "Recent history save failed".into())
    }

    /// Return the current `clear` value held by this [`RecentStore`].
    pub fn clear(&self, kind: &str) -> Result<(), String> {
        self.validate(kind)?;
        fs::create_dir_all(&self.state_dir).map_err(|_| "Could not create recent directory")?;
        let path = self.path_for_kind(kind);
        let _lock = big_os_kit::storage::WriteLock::try_acquire(&path)
            .map_err(|_| "Recent history busy")?;
        big_os_kit::storage::atomic_write_private(&path, b"")
            .map_err(|_| "Recent history clear failed".into())
    }
}

/// Return the basename of `path` for use as a recent-file row label,
/// falling back to the raw input when there is no basename.
#[must_use]
pub fn display_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().to_string(),
    )
}

fn parse_recent_line(line: &str) -> Option<RecentEntry> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(RecentEntry {
        path: value.get("path")?.as_str()?.to_string(),
        timestamp: value.get("ts")?.as_u64()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_state_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("big-app-kit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn display_name_uses_basename_or_raw_path() {
        assert_eq!(display_name("/home/user/video.mp4"), "video.mp4");
        assert_eq!(display_name("/"), "/");
        assert_eq!(display_name(""), "");
    }

    #[test]
    fn add_keeps_newest_first_without_duplicates_and_skips_non_files() {
        let state_directory = temp_state_dir("recent-add");
        let store = RecentStore::new(&state_directory, 2);
        let a = state_directory.join("a.mkv");
        let b = state_directory.join("b.mkv");
        let c = state_directory.join("c.mkv");
        for file in [&a, &b, &c] {
            fs::write(file, b"").unwrap();
        }
        let name = |p: &PathBuf| p.to_str().unwrap().to_owned();

        store.add(&name(&a), "video", 1).unwrap();
        store.add(&name(&b), "video", 2).unwrap();
        store.add(&name(&a), "video", 3).unwrap();
        store.add("https://example.com/stream", "video", 4).unwrap();
        store
            .add(&name(&state_directory.join("gone.mkv")), "video", 5)
            .unwrap();
        let paths: Vec<_> = store.load("video").into_iter().map(|e| e.path).collect();
        assert_eq!(paths, [name(&a), name(&b)]);

        store.add(&name(&c), "video", 6).unwrap();
        let paths: Vec<_> = store.load("video").into_iter().map(|e| e.path).collect();
        assert_eq!(paths, [name(&c), name(&a)]);
        assert!(store.load("audio").is_empty());

        let _ = fs::remove_dir_all(state_directory);
    }

    #[test]
    fn clear_missing_recent_file_is_idempotent() {
        let state_directory = temp_state_dir("recent-clear-missing");
        let store = RecentStore::new(&state_directory, 5);

        store.clear("audio").unwrap();

        let _ = fs::remove_dir_all(state_directory);
    }

    #[test]
    fn clear_reports_real_remove_errors() {
        let state_directory = temp_state_dir("recent-clear-error");
        let store = RecentStore::new(&state_directory, 5);
        let recent_file_path = store.path_for_kind("audio");
        fs::create_dir_all(&recent_file_path).unwrap();

        assert!(store.clear("audio").is_err());

        let _ = fs::remove_dir_all(state_directory);
    }
}
