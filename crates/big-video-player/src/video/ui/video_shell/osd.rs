use super::*;

use std::path::{Path, PathBuf};

pub(super) struct OsdState {
    /// Top OSD revealer (header bar). Revealed/hidden together with `controls`.
    pub(super) header_revealer: gtk::Revealer,
    /// Bottom OSD revealer (transport/controls bar).
    pub(super) controls_revealer: gtk::Revealer,
    /// Overlay the cursor is set on (hidden when controls hide).
    pub(super) main_overlay: gtk::Overlay,
    /// Button-backed popovers whose open state must keep the OSD shown.
    pub(super) header_popovers: Vec<gtk::Popover>,
    /// Controls-bar hover popovers (volume / speed / voice) whose open state
    /// must keep the OSD shown — else the auto-hide timer dismisses the popover
    /// mid-hover (the "intensity slider flashes then disappears" bug).
    pub(super) controls_popovers: Vec<gtk::Popover>,
    /// True while playback is paused — keep the OSD shown (old "bail if paused").
    pub(super) is_paused: Cell<bool>,
    /// True while the welcome screen is up (nothing loaded) — never auto-hide.
    pub(super) is_welcome_visible: Cell<bool>,
    /// True while interactive trim mode is active — pin the OSD so the transport
    /// (scrub / play-pause) stays reachable while marking cuts. Shared (`Rc`)
    /// with the shell model and the trim key controller (one source of truth).
    pub(super) trim_active: Rc<Cell<bool>>,
    /// Pending auto-hide timer; replaced on each activity, removed on drop.
    pub(super) hide_timeout_id: RefCell<Option<glib::SourceId>>,
}

impl OsdState {
    /// Reveal the header + controls and restore the default cursor.
    pub(super) fn show(&self) {
        self.header_revealer.set_visible(true);
        self.controls_revealer.set_visible(true);
        self.header_revealer.set_reveal_child(true);
        self.controls_revealer.set_reveal_child(true);
        self.main_overlay.set_cursor_from_name(Some("default"));
    }

    /// Hide the header + controls and the cursor — unless a guard says otherwise
    /// (welcome up, paused, or a header menu popover open). Mirrors the old
    /// `hide_controls` bail-outs.
    pub(super) fn hide(&self) {
        if self.is_welcome_visible.get() || self.is_paused.get() || self.trim_active.get() {
            return;
        }
        if self.header_popovers.iter().any(gtk::Popover::is_visible) {
            return;
        }
        // Keep the OSD shown while a controls-bar hover popover (volume / speed /
        // voice) is open, so the auto-hide timer does not dismiss it mid-hover.
        if self.controls_popovers.iter().any(gtk::Popover::is_visible) {
            return;
        }
        self.header_revealer.set_reveal_child(false);
        self.controls_revealer.set_reveal_child(false);
        self.main_overlay.set_cursor_from_name(Some("none"));
    }

    /// Reveal the OSD now and (re)arm the auto-hide timer. Never arms the timer
    /// while the welcome screen is up.
    pub(super) fn show_and_schedule_hide(self: &Rc<Self>) {
        self.show();
        if let Some(id) = self.hide_timeout_id.borrow_mut().take() {
            id.remove();
        }
        if self.is_welcome_visible.get() {
            return;
        }
        let state = Rc::downgrade(self);
        let id =
            glib::timeout_add_local_once(Duration::from_millis(OSD_HIDE_DELAY_MS), move || {
                if let Some(state) = state.upgrade() {
                    state.hide();
                    state.hide_timeout_id.borrow_mut().take();
                }
            });
        self.hide_timeout_id.replace(Some(id));
    }
}

impl Drop for OsdState {
    fn drop(&mut self) {
        if let Some(id) = self.hide_timeout_id.borrow_mut().take() {
            id.remove();
        }
    }
}

/// Static idle background behind the welcome screen (no video loaded). The gl
/// surface is hidden (opacity 0) so the `.idle-bg` box shows through. The base
/// `.idle-bg` gradient (style.css) is the immediate look; once the desktop
/// wallpaper has been detected + blurred ([`super::idle_wallpaper`]) a
/// user-priority provider overrides it with the cached blurred image. No
/// animation — the old 33 ms gradient tick burned CPU under software rendering.
pub(super) struct IdleBackground {
    /// The black bg box carrying the `.idle-bg` class (background target).
    pub(super) video_bg: gtk::Box,
    /// The visible video surface (GLArea), hidden while idle.
    pub(super) gl_area: gtk::GLArea,
    /// User-priority provider holding the blurred-wallpaper CSS; `None` = the
    /// base `.idle-bg` gradient fallback.
    pub(super) provider: RefCell<Option<gtk::CssProvider>>,
    /// Cached blurred-wallpaper path; `None` until detection succeeds (or never,
    /// if no desktop wallpaper resolves).
    pub(super) blurred_path: RefCell<Option<PathBuf>>,
    /// True while the idle background is shown (welcome up).
    pub(super) is_shown: Cell<bool>,
}

