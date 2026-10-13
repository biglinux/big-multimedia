// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared immersive chrome and notice policy for media apps (audio player,
//! video player, webcam, image viewer): a real `AdwHeaderBar` (top) with an
//! accessible primary menu, a bottom control bar, and an optional resizable
//! sidebar shell.
//!
//! Design: the app owns its media `gtk::Overlay`, mpv/camera/image surface, and
//! any clipped offload surface. The framework owns the repeatable shell policy:
//! root creation, the notice banner, header, primary menu, bottom revealer,
//! and sidebar split/resize policy. This keeps each app's media plumbing
//! untouched (e.g. the video player's `GtkGraphicsOffload` subsurface, which an
//! `AdwToolbarView` would throttle to ~1 fps) while shared shell mechanics are
//! not re-derived.
//!
//! The app supplies the header's primary button (a menu, a settings panel):
//! the chrome only places it before the window controls.
//! App actions go in the header slots and the bottom bar; auto-hide is driven by
//! [`BigMediaImmersionController`].

use adw::prelude::*;
use relm4::gtk;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

#[path = "media_window/sidebar.rs"]
mod sidebar;

pub use sidebar::{BigMediaSidebarShell, BigMediaSidebarSpec};

const DEFAULT_IMMERSION_IDLE_TIMEOUT_MS: u64 = 2_000;
const MIN_IMMERSION_IDLE_TIMEOUT_MS: u64 = 250;

/// Overlay policy for one media scene layer.
///
/// Media apps repeatedly need the same GTK overlay bookkeeping: the video,
/// camera, or image surface must be clipped to the scene and must not affect
/// layout measurement, while floating UI such as spinners, welcome panels, and
/// OSD chrome should be stacked in a predictable order. This display-free spec
/// keeps that policy testable before a rendered GTK smoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BigMediaOverlayLayerSpec {
    should_clip_layer: bool,
    should_measure_layer: bool,
}

impl BigMediaOverlayLayerSpec {
    /// Layer policy for the primary media surface.
    ///
    /// Use for mpv/GLArea/camera/image widgets that should be clipped to the
    /// media scene and ignored by the overlay's size request.
    #[must_use]
    pub const fn clipped_media() -> Self {
        Self {
            should_clip_layer: true,
            should_measure_layer: false,
        }
    }

    /// Layer policy for floating UI that should not influence layout size.
    ///
    /// Use for OSD controls, buffering spinners, pointer hit regions, or other
    /// overlays whose position is controlled by their own alignment.
    #[must_use]
    pub const fn floating_overlay() -> Self {
        Self {
            should_clip_layer: false,
            should_measure_layer: false,
        }
    }

    /// Whether GTK should clip this layer to the overlay allocation.
    #[must_use]
    pub const fn should_clip_layer(self) -> bool {
        self.should_clip_layer
    }

    /// Whether GTK should include this layer in the overlay's size request.
    #[must_use]
    pub const fn should_measure_layer(self) -> bool {
        self.should_measure_layer
    }
}

/// Semantic role for one media overlay child.
///
/// The role names the user's visible layer purpose and carries the shared
/// clip/measurement policy. Consumers should add role layers in stacking order:
/// media surfaces first, then empty/welcome panels, then transient status
/// overlays, then chrome via [`BigMediaChromePlacement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigMediaOverlayLayerRole {
    /// Primary mpv, GLArea, camera preview, or image viewport surface.
    PrimaryMediaSurface,
    /// Fallback renderer used when the primary surface cannot be realized.
    FallbackMediaSurface,
    /// Empty-state or first-run surface shown before media is active.
    WelcomeOverlay,
    /// Transient status UI such as buffering, loading, or progress overlays.
    StatusOverlay,
}

impl BigMediaOverlayLayerRole {
    /// Clip and measurement policy for this layer role.
    #[must_use]
    pub const fn layer_spec(self) -> BigMediaOverlayLayerSpec {
        match self {
            Self::PrimaryMediaSurface | Self::FallbackMediaSurface => {
                BigMediaOverlayLayerSpec::clipped_media()
            }
            Self::WelcomeOverlay | Self::StatusOverlay => {
                BigMediaOverlayLayerSpec::floating_overlay()
            }
        }
    }

