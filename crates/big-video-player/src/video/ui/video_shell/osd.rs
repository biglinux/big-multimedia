use super::*;
use big_media_components::surface::BigMediaImmersionController;

use std::path::{Path, PathBuf};

pub(super) struct OsdState {
    /// Top OSD revealer (header bar). Revealed/hidden together with `controls`.
    pub(super) header_revealer: gtk::Revealer,
    /// Bottom OSD revealer (transport/controls bar).
    pub(super) controls_revealer: gtk::Revealer,
    /// Shared media chrome immersion controller: owns timer, cursor hiding,
    /// revealer updates, and popover guards for media shells.
    immersion: Rc<BigMediaImmersionController>,
    /// True while playback is paused — keep the OSD shown (old "bail if paused").
    pub(super) is_paused: Cell<bool>,
    /// True while the welcome screen is up (nothing loaded) — never auto-hide.
    pub(super) is_welcome_visible: Cell<bool>,
    /// True while interactive trim mode is active — pin the OSD so the transport
    /// (scrub / play-pause) stays reachable while marking cuts. Shared (`Rc`)
    /// with the shell model and the trim key controller (one source of truth).
    pub(super) trim_active: Rc<Cell<bool>>,
    /// Header/controls menu popovers, so [`Self::hide_on_focus_out`] can tell a
    /// real alt-tab from the pseudo focus-out a menu's own grab triggers.
    menu_popovers: Vec<gtk::Popover>,
}

impl OsdState {
    pub(super) fn new(
        main_overlay: &gtk::Overlay,
        header_revealer: &gtk::Revealer,
        controls_revealer: &gtk::Revealer,
        header_popovers: Vec<gtk::Popover>,
        controls_popovers: Vec<gtk::Popover>,
        is_welcome_visible: bool,
        trim_active: Rc<Cell<bool>>,
    ) -> Rc<Self> {
        let immersion = BigMediaImmersionController::new(main_overlay);
        immersion.set_idle_timeout(Duration::from_millis(OSD_HIDE_DELAY_MS));
        immersion.set_hide_cursor_when_idle(true);
        immersion.register_revealer(header_revealer);
        immersion.register_revealer(controls_revealer);
        let menu_popovers: Vec<gtk::Popover> = header_popovers
            .into_iter()
            .chain(controls_popovers)
            .collect();
        for popover in &menu_popovers {
            immersion.register_popover_hide_guard(popover);
        }

        let state = Rc::new(Self {
            header_revealer: header_revealer.clone(),
            controls_revealer: controls_revealer.clone(),
            immersion,
            is_paused: Cell::new(false),
            is_welcome_visible: Cell::new(is_welcome_visible),
            trim_active,
            menu_popovers,
        });
        let state_weak = Rc::downgrade(&state);
        state.immersion.register_hide_guard(move || {
            state_weak
                .upgrade()
                .is_some_and(|state| state.should_keep_visible())
        });
        state
    }

    /// Keep the header and controls shown while `pinned` says so.
    pub(super) fn keep_shown_while(&self, pinned: impl Fn() -> bool + 'static) {
        self.immersion.register_hide_guard(pinned);
    }

    /// Reveal the header + controls and restore the default cursor.
    pub(super) fn show(&self) {
        self.immersion.show_now();
    }

    /// Collapse the OSD when the window loses focus (alt-tab away). Bypasses the
    /// paused guard — a paused video should still lose its overlay on focus-out,
    /// like other players — but keeps the welcome screen and trim controls up.
    pub(super) fn hide_on_focus_out(&self) {
        // Opening a header menu (hamburger / open) grabs input, which the window
        // reports as a focus-out. Don't tear down the OSD then — that would also
        // hide the menu itself (a child of the header revealer). A real alt-tab
        // happens with every menu closed, so this only skips the false positive.
        let any_menu_open = self
            .menu_popovers
            .iter()
            .any(gtk::prelude::WidgetExt::is_visible);
        if !focus_out_should_hide(
            self.is_welcome_visible.get(),
            self.trim_active.get(),
            any_menu_open,
        ) {
            return;
        }
        self.header_revealer.set_reveal_child(false);
        self.controls_revealer.set_reveal_child(false);
    }

