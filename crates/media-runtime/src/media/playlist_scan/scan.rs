use std::collections::HashSet;
use std::path::Path;

use big_app_kit::subprocess::BigSubprocessSpec;

use super::super::playlist::{AUDIO_EXTENSIONS, SCAN_LIMIT, ScanResult, VIDEO_EXTENSIONS};
use super::is_media_file;

const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".git",
    ".cache",
    ".npm",
    ".cargo",
    "target",
    "build",
    "dist",
    ".local",
    ".vscode",
    ".idea",
];

pub fn scan_folder_flat(folder: &Path) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let path_str = path.to_string_lossy().to_string();
                if is_media_file(&path_str) {
                    files.push(path_str);
                }
            }
        }
    }
    files.sort_by_key(|item| item.to_lowercase());
    files
}

/// Skips hidden entries (leading `.`).
pub fn list_subfolders(folder: &Path) -> Vec<String> {
    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.starts_with('.') {
                    dirs.push(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }
    dirs.sort_by_key(|item| item.to_lowercase());
    dirs
}

/// Prefers `fd` for speed; falls back to a hand-rolled walker when unavailable.
pub fn scan_folder_recursive(folder: &Path) -> ScanResult {
    let all_extensions: Vec<&str> = VIDEO_EXTENSIONS
        .iter()
        .chain(AUDIO_EXTENSIONS.iter())
        .copied()
        .collect();
    let ext_args: Vec<String> = all_extensions
        .iter()
        .flat_map(|ext| vec!["-e".to_string(), (*ext).to_string()])
        .collect();

    let mut builder = BigSubprocessSpec::builder()
        .program("fd")
        .arg("-t")
        .arg("f")
        .arg("--max-results")
        .arg(format!("{}", SCAN_LIMIT + 1))
        .arg("--exclude")
        .arg("node_modules")
        .arg("--exclude")
        .arg(".git")
        .arg("--exclude")
        .arg("__pycache__")
        .arg("--exclude")
        .arg(".cache");
    for arg in &ext_args {
        builder = builder.arg(arg);
    }
    builder = builder
        .arg("--color=never")
        .arg("--absolute-path")
        // Pattern and folder after `--`: a folder starting with `-` stays a path.
        .arg("--")
        .arg(".")
        .arg(folder.as_os_str());

    if let Ok(output) = builder.build().run()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut files: Vec<String> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .filter(|line| is_media_file(line))
            .map(std::string::ToString::to_string)
            .collect();
        files.sort_by_key(|item| item.to_lowercase());
        let total_found = files.len();
        let truncated = total_found > SCAN_LIMIT;
        if truncated {
            files.truncate(SCAN_LIMIT);
        }
        return ScanResult {
            files,
            truncated,
            total_found,
        };
    }

    let mut files = Vec::new();
    let mut visited = HashSet::new();
    walk_dir(folder, &mut files, &mut visited, 0);
    files.sort_by_key(|item| item.to_lowercase());
    let total_found = files.len();
    let truncated = total_found > SCAN_LIMIT;
    if truncated {
        files.truncate(SCAN_LIMIT);
    }
    ScanResult {
        files,
        truncated,
        total_found,
    }
}

/// `(dev, inode)` set guards against symlink loops.
fn walk_dir(dir: &Path, result: &mut Vec<String>, visited: &mut HashSet<(u64, u64)>, depth: u32) {
    if depth > 20 || result.len() >= SCAN_LIMIT + 100 {
        return;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(dir) {
            let key = (meta.dev(), meta.ino());
            if !visited.insert(key) {
                return;
            }
        }
    }

    let is_code_dir = dir.join("package.json").exists() || dir.join("tsconfig.json").exists();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if result.len() >= SCAN_LIMIT + 100 {
                return;
            }
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !name.starts_with('.') && !SKIP_DIRS.contains(&name.as_str()) {
                    walk_dir(&path, result, visited, depth + 1);
                }
            } else if path.is_file()
                && let Some(ext) = path.extension().and_then(|e| e.to_str())
            {
                let ext_lower = ext.to_ascii_lowercase();
                if !VIDEO_EXTENSIONS.contains(&ext_lower.as_str())
                    && !AUDIO_EXTENSIONS.contains(&ext_lower.as_str())
                {
                    continue;
                }
                if ext_lower == "ts"
                    && (is_code_dir
                        || !big_media_components::media::files::is_mpeg_ts(&path.to_string_lossy()))
                {
                    continue;
                }
                result.push(path.to_string_lossy().to_string());
            }
        }
    }
}