    /// Relative order used by consumers when assembling a media scene.
    #[must_use]
    pub const fn stacking_order(self) -> u8 {
        match self {
            Self::PrimaryMediaSurface | Self::FallbackMediaSurface => 10,
            Self::WelcomeOverlay => 30,
            Self::StatusOverlay => 40,
        }
    }
}

/// Which part of [`BigMediaChrome`] to add at the current overlay position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum BigMediaChromePlacement {
    /// Add the header revealer only.
    Header,
    /// Add the bottom revealer only.
    Bottom,
    /// Add the header revealer and then the bottom revealer.
    HeaderThenBottom,
}

/// Shared `GtkOverlay` stack for full-bleed media scenes.
///
/// The app still owns its renderer widgets and domain state. This wrapper owns
/// only the repeatable GTK policy around the scene: one background child plus
/// ordered overlay layers with explicit clip/measurement behavior.
#[derive(Debug, Clone)]
pub struct BigMediaOverlayStack {
    overlay: gtk::Overlay,
    last_semantic_layer_order: Rc<Cell<u8>>,
}

impl BigMediaOverlayStack {
    /// Create a media overlay stack around the background widget.
    #[must_use]
    pub fn new(background: &impl IsA<gtk::Widget>) -> Self {
        let overlay = gtk::Overlay::builder()
            .hexpand(true)
            .vexpand(true)
            .child(background)
            .build();
        Self {
            overlay,
            last_semantic_layer_order: Rc::new(Cell::new(0)),
        }
    }

    /// The assembled `GtkOverlay`.
    #[must_use]
    pub fn overlay(&self) -> &gtk::Overlay {
        &self.overlay
    }

    /// Add a layer using an explicit clip/measurement policy.
    pub fn add_layer(&self, layer: &impl IsA<gtk::Widget>, layer_spec: BigMediaOverlayLayerSpec) {
        self.overlay.add_overlay(layer);
        self.overlay
            .set_clip_overlay(layer, layer_spec.should_clip_layer());
        self.overlay
            .set_measure_overlay(layer, layer_spec.should_measure_layer());
    }

    /// Add a layer using the shared semantic role policy.
    pub fn add_layer_with_role(
        &self,
        layer: &impl IsA<gtk::Widget>,
        layer_role: BigMediaOverlayLayerRole,
    ) {
        let previous_order = self.last_semantic_layer_order.get();
        let next_order = layer_role.stacking_order();
        debug_assert!(
            next_order >= previous_order,
            "media overlay layer {layer_role:?} was added after a later layer order {previous_order}"
        );
        self.last_semantic_layer_order
            .set(previous_order.max(next_order));
        self.add_layer(layer, layer_role.layer_spec());
    }

    /// Add the primary media surface for the scene.
    pub fn add_primary_media_surface(&self, surface: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(surface, BigMediaOverlayLayerRole::PrimaryMediaSurface);
    }

    /// Add a fallback media surface for GL-less or software-rendered sessions.
    pub fn add_fallback_media_surface(&self, surface: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(surface, BigMediaOverlayLayerRole::FallbackMediaSurface);
    }

    /// Add an empty-state or welcome overlay.
    pub fn add_welcome_overlay(&self, overlay: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(overlay, BigMediaOverlayLayerRole::WelcomeOverlay);
    }

    /// Add transient status UI such as buffering or loading state.
    pub fn add_status_overlay(&self, overlay: &impl IsA<gtk::Widget>) {
        self.add_layer_with_role(overlay, BigMediaOverlayLayerRole::StatusOverlay);
    }

    /// Install media chrome at the current overlay position.
    pub fn install_chrome(&self, chrome: &BigMediaChrome, placement: BigMediaChromePlacement) {
        match placement {
            BigMediaChromePlacement::Header => chrome.install_header_into(&self.overlay),
            BigMediaChromePlacement::Bottom => chrome.install_bottom_into(&self.overlay),
            BigMediaChromePlacement::HeaderThenBottom => chrome.install_into(&self.overlay),
        }
    }
}