    /// Reveal the OSD now and (re)arm the auto-hide timer. Never arms the timer
    /// while the welcome screen is up.
    pub(super) fn show_and_schedule_hide(self: &Rc<Self>) {
        self.immersion.show_and_schedule_hide();
    }

    fn should_keep_visible(&self) -> bool {
        should_keep_osd_visible(
            self.is_welcome_visible.get(),
            self.is_paused.get(),
            self.trim_active.get(),
        )
    }
}

/// Pure: on a window focus-out, collapse the OSD only when nothing pins it — no
/// welcome screen, not in trim mode, and no header menu open (a menu grab reads
/// as a focus-out, and tearing down then would hide the just-opened menu too).
fn focus_out_should_hide(welcome: bool, trim: bool, any_menu_open: bool) -> bool {
    !welcome && !trim && !any_menu_open
}

/// Pure: keep the OSD shown while the welcome screen is up, playback is paused,
/// or trim mode is active (the idle-hide timer is never armed while true).
fn should_keep_osd_visible(welcome: bool, paused: bool, trim: bool) -> bool {
    welcome || paused || trim
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
    /// What the TV plays while only the TV shows it: the surface is covered
    /// then too, by that video's own picture.
    pub(super) tv_source: RefCell<Option<String>>,
    /// `tv_source`'s frame, blurred; `None` until it is made.
    pub(super) tv_picture: RefCell<Option<PathBuf>>,
}

impl IdleBackground {
    /// Show the idle background: hide the gl surface, add the `.idle-bg` class,
    /// and apply the current background (blurred wallpaper if known, else the
    /// base gradient).
    pub(super) fn show(&self, display: &gtk::gdk::Display) {
        self.is_shown.set(true);
        self.refresh(display);
    }

    /// Hide the idle background: restore the gl surface opacity, drop the class,
    /// and remove the wallpaper provider. Restoring gl opacity is load-bearing —
    /// video would stay invisible otherwise.
    pub(super) fn hide(&self, display: &gtk::gdk::Display) {
        self.is_shown.set(false);
        self.refresh(display);
    }

    /// While only the TV plays `source`: cover the frozen frame with that
    /// video's picture at `seconds`, blurred and darkened. `None` gives the
    /// video back.
    pub(super) fn show_tv(self: &Rc<Self>, source: Option<String>, seconds: f64) {
        if *self.tv_source.borrow() == source {
            return;
        }
        self.tv_picture.take();
        self.tv_source.replace(source.clone());
        if let Some(display) = gtk::gdk::Display::default() {
            self.refresh(&display);
        }
        let Some(source) = source.filter(|source| !super::playback::is_remote_uri(source)) else {
            return;
        };
        // An ffmpeg run: off the main loop.
        let (sender, receiver) = async_channel::bounded::<PathBuf>(1);
        let wanted = source.clone();
        let spawned = std::thread::Builder::new()
            .name("tv-background".into())
            .spawn(move || {
                if let Some(picture) = super::idle_wallpaper::tv_backdrop(&wanted, seconds) {
                    let _ = sender.send_blocking(picture);
                }
            });
        if spawned.is_err() {
            return;
        }
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let (Ok(picture), Some(this)) = (receiver.recv().await, this.upgrade()) else {
                return;
            };
            if this.tv_source.borrow().as_deref() == Some(source.as_str()) {
                this.tv_picture.replace(Some(picture));
                if let Some(display) = gtk::gdk::Display::default() {
                    this.refresh(&display);
                }
            }
        });
    }

    /// Cover the surface while the welcome screen is up or only the TV plays;
    /// otherwise restore it. Restoring gl opacity is load-bearing — video
    /// would stay invisible otherwise.
    fn refresh(&self, display: &gtk::gdk::Display) {
        let covered = self.is_shown.get() || self.tv_source.borrow().is_some();
        self.gl_area.set_opacity(if covered { 0.0 } else { 1.0 });
        if covered {
            self.video_bg.add_css_class("idle-bg");
            self.apply_background(display);
        } else {
            self.video_bg.remove_css_class("idle-bg");
            self.clear_provider(display);
        }
    }

    /// Record the detected blurred wallpaper and apply it if idle is shown now.
    /// Called from the wallpaper worker once the blurred image is cached.
    pub(super) fn set_blurred_wallpaper(&self, path: &Path, display: &gtk::gdk::Display) {
        self.blurred_path.replace(Some(path.to_path_buf()));
        if self.is_shown.get() {
            self.refresh(display);
        }
    }

    /// (Re)apply the background: the TV's video picture while only the TV
    /// plays, else the blurred wallpaper when known (static image provider),
    /// else nothing — the base `.idle-bg` gradient shows through.
    fn apply_background(&self, display: &gtk::gdk::Display) {
        self.clear_provider(display);
        let tv = self.tv_source.borrow().is_some();
        let path = if tv {
            self.tv_picture.borrow().clone()
        } else {
            self.blurred_path.borrow().clone()
        };
        let Some(path) = path else {
            return;
        };
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&idle_bg_image_css(&path, tv));
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
        if let Some(provider) = self.provider.borrow_mut().take()
            && let Some(display) = gtk::gdk::Display::default()
        {
            gtk::style_context_remove_provider_for_display(&display, &provider);
        }
    }
}

