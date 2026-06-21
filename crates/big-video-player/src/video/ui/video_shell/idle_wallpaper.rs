//! Static idle background sourced from the desktop wallpaper.
//!
//! Replaces the old CPU-hungry animated gradient (a 33 ms CSS tick that redrew
//! five layered gradients every frame — costly under software rendering). The
//! desktop's active wallpaper is detected, blurred + darkened with ffmpeg, and
//! cached under `~/.cache/big-video-player/`; the cached image is then shown as
//! a STATIC background behind the welcome screen when no video plays. The base
//! `.idle-bg` gradient in `style.css` is the fallback when no wallpaper resolves
//! (no desktop integration, or ffmpeg missing).
//!
//! Detection + blur run off the GTK main loop (a worker thread); the result is
//! delivered back over an async channel and applied on the main thread.

use std::ffi::OsStr;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use gtk::glib;

use super::osd::IdleBackground;

/// Image extensions accepted as wallpapers (lower-case match).
const IMAGE_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "tiff", "avif", "svg"];

/// Kick off desktop-wallpaper detection + blur on a worker thread, then apply
/// the cached blurred image as the idle background once it is ready. No-op on
/// any failure — the static gradient fallback stays in place.
pub(super) fn spawn_apply(idle_bg: Rc<IdleBackground>) {
    let (sender, receiver) = async_channel::bounded::<PathBuf>(1);
    // Worker: detection + ffmpeg blur are blocking (disk read + a subprocess),
    // so they must not run on the GTK main loop. Only `sender` (Send) crosses
    // the thread boundary; the `Rc<IdleBackground>` stays on the main thread.
    let spawned = std::thread::Builder::new()
        .name("idle-wallpaper".into())
        .spawn(move || {
            if let Some(path) = resolve_blurred_wallpaper() {
                let _ = sender.send_blocking(path);
            }
        });
    if spawned.is_err() {
        return;
    }
    glib::spawn_future_local(async move {
        if let Ok(path) = receiver.recv().await {
            if let Some(display) = gtk::gdk::Display::default() {
                idle_bg.set_blurred_wallpaper(&path, &display);
            }
        }
    });
}

/// Detect the active wallpaper and return the path to its blurred, cached copy.
fn resolve_blurred_wallpaper() -> Option<PathBuf> {
    let source = detect_wallpaper()?;
    ensure_blurred(&source)
}

/// Best-effort active-wallpaper detection across the desktops BigLinux ships /
/// runs on: KDE Plasma first (the default), then GNOME. `None` on anything else.
fn detect_wallpaper() -> Option<PathBuf> {
    detect_plasma_wallpaper().or_else(detect_gnome_wallpaper)
}

/// Parse the active Plasma wallpaper from `plasma-org.kde.plasma.desktop-appletsrc`
/// (KConfig INI): the first `Image=` under a `[…][Wallpaper][org.kde.image][General]`
/// group that resolves to an existing image file.
fn detect_plasma_wallpaper() -> Option<PathBuf> {
    let config = config_home().join("plasma-org.kde.plasma.desktop-appletsrc");
    let text = std::fs::read_to_string(&config).ok()?;

    let mut in_wallpaper_image = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            // KConfig group header: `[A][B][C]`. The image wallpaper plugin's
            // settings live under a header containing both these segments.
            in_wallpaper_image =
                line.contains("[Wallpaper][org.kde.image]") && line.ends_with("[General]");
            continue;
        }
        if in_wallpaper_image {
            if let Some(value) = line.strip_prefix("Image=") {
                if let Some(image) = resolve_plasma_image(value.trim()) {
                    return Some(image);
                }
            }
        }
    }
    None
}

/// Resolve a Plasma `Image=` value to a concrete file. The value may be a direct
/// `file://` URI / path, or a KPackage wallpaper directory whose actual images
/// live under `contents/images/`.
fn resolve_plasma_image(value: &str) -> Option<PathBuf> {
    let raw = value.strip_prefix("file://").unwrap_or(value);
    let path = PathBuf::from(percent_decode(raw));
    if path.is_file() {
        return Some(path);
    }
    if path.is_dir() {
        for sub in ["contents/images_dark", "contents/images"] {
            if let Some(image) = largest_image_in(&path.join(sub)) {
                return Some(image);
            }
        }
    }
    None
}

/// Query the GNOME wallpaper via `gsettings`. `None` if gsettings is missing or
/// the value does not resolve to an existing file.
fn detect_gnome_wallpaper() -> Option<PathBuf> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.background", "picture-uri"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout);
    let value = value.trim().trim_matches(['\'', '"']);
    let raw = value.strip_prefix("file://").unwrap_or(value);
    let path = PathBuf::from(percent_decode(raw));
    path.is_file().then_some(path)
}