/// Create the shared media root: a vertical box the shell mounts its content
/// (and its [`MediaNotice`]) into.
#[must_use]
pub fn create_media_root() -> gtk::Box {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root
}

/// A message for the person in a banner that takes its own row in the layout:
/// it moves the content aside instead of covering it, as a toast covered the
/// controls and stacked up by the dozen. One at a time; asking again for the
/// one already up changes nothing. It stays until closed or replaced: a row
/// that went away by itself would move the controls under the pointer.
#[derive(Clone)]
pub struct MediaNotice {
    banner: adw::Banner,
    action: Rc<RefCell<Option<Box<dyn FnOnce()>>>>,
}

impl Default for MediaNotice {
    fn default() -> Self {
        Self::new()
    }
}

impl MediaNotice {
    /// A hidden notice; nothing shows until [`Self::show`] or [`Self::show_action`].
    #[must_use]
    pub fn new() -> Self {
        let notice = Self {
            banner: adw::Banner::builder().revealed(false).build(),
            action: Rc::default(),
        };
        let action = notice.action.clone();
        notice.banner.connect_button_clicked(move |banner| {
            banner.set_revealed(false);
            if let Some(action) = action.borrow_mut().take() {
                action();
            }
        });
        notice
    }

    /// The banner, to place where the shell wants its messages.
    #[must_use]
    pub fn widget(&self) -> &adw::Banner {
        &self.banner
    }

    /// Show `message` with a button that closes it.
    pub fn show(&self, message: &str) {
        self.present(message, &crate::i18n::t("Close"), None);
    }

    /// Show `message` with a button that does `action`, closing it.
    pub fn show_action(&self, message: &str, label: &str, action: impl FnOnce() + 'static) {
        self.present(message, label, Some(Box::new(action)));
    }

    fn present(&self, message: &str, label: &str, action: Option<Box<dyn FnOnce()>>) {
        *self.action.borrow_mut() = action;
        if !(self.banner.is_revealed() && self.banner.title() == message) {
            self.banner.set_title(message);
            self.banner.set_revealed(true);
        }
        self.banner.set_button_label(Some(label));
    }
}

const WINDOW_MAXIMIZE_ICON: &str = "window-maximize-symbolic";
const WINDOW_RESTORE_ICON: &str = "window-restore-symbolic";
const WINDOW_CONTROL_CLASS: &str = "big-media-window-control";

/// Flat, frameless icon button for a floating-header window control.
fn media_window_control_button(icon_name: &str) -> gtk::Button {
    gtk::Button::builder()
        .child(&gtk::Image::from_icon_name(icon_name))
        .css_classes([WINDOW_CONTROL_CLASS])
        .has_frame(false)
        .valign(gtk::Align::Center)
        .build()
}

/// The top-level `gtk::Window` hosting a control button, if realized.
fn window_for_control(control: &gtk::Button) -> Option<gtk::Window> {
    control.root()?.downcast::<gtk::Window>().ok()
}

/// Swap the maximize/restore glyph to match the window's maximized state.
fn update_maximize_restore_icon(button: &gtk::Button, is_maximized: bool) {
    let Some(image) = button.child().and_downcast::<gtk::Image>() else {
        return;
    };
    image.set_icon_name(Some(if is_maximized {
        WINDOW_RESTORE_ICON
    } else {
        WINDOW_MAXIMIZE_ICON
    }));
}

/// Shared floating chrome (top `AdwHeaderBar` + bottom bar, each in a `Revealer`)
/// for media-viewer apps. Build it, fill the header slots + bottom child, then
/// [`install_into`](Self::install_into) the app's `gtk::Overlay` (after the app
/// has added its media surfaces, so the bars stack on top).
#[derive(Debug, Clone)]
pub struct BigMediaChrome {
    header: adw::HeaderBar,
    header_revealer: gtk::Revealer,
    bottom_revealer: gtk::Revealer,
    window_controls: gtk::Box,
    minimize_button: gtk::Button,
    maximize_restore_button: gtk::Button,
    close_button: gtk::Button,
}