impl IdleBackground {
    /// Show the idle background: hide the gl surface, add the `.idle-bg` class,
    /// and apply the current background (blurred wallpaper if known, else the
    /// base gradient).
    pub(super) fn show(&self, display: &gtk::gdk::Display) {
        self.is_shown.set(true);
        self.gl_area.set_opacity(0.0);
        self.video_bg.add_css_class("idle-bg");
        self.apply_background(display);
    }

    /// Hide the idle background: restore the gl surface opacity, drop the class,
    /// and remove the wallpaper provider. Restoring gl opacity is load-bearing —
    /// video would stay invisible otherwise.
    pub(super) fn hide(&self, display: &gtk::gdk::Display) {
        self.is_shown.set(false);
        self.gl_area.set_opacity(1.0);
        self.video_bg.remove_css_class("idle-bg");
        self.clear_provider(display);
    }

    /// Record the detected blurred wallpaper and apply it if idle is shown now.
    /// Called from the wallpaper worker once the blurred image is cached.
    pub(super) fn set_blurred_wallpaper(&self, path: &Path, display: &gtk::gdk::Display) {
        self.blurred_path.replace(Some(path.to_path_buf()));
        if self.is_shown.get() {
            self.apply_background(display);
        }
    }

    /// (Re)apply the background: the blurred wallpaper when known (static image
    /// provider), else nothing — the base `.idle-bg` gradient shows through.
    fn apply_background(&self, display: &gtk::gdk::Display) {
        self.clear_provider(display);
        let Some(path) = self.blurred_path.borrow().clone() else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&idle_bg_image_css(&path));
        gtk::style_context_add_provider_for_display(
            display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 2,
        );
        self.provider.replace(Some(provider));
    }

    /// Remove the wallpaper provider from `display` if one is installed.
    fn clear_provider(&self, display: &gtk::gdk::Display) {
        if let Some(provider) = self.provider.borrow_mut().take() {
            gtk::style_context_remove_provider_for_display(display, &provider);
        }
    }
}

impl Drop for IdleBackground {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.borrow_mut().take() {
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_remove_provider_for_display(&display, &provider);
            }
        }
    }
}

/// Static `.idle-bg` CSS that paints the cached blurred wallpaper, scaled to
/// cover the surface. `#0e0e18` stays as the letterbox/transparency backdrop.
fn idle_bg_image_css(path: &Path) -> String {
    format!(
        ".idle-bg {{
            background-color: #0e0e18;
            background-image: url(\"{}\");
            background-size: cover;
            background-position: center;
            background-repeat: no-repeat;
        }}",
        file_url(path)
    )
}

/// Build a `file://` URL for a CSS `url()`, percent-encoding every byte that is
/// not an unreserved / path-safe character (handles spaces, quotes, `#`, `?` and
/// non-ASCII UTF-8 bytes alike).
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for &byte in path.as_os_str().to_string_lossy().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// Window aspect-ratio snap state: resizes the window so the video fills it
/// without letterbox bars. Mirrors the old `controls/aspect.rs` — snaps 200 ms
/// after a file loads and (debounced 300 ms) on every default-size change,
/// bailing when disabled, fullscreen, maximized, or the sidebar is open.
pub(super) struct AspectSnap {
    /// The content widget; the window to resize is resolved lazily from it (ADR
    /// D14). Weak → no cycle with its own size-notify signals.
    pub(super) content: glib::WeakRef<adw::ToastOverlay>,
    /// Read for `AUTO_WINDOW_RESIZE` and the snap toggle.
    pub(super) settings: Settings,
    /// Live mpv handle (read for the loaded video's pixel dimensions).
    pub(super) mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    /// Video surface, re-rendered after a resize so a paused frame redraws.
    pub(super) gl_area: gtk::GLArea,
    /// Sidebar host: snapping is skipped while the playlist sidebar is open.
    pub(super) split_view: adw::OverlaySplitView,
    /// Cached video aspect ratio (`width/height`); `0.0` until a file loads.
    pub(super) aspect: Cell<f64>,
    /// True during a programmatic resize so the size-notify does not re-enter.
    pub(super) resizing: Cell<bool>,
    /// Debounce token: a scheduled snap runs only if it is still the latest.
    pub(super) resize_gen: Cell<u64>,
}