/// Largest image file (by byte size) directly under `dir`, if any. Picks the
/// highest-resolution variant of a KPackage wallpaper.
fn largest_image_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_image_path(path))
        .max_by_key(|path| std::fs::metadata(path).map_or(0, |m| m.len()))
}

/// True when `path`'s extension is a known image type (case-insensitive).
fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase)
        .is_some_and(|ext| IMAGE_EXTS.contains(&ext.as_str()))
}

/// Return the cached blurred copy of `source`, blurring it with ffmpeg on a
/// cache miss. The cache key folds in the source path and mtime so a changed
/// wallpaper (re)blurs. `None` if ffmpeg is unavailable or fails.
fn ensure_blurred(source: &Path) -> Option<PathBuf> {
    let cache_dir = cache_home().join("big-video-player");
    std::fs::create_dir_all(&cache_dir).ok()?;

    let mtime = std::fs::metadata(source)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    mtime.hash(&mut hasher);
    let cache = cache_dir.join(format!("wallpaper-blur-{:016x}.png", hasher.finish()));

    if std::fs::metadata(&cache).is_ok_and(|m| m.len() > 0) {
        return Some(cache);
    }
    blur_to(source, &cache)
}

/// Blur + darken `source` into `dest`. Tries ffmpeg directly; if that fails on
/// an SVG (ffmpeg builds without an SVG decoder), rasterizes it with
/// `rsvg-convert` first — BigLinux ships SVG wallpapers — then blurs the raster.
fn blur_to(source: &Path, dest: &Path) -> Option<PathBuf> {
    if run_blur(source, dest) {
        return Some(dest.to_path_buf());
    }
    if is_svg(source) {
        let raster = dest.with_extension("src.png");
        let ok = rasterize_svg(source, &raster) && run_blur(&raster, dest);
        let _ = std::fs::remove_file(&raster);
        if ok {
            return Some(dest.to_path_buf());
        }
    }
    None
}

/// Run the ffmpeg blur on `source` → `dest`; `true` on a non-empty result. Argv
/// array, no shell.
fn run_blur(source: &Path, dest: &Path) -> bool {
    Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(source)
        .args([
            "-vf",
            // Downscale (even height), strong gaussian blur, slight darken +
            // desaturate so the welcome-screen text stays legible over it.
            "scale=1280:-2:flags=bilinear,gblur=sigma=24,eq=brightness=-0.18:saturation=0.9",
            "-frames:v",
            "1",
        ])
        .arg(dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && std::fs::metadata(dest).is_ok_and(|m| m.len() > 0)
}

/// Rasterize an SVG `source` to PNG `dest` via `rsvg-convert`; `true` on success.
fn rasterize_svg(source: &Path, dest: &Path) -> bool {
    Command::new("rsvg-convert")
        .args(["-w", "1920", "-o"])
        .arg(dest)
        .arg(source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && std::fs::metadata(dest).is_ok_and(|m| m.len() > 0)
}

/// True when `path` has an `.svg` extension (case-insensitive).
fn is_svg(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
fn config_home() -> PathBuf {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_CACHE_HOME` or `~/.cache`.
fn cache_home() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache")
}

/// Resolve an XDG base dir from its env var, falling back to `~/<default>`.
fn xdg_dir(env: &str, default: &str) -> PathBuf {
    std::env::var_os(env)
        .filter(|value| !value.is_empty())
        .map_or_else(
            || {
                let home =
                    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
                home.join(default)
            },
            PathBuf::from,
        )
}

/// Minimal percent-decoding for wallpaper URIs (`gsettings` / `file://` values
/// percent-encode spaces and the like). Leaves malformed escapes untouched.
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_handles_spaces_and_passthrough() {
        assert_eq!(
            percent_decode("/home/u/My%20Wallpaper.png"),
            "/home/u/My Wallpaper.png"
        );
        assert_eq!(percent_decode("/plain/path.jpg"), "/plain/path.jpg");
        // Malformed escape is left as-is, not dropped.
        assert_eq!(percent_decode("/a%2/b"), "/a%2/b");
    }

    #[test]
    fn is_image_path_matches_known_extensions_case_insensitive() {
        assert!(is_image_path(Path::new("/x/a.JPG")));
        assert!(is_image_path(Path::new("/x/b.png")));
        assert!(!is_image_path(Path::new("/x/c.txt")));
        assert!(!is_image_path(Path::new("/x/noext")));
    }

    #[test]
    fn xdg_dir_prefers_env_then_home_default() {
        // Empty env value is ignored in favour of the HOME-based default.
        // (We only assert the default branch shape — env mutation in tests is
        // racy, so just verify the fallback joins the default segment.)
        let dir = xdg_dir("BIG_VIDEO_PLAYER_NONEXISTENT_ENV", ".cache");
        assert!(dir.ends_with(".cache"));
    }
}