impl BigMediaChrome {
    /// Build the chrome. `reveal_initially` controls whether the bars start
    /// shown (e.g. `false` for a welcome/background state with no media yet).
    #[must_use]
    pub fn new(menu_button: &impl IsA<gtk::Widget>, reveal_initially: bool) -> Self {
        let header = adw::HeaderBar::new();
        header.set_show_start_title_buttons(false);
        header.set_show_end_title_buttons(false);
        // Custom window controls: the OSD header floats over edge-to-edge video,
        // so it can't use the compositor's native title buttons (disabled above).
        // Draw our own min/maximize-restore/close wired to the window, hidden
        // until an app opts in via `show_end_window_controls`.
        let minimize_button = media_window_control_button("window-minimize-symbolic");
        minimize_button.connect_clicked(|button| {
            if let Some(window) = window_for_control(button) {
                window.minimize();
            }
        });
        let maximize_restore_button = media_window_control_button(WINDOW_MAXIMIZE_ICON);
        maximize_restore_button.connect_root_notify(|button| {
            let Some(window) = window_for_control(button) else {
                return;
            };
            update_maximize_restore_icon(button, window.is_maximized());
            let button = button.downgrade();
            window.connect_maximized_notify(move |window| {
                if let Some(button) = button.upgrade() {
                    update_maximize_restore_icon(&button, window.is_maximized());
                }
            });
        });
        maximize_restore_button.connect_clicked(|button| {
            let Some(window) = window_for_control(button) else {
                return;
            };
            if window.is_maximized() {
                window.unmaximize();
            } else {
                window.maximize();
            }
        });
        let close_button = media_window_control_button("window-close-symbolic");
        close_button.connect_clicked(|button| {
            if let Some(window) = window_for_control(button) {
                window.close();
            }
        });
        // Fullscreen has no window to minimize, maximize or close from the
        // header: the buttons go while it lasts. An inner box of their own,
        // so the app's opt-in keeps `window_controls` to itself.
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        buttons.append(&minimize_button);
        buttons.append(&maximize_restore_button);
        buttons.append(&close_button);
        buttons.connect_root_notify(|buttons| {
            let Some(window) = buttons.root().and_downcast::<gtk::Window>() else {
                return;
            };
            buttons.set_visible(!window.is_fullscreen());
            let buttons = buttons.downgrade();
            window.connect_fullscreened_notify(move |window| {
                if let Some(buttons) = buttons.upgrade() {
                    buttons.set_visible(!window.is_fullscreen());
                }
            });
        });
        let window_controls = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        window_controls.set_valign(gtk::Align::Center);
        window_controls.set_visible(false);
        window_controls.append(&buttons);
        header.pack_end(&window_controls);
        header.pack_end(menu_button);

        // `.osd-header` (app-themed) wrapper: the built-in `.osd` class forces
        // light-on-dark and breaks header icons in the light theme.
        let header_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        header_box.add_css_class("osd-header");
        header_box.append(&header);
        let header_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::Start)
            .transition_type(gtk::RevealerTransitionType::Crossfade)
            .reveal_child(reveal_initially)
            .child(&header_box)
            .build();

        let bottom_revealer = gtk::Revealer::builder()
            .valign(gtk::Align::End)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(reveal_initially)
            .build();

