//! `PaneContent` — one mountable content unit, three mount points.
//!
//! The keystone of the unified-shell restructure (`RESTRUCTURE-UNIFIED-SHELL.md`
//! §4.3.1, ADR D13): a single small trait every piece of content implements so
//! the generic chrome can host it in a standalone window, a `big-host` multicall
//! module, or the shell background surface — and arrange several of them in a
//! split. The chrome depends on this trait, never on a content crate
//! (`vte4`/`libmpv`/…); each content crate implements the trait for its own type
//! (the adapters land per phase: terminal, editor, files, image, camera, audio,
//! video).
//!
//! This module is the **API skeleton only** — the base trait, its optional
//! extension traits, the typed activity/focus enums, the owner [`PaneGuard`],
//! the [`PaneFactory`] bridge, and the [`PaneNode`] split tree. No adapter and
//! no assembled tabbed widget live here yet (K2-T2+).
//!
//! # Design (why these shapes)
//!
//! - **Small, object-safe base.** Adding a required method to a public trait is a
//!   SemVer break, so [`PaneContent`] carries only what *every* container needs;
//!   each optional capability is a separate trait ([`PanePreferences`],
//!   [`PaneCommands`]) added additively. The base is object-safe, so a container
//!   holds a `Box<dyn PaneContent>`.
//! - **Layout lives in the container, not the content.** Content is a
//!   layout-ignorant *leaf*; the tab set ([`crate::layout::tab_workspace`]) and the
//!   split arrangement ([`PaneNode`]) are data the container owns. This keeps
//!   layout serde-friendly and testable, and stops content from depending on where
//!   it is mounted.
//! - **Typed activity, never a `bool`.** [`PaneActivity`] distinguishes a *job*
//!   (a running command, audio playback, a recording, a download) from mere
//!   occlusion: a job MUST keep running when [`PaneActivity::Occluded`]; only
//!   animation / redraw / preview should pause. A `set_active(bool)` would invite
//!   the "occluded ⇒ kill the job" bug.
//! - **Ownership = a single strong owner that tears down on drop.** [`PaneGuard`]
//!   is the sole strong owner of a mounted pane (generalizing the shipping
//!   `big-terminal-ui` window guards); dropping it drops the content, whose own `Drop`
//!   must unparent its widget subtree. Implementors MUST NOT hold a strong `Rc`
//!   back to their own widgets — weak-ify every closure⇄widget edge (INVARIANTS
//!   H9). A debug finalization census must show zero survivors after unmount.
//!
//! `PaneState` (layout persistence) is deliberately **not** here yet — it needs
//! `serde_json` and is only used in the persistence phase (P5); being an extension
//! trait, it is added then without breaking this API.

pub mod controller;
pub use controller::ControllerPane;

use relm4::gtk;

/// A unit of content mountable in any container: a tab, a split-pane leaf, a
/// host-module window, or the shell background surface. One trait, three mount
/// points. Small and object-safe (`Box<dyn PaneContent>`); optional capabilities
/// are the separate traits below.
///
/// # Leak contract (mandatory — INVARIANTS H9)
///
/// The container owns the boxed content via a [`PaneGuard`] and drops it on
/// unmount; the implementor's `Drop` MUST tear the whole widget subtree down
/// (unparent `set_parent` children, drop controllers). Never hold a strong `Rc`
/// back to your own widgets — weak-ify every closure⇄widget edge. A debug
/// finalization census must show full finalization (zero survivors).
///
/// # Examples
///
/// ```no_run
/// use big_relm4_components::pane::{PaneContent, KeyboardNeed};
/// use relm4::gtk;
/// use relm4::gtk::prelude::Cast;
///
/// struct EmptyPane;
/// impl PaneContent for EmptyPane {
///     fn root(&self) -> gtk::Widget {
///         gtk::Label::new(Some("nothing here")).upcast()
///     }
///     fn title(&self) -> String {
///         "Empty".to_owned()
///     }
///     fn connect_title_changed(&self, _on_change: Box<dyn Fn(&str)>) {}
///     fn keyboard_need(&self) -> KeyboardNeed {
///         KeyboardNeed::None
///     }
/// }
/// let boxed: Box<dyn PaneContent> = Box::new(EmptyPane);
/// assert_eq!(boxed.title(), "Empty");
/// ```
pub trait PaneContent {
    /// The widget to mount. Called once; the container parents/unparents it.
    fn root(&self) -> gtk::Widget;

    /// Title for the tab/header (feeds `crate::layout::tab_workspace::BigTab::title`).
    fn title(&self) -> String;

    /// Subscribe to later title changes (editor dirty-marker, terminal working
    /// directory, player now-playing). The container re-reads [`title`](Self::title)
    /// and updates the tab/header when `on_change` fires.
    fn connect_title_changed(&self, on_change: Box<dyn Fn(&str)>);

    /// Keyboard need — drives the container focus policy, especially the
    /// background surface's `gtk4_layer_shell::KeyboardMode` escalation. Static
    /// content returns [`KeyboardNeed::None`].
    fn keyboard_need(&self) -> KeyboardNeed;

    /// Activity transition. The reason is typed (see [`PaneActivity`]).
    /// Idempotent; defaulted to a no-op for static content.
    fn set_activity(&self, _activity: PaneActivity) {}
}

/// Why the container is changing a pane's activity.
///
/// The distinction this enum encodes is load-bearing: a running **job** (a
/// terminal command, audio playback, a recording, a download, a transcode) MUST
/// keep running when merely [`Occluded`](Self::Occluded) or [`Hidden`](Self::Hidden);
/// only [`UserPaused`](Self::UserPaused) (and, opt-in, [`OnBattery`](Self::OnBattery))
/// may suspend work. Animation / redraw / polling / camera *preview* SHOULD stop on
/// occlusion; jobs MUST NOT. A `bool` would lose this distinction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PaneActivity {
    /// Mapped and unobstructed — render and run normally.
    Visible,
    /// Covered by another surface. Pause animation/redraw/preview; keep jobs running.
    Occluded,
    /// Unmapped (e.g. on an inactive tab). Same rule as `Occluded` for jobs.
    Hidden,
    /// The user explicitly paused this content. Work may suspend.
    UserPaused,
    /// Running on battery; opt-in suspension of nonessential work.
    OnBattery,
}

/// Container-neutral focus requirement. Maps onto `gtk4_layer_shell::KeyboardMode`
/// for the background surface; toplevel windows ignore it (they always accept focus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardNeed {
    /// Never takes keyboard focus (static content — wallpaper, a thumbnail grid).
    None,
    /// Takes focus on demand (click/hover), releases it otherwise (desktop icons).
    OnDemand,
    /// Holds keyboard focus while mapped (terminal, editor, text entry).
    Exclusive,
}