/// Static `.idle-bg` CSS that paints a cached blurred image, scaled to cover
/// the surface; `dim` darkens it further, for the TV's card to read over it.
/// `#0e0e18` stays as the letterbox/transparency backdrop.
fn idle_bg_image_css(path: &Path, dim: bool) -> String {
    let shade = if dim { 0.72 } else { 0.0 };
    format!(
        ".idle-bg {{
            background-color: #0e0e18;
            background-image: linear-gradient(rgba(0, 0, 0, {shade}), rgba(0, 0, 0, {shade})),
                url(\"{}\");
            background-size: cover;
            background-position: center;
            background-repeat: no-repeat;
        }}",
        // GLib escapes `"` and `\`, so the URI cannot end the CSS string.
        gtk::gio::File::for_path(path).uri()
    )
}

/// Window aspect-ratio snap state: resizes the window so the video fills it
/// without letterbox bars. Snaps 200 ms
/// after a file loads and (debounced 300 ms) on every default-size change,
/// bailing when disabled, fullscreen, maximized, or the sidebar is open.
pub(super) struct AspectSnap {
    /// The content widget; the window to resize is resolved lazily from it (ADR
    /// D14). Weak → no cycle with its own size-notify signals.
    pub(super) content: glib::WeakRef<gtk::Box>,
    /// Read for `AUTO_WINDOW_RESIZE` and the snap toggle.
    pub(super) settings: Settings,
    /// Live mpv handle (read for the loaded video's pixel dimensions).
    pub(super) mpv: Rc<RefCell<Option<Arc<MpvBackend>>>>,
    /// Video surface, re-rendered after a resize so a paused frame redraws.
    pub(super) gl_area: gtk::GLArea,
    /// The playlist panel: no snapping while it is open, as the window the
    /// person arranged around it would jump under the pointer.
    pub(super) playlist_panel: glib::WeakRef<gtk::Revealer>,
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
        if let Some(mpv) = self.mpv.borrow().as_ref()
            && let Some((w, h)) = mpv.get_video_dimensions()
            && h > 0
        {
            self.aspect.set(w as f64 / h as f64);
        }
    }

    /// Whether a snap is currently allowed (setting on, windowed, no playlist
    /// panel open).
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
            && !self
                .playlist_panel
                .upgrade()
                .is_some_and(|panel| panel.reveals_child())
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
        let r#gen = self.resize_gen.get().wrapping_add(1);
        self.resize_gen.set(r#gen);
        let this = self.clone();
        glib::timeout_add_local_once(Duration::from_millis(300), move || {
            if this.resize_gen.get() == r#gen {
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
    use super::{aspect_snap_target, focus_out_should_hide, should_keep_osd_visible};

    #[test]
    fn focus_out_hides_only_when_unpinned() {
        assert!(focus_out_should_hide(false, false, false));
        assert!(!focus_out_should_hide(true, false, false), "welcome pins");
        assert!(!focus_out_should_hide(false, true, false), "trim pins");
        assert!(
            !focus_out_should_hide(false, false, true),
            "open menu pins (grab reads as focus-out)"
        );
    }

    #[test]
    fn osd_kept_visible_only_when_welcome_paused_or_trim() {
        assert!(!should_keep_osd_visible(false, false, false));
        assert!(should_keep_osd_visible(true, false, false));
        assert!(should_keep_osd_visible(false, true, false));
        assert!(should_keep_osd_visible(false, false, true));
    }

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