        Self {
            header,
            header_revealer,
            bottom_revealer,
            window_controls,
            minimize_button,
            maximize_restore_button,
            close_button,
        }
    }

    /// Configure and reveal the end-of-header window controls (minimize,
    /// maximize/restore, close). The controls stay hidden until this is called;
    /// apps pass already-translated tooltip labels.
    pub fn show_end_window_controls(
        &self,
        minimize_label: &str,
        maximize_restore_label: &str,
        close_label: &str,
    ) {
        self.minimize_button.set_tooltip_text(Some(minimize_label));
        self.maximize_restore_button
            .set_tooltip_text(Some(maximize_restore_label));
        self.close_button.set_tooltip_text(Some(close_label));
        self.window_controls.set_visible(true);
    }

    /// Add both bars to the app's overlay as top overlays. Call after the app
    /// has added media surfaces so the chrome stays on top.
    pub fn install_into(&self, overlay: &gtk::Overlay) {
        self.install_header_into(overlay);
        self.install_bottom_into(overlay);
    }

    /// Add only the header bar to the app's overlay. Use this when the app needs
    /// explicit overlay stacking order for OSD, welcome, buffering, or sidebars.
    pub fn install_header_into(&self, overlay: &gtk::Overlay) {
        overlay.add_overlay(&self.header_revealer);
        overlay.set_measure_overlay(&self.header_revealer, false);
    }

    /// Add only the bottom bar to the app's overlay. Use this when the app needs
    /// explicit overlay stacking order for OSD, welcome, buffering, or sidebars.
    pub fn install_bottom_into(&self, overlay: &gtk::Overlay) {
        overlay.add_overlay(&self.bottom_revealer);
        overlay.set_measure_overlay(&self.bottom_revealer, false);
    }

    /// Set the bottom bar's content (the app's transport / capture controls).
    pub fn set_bottom_child(&self, child: &impl IsA<gtk::Widget>) {
        self.bottom_revealer.set_child(Some(child));
    }

    /// Add an app action to the header START (left of the title).
    pub fn add_header_start(&self, widget: &impl IsA<gtk::Widget>) {
        self.header.pack_start(widget);
    }

    /// The real `AdwHeaderBar` (for direct tweaks; prefer the slot helpers).
    #[must_use]
    pub fn header(&self) -> &adw::HeaderBar {
        &self.header
    }

    /// The header reveal widget (apps wire auto-hide / OSD onto it).
    #[must_use]
    pub fn header_revealer(&self) -> &gtk::Revealer {
        &self.header_revealer
    }

    /// The bottom reveal widget (apps wire auto-hide / OSD onto it).
    #[must_use]
    pub fn bottom_revealer(&self) -> &gtk::Revealer {
        &self.bottom_revealer
    }
}

/// Shared idle-reveal controller for media chrome.
///
/// The controller observes pointer and keyboard activity on a media root, keeps
/// registered revealers or opacity targets visible while the user is active,
/// and hides them after a short idle timeout unless the app temporarily
/// inhibits hiding while a popover or dialog is open.
pub struct BigMediaImmersionController {
    opacity_targets: RefCell<Vec<gtk::glib::WeakRef<gtk::Widget>>>,
    reveal_targets: RefCell<Vec<gtk::glib::WeakRef<gtk::Revealer>>>,
    hide_guard_predicates: RefCell<Vec<Box<dyn Fn() -> bool>>>,
    cursor_host: gtk::glib::WeakRef<gtk::Widget>,
    idle_timeout_ms: Cell<u64>,
    inhibit_count: Cell<u32>,
    should_hide_cursor_when_idle: Cell<bool>,
    idle_timer: RefCell<Option<gtk::glib::SourceId>>,
    self_weak: RefCell<Weak<Self>>,
}

impl BigMediaImmersionController {
    /// Create a controller whose cursor is restored on `cursor_host` activity.
    #[must_use]
    pub fn new(cursor_host: &impl IsA<gtk::Widget>) -> Rc<Self> {
        let controller = Rc::new(Self {
            opacity_targets: RefCell::new(Vec::new()),
            reveal_targets: RefCell::new(Vec::new()),
            hide_guard_predicates: RefCell::new(Vec::new()),
            cursor_host: cursor_host.upcast_ref::<gtk::Widget>().downgrade(),
            idle_timeout_ms: Cell::new(DEFAULT_IMMERSION_IDLE_TIMEOUT_MS),
            inhibit_count: Cell::new(0),
            should_hide_cursor_when_idle: Cell::new(false),
            idle_timer: RefCell::new(None),
            self_weak: RefCell::new(Weak::new()),
        });
        *controller.self_weak.borrow_mut() = Rc::downgrade(&controller);
        controller
    }

    /// Override the default idle timeout used before hiding registered chrome.
    pub fn set_idle_timeout(&self, idle_timeout: std::time::Duration) {
        self.idle_timeout_ms
            .set(bounded_immersion_idle_timeout_ms(idle_timeout));
    }

