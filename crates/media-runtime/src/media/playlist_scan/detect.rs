use std::path::Path;

/// `.ts` is ambiguous (TypeScript vs MPEG-TS) — sniff the header to disambiguate.
#[must_use]
pub fn is_video_file(path: &str) -> bool {
    let p = Path::new(path);
    if p.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ts"))
        && is_code_project_dir(p)
    {
        return false;
    }
    big_media_components::media::files::is_video_file(path)
}

#[must_use]
pub fn is_audio_file(path: &str) -> bool {
    big_media_components::media::files::is_audio_file(path)
}

#[must_use]
pub fn is_media_file(path: &str) -> bool {
    is_video_file(path) || is_audio_file(path)
}

/// Walks parents looking for `package.json`, `tsconfig.json`, or `node_modules/`.
///
/// Stops at — without inspecting — the user's home directory and the filesystem
/// roots. A marker there (e.g. a global `node_modules` in `$HOME`) is not a
/// project boundary and must not reclassify every `.ts` media file living
/// somewhere under it as TypeScript.
pub(crate) fn is_code_project_dir(path: &Path) -> bool {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == Path::new("/") || d == Path::new("/home") || home.as_deref() == Some(d) {
            break;
        }
        if d.join("package.json").exists()
            || d.join("tsconfig.json").exists()
            || d.join("node_modules").is_dir()
        {
            return true;
        }
        dir = d.parent();
    }
    false
}