impl AspectSnap {
    /// Refresh the cached aspect from the loaded video's pixel dimensions.
    fn update_aspect_from_mpv(&self) {
        if let Some(mpv) = self.mpv.borrow().as_ref() {
            if let Some((w, h)) = mpv.get_video_dimensions() {
                if h > 0 {
                    self.aspect.set(w as f64 / h as f64);
                }
            }
        }
    }

    /// Whether a snap is currently allowed (setting on, windowed, no sidebar).
    fn should_snap(&self) -> bool {
        let Some(window) = self
            .content
            .upgrade()
            .and_then(|content| content.root())
            .and_downcast::<adw::ApplicationWindow>()
        else {
            return false;
        };
        self.settings.get_bool(keys::AUTO_WINDOW_RESIZE)
            && !self.resizing.get()
            && !window.is_fullscreen()
            && !window.is_maximized()
            && !self.split_view.shows_sidebar()
    }

    /// Snap 200 ms after a file loads: read the new dimensions, then resize.
    pub(super) fn snap_on_load(self: &Rc<Self>) {
        let this = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(200), move || {
            this.update_aspect_from_mpv();
            this.snap();
        });
    }

    /// Debounced (300 ms) snap after a default-size change.
    pub(super) fn schedule(self: &Rc<Self>) {
        if !self.should_snap() || self.aspect.get() <= 0.0 {
            return;
        }
        let gen = self.resize_gen.get().wrapping_add(1);
        self.resize_gen.set(gen);
        let this = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if this.resize_gen.get() == gen {
                this.snap();
            }
        });
    }

    /// Resize the window's default size to match the video aspect (480×270
    /// floor), keeping the larger dimension, then force a re-render.
    fn snap(&self) {
        let Some(window) = self
            .content
            .upgrade()
            .and_then(|content| content.root())
            .and_downcast::<adw::ApplicationWindow>()
        else {
            return;
        };
        if !self.should_snap() {
            return;
        }
        let (w, h) = window.default_size();
        let Some((target_w, target_h)) = aspect_snap_target(w, h, self.aspect.get()) else {
            return;
        };

        self.resizing.set(true);
        window.set_default_size(target_w, target_h);
        self.resizing.set(false);
        self.gl_area.queue_render();
    }
}

/// Target window size that matches `aspect` (video width/height) while keeping
/// the larger current dimension and enforcing the 480×270 floor. `(w, h)` is the
/// current default size. Returns `None` when inputs are invalid or the window is
/// already within tolerance of the aspect (no resize needed).
fn aspect_snap_target(w: i32, h: i32, aspect: f64) -> Option<(i32, i32)> {
    if aspect <= 0.0 || w <= 0 || h <= 0 {
        return None;
    }
    let current = f64::from(w) / f64::from(h);
    if (current - aspect).abs() < 0.01 {
        return None;
    }
    Some(if current > aspect {
        // Too wide: shrink width to match, keep height.
        (((f64::from(h) * aspect) as i32).max(480), h)
    } else {
        // Too tall: shrink height to match, keep width.
        (w, ((f64::from(w) / aspect) as i32).max(270))
    })
}

#[cfg(test)]
mod tests {
    use super::aspect_snap_target;

    #[test]
    fn no_target_for_invalid_inputs() {
        assert_eq!(aspect_snap_target(800, 600, 0.0), None);
        assert_eq!(aspect_snap_target(800, 600, -1.5), None);
        assert_eq!(aspect_snap_target(0, 600, 1.5), None);
        assert_eq!(aspect_snap_target(800, 0, 1.5), None);
    }

    #[test]
    fn no_target_when_already_within_tolerance() {
        // 800/450 = 1.777… ≈ 16:9; within 0.01 → no resize.
        assert_eq!(aspect_snap_target(800, 450, 16.0 / 9.0), None);
        // Clearly outside tolerance still resizes (sanity for the guard).
        assert!(aspect_snap_target(800, 450, 1.20).is_some());
    }

    #[test]
    fn too_wide_shrinks_width_keeps_height() {
        // current 1000/500 = 2.0 > aspect 1.5 → width = 500*1.5 = 750, height kept.
        assert_eq!(aspect_snap_target(1000, 500, 1.5), Some((750, 500)));
    }

    #[test]
    fn too_tall_shrinks_height_keeps_width() {
        // current 600/600 = 1.0 < aspect 1.5 → height = 600/1.5 = 400, width kept.
        assert_eq!(aspect_snap_target(600, 600, 1.5), Some((600, 400)));
    }

    #[test]
    fn enforces_minimum_dimensions() {
        // Width floor: very wide window, tiny aspect → width clamps to 480.
        assert_eq!(aspect_snap_target(2000, 100, 0.5), Some((480, 100)));
        // Height floor: tall window, large aspect → height clamps to 270.
        assert_eq!(aspect_snap_target(100, 2000, 5.0), Some((100, 270)));
    }
}