    /// Hide the cursor on the controller host when media chrome becomes idle.
    pub fn set_hide_cursor_when_idle(&self, should_hide: bool) {
        self.should_hide_cursor_when_idle.set(should_hide);
    }

    /// Hide `revealer` when the media surface becomes idle. Focus inside the
    /// revealer does not pin it open: a media OSD must fade a few seconds after
    /// the pointer stops even though a transport control keeps keyboard focus
    /// after a click (which is the normal state). The pointer resting on it
    /// does: someone reading a control is about to click it, and a click
    /// landing after the fade would hit the video instead.
    pub fn register_revealer(&self, revealer: &gtk::Revealer) {
        self.reveal_targets.borrow_mut().push(revealer.downgrade());
        // Moving off the chrome re-arms the fade through the media surface's
        // own motion/leave handlers, so no leave handler here: one would also
        // fire when the fade itself unmaps the chrome and reveal it again.
        let hover = gtk::EventControllerMotion::new();
        revealer.add_controller(hover.clone());
        let hover = hover.downgrade();
        self.register_hide_guard(move || {
            hover
                .upgrade()
                .is_some_and(|hover| hover.contains_pointer())
        });
    }

    /// Keep chrome visible while `should_keep_visible` returns `true`.
    pub fn register_hide_guard(&self, should_keep_visible: impl Fn() -> bool + 'static) {
        self.hide_guard_predicates
            .borrow_mut()
            .push(Box::new(should_keep_visible));
    }

    /// Keep chrome visible while `popover` is open.
    pub fn register_popover_hide_guard(&self, popover: &gtk::Popover) {
        let popover = popover.downgrade();
        self.register_hide_guard(move || {
            popover
                .upgrade()
                .is_some_and(|popover| popover.is_visible())
        });
    }

    /// Reveal registered chrome without arming an idle hide.
    pub fn show_now(&self) {
        self.cancel_pending();
        self.show_all();
    }

    /// Reveal registered chrome and arm the idle hide timer when guards allow it.
    pub fn show_and_schedule_hide(&self) {
        self.cancel_pending();
        self.show_all();
        if self.should_keep_chrome_visible() {
            return;
        }
        let controller = self.self_weak.borrow().clone();
        let idle_timeout = std::time::Duration::from_millis(self.idle_timeout_ms.get());
        let id = gtk::glib::timeout_add_local(idle_timeout, move || {
            if let Some(controller) = controller.upgrade() {
                controller.hide_all();
                controller.idle_timer.borrow_mut().take();
            }
            gtk::glib::ControlFlow::Break
        });
        *self.idle_timer.borrow_mut() = Some(id);
    }

    fn cancel_pending(&self) {
        if let Some(source_id) = self.idle_timer.borrow_mut().take() {
            big_gtk_kit::source::remove(source_id);
        }
    }

    fn guard_snapshot(&self) -> BigMediaImmersionGuardSnapshot {
        BigMediaImmersionGuardSnapshot {
            active_inhibitor_count: self.inhibit_count.get(),
            active_hide_guard_count: self
                .hide_guard_predicates
                .borrow()
                .iter()
                .filter(|should_keep_visible| should_keep_visible())
                .count(),
        }
    }

    fn should_keep_chrome_visible(&self) -> bool {
        self.guard_snapshot().should_keep_chrome_visible()
    }

    fn show_all(&self) {
        for widget in self
            .opacity_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            widget.set_opacity(1.0);
        }
        for revealer in self
            .reveal_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            revealer.set_reveal_child(true);
        }
        if self.should_hide_cursor_when_idle.get()
            && let Some(host) = self.cursor_host.upgrade()
        {
            host.set_cursor_from_name(Some("default"));
        }
    }

    fn hide_all(&self) {
        if self.should_keep_chrome_visible() {
            return;
        }
        for widget in self
            .opacity_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            widget.set_opacity(0.0);
        }
        for revealer in self
            .reveal_targets
            .borrow()
            .iter()
            .filter_map(gtk::glib::WeakRef::upgrade)
        {
            revealer.set_reveal_child(false);
        }
        if self.should_hide_cursor_when_idle.get()
            && let Some(host) = self.cursor_host.upgrade()
        {
            host.set_cursor_from_name(Some("none"));
        }
    }
}

impl Drop for BigMediaImmersionController {
    fn drop(&mut self) {
        if let Some(source_id) = self.idle_timer.borrow_mut().take() {
            big_gtk_kit::source::remove(source_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BigMediaImmersionGuardSnapshot {
    active_inhibitor_count: u32,
    active_hide_guard_count: usize,
}

impl BigMediaImmersionGuardSnapshot {
    const fn should_keep_chrome_visible(self) -> bool {
        self.active_inhibitor_count > 0 || self.active_hide_guard_count > 0
    }
}

fn bounded_immersion_idle_timeout_ms(idle_timeout: std::time::Duration) -> u64 {
    let timeout_ms = idle_timeout.as_millis().try_into().unwrap_or(u64::MAX);
    timeout_ms.max(MIN_IMMERSION_IDLE_TIMEOUT_MS)
}

#[cfg(test)]
mod tests {
    use super::BigMediaImmersionGuardSnapshot;
    use super::{
        BigMediaChromePlacement, BigMediaOverlayLayerRole, MIN_IMMERSION_IDLE_TIMEOUT_MS,
        bounded_immersion_idle_timeout_ms,
    };

    #[test]
    fn overlay_layer_roles_follow_policy() {
        for role in [
            BigMediaOverlayLayerRole::PrimaryMediaSurface,
            BigMediaOverlayLayerRole::FallbackMediaSurface,
        ] {
            assert!(
                role.layer_spec().should_clip_layer(),
                "{role:?} should clip to the media scene"
            );
            assert!(
                !role.layer_spec().should_measure_layer(),
                "{role:?} should not influence overlay measurement"
            );
            assert_eq!(role.stacking_order(), 10);
        }

        for role in [
            BigMediaOverlayLayerRole::WelcomeOverlay,
            BigMediaOverlayLayerRole::StatusOverlay,
        ] {
            assert!(
                !role.layer_spec().should_clip_layer(),
                "{role:?} should float over the media scene"
            );
            assert!(
                !role.layer_spec().should_measure_layer(),
                "{role:?} should not influence overlay measurement"
            );
        }

        assert!(
            BigMediaOverlayLayerRole::PrimaryMediaSurface.stacking_order()
                < BigMediaOverlayLayerRole::WelcomeOverlay.stacking_order()
        );
        assert!(
            BigMediaOverlayLayerRole::WelcomeOverlay.stacking_order()
                < BigMediaOverlayLayerRole::StatusOverlay.stacking_order()
        );
    }

    #[test]
    fn chrome_placements_name_mount_points() {
        let header_placement = BigMediaChromePlacement::Header;
        assert!(matches!(header_placement, BigMediaChromePlacement::Header));
        assert_ne!(
            BigMediaChromePlacement::Header,
            BigMediaChromePlacement::Bottom
        );
        assert_eq!(
            format!("{:?}", BigMediaChromePlacement::HeaderThenBottom),
            "HeaderThenBottom"
        );
    }

    #[test]
    fn inhibitors_and_guards_pin_chrome() {
        assert!(
            BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 1,
                active_hide_guard_count: 0,
            }
            .should_keep_chrome_visible()
        );
        assert!(
            BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 0,
                active_hide_guard_count: 1,
            }
            .should_keep_chrome_visible()
        );
        assert!(
            !BigMediaImmersionGuardSnapshot {
                active_inhibitor_count: 0,
                active_hide_guard_count: 0,
            }
            .should_keep_chrome_visible()
        );
    }

    #[test]
    fn immersion_idle_timeout_never_flickers_immediately() {
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::ZERO),
            MIN_IMMERSION_IDLE_TIMEOUT_MS
        );
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::from_millis(1)),
            MIN_IMMERSION_IDLE_TIMEOUT_MS
        );
        assert_eq!(
            bounded_immersion_idle_timeout_ms(std::time::Duration::from_millis(1_500)),
            1_500
        );
    }
}
