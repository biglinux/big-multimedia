// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Desktop integration contracts.

use std::path::Path;
use std::path::PathBuf;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use relm4::adw::Application;
use relm4::gtk;

pub use crate::actions::{
    BigActionRegistry, BigActionScope, BigActionSpec, install_action, install_action_enabled,
    install_i32_action, install_quit_action, install_radio_action, install_static_accels,
    install_string_action, install_toggle_action,
};

/// Persistent or transient desktop notification spec.
///
/// # Capabilities
///
/// `notifications`, `notifications+action`
///
/// # Archetypes
///
/// `terminal`, `editor`, `media-player`, `media-converter`, `file-manager`, `pkg-manager`, `system-monitor`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNotificationSpec {
    /// Stable identifier the desktop uses to deduplicate notifications
    /// from this app.
    pub id: String,
    /// Localized title shown in the notification body.
    pub title: String,
    /// Optional secondary text; if `None`, only the title is shown.
    pub body: Option<String>,
    /// Optional action activated when the user clicks the notification.
    pub action: Option<BigActionSpec>,
}

impl BigNotificationSpec {
    /// Build a notification spec with no body and no action.
    #[must_use]
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            body: None,
            action: None,
        }
    }
}

/// Keyboard shortcut binding: action name + accelerator string + label.
///
/// # Capabilities
///
/// `keybindings`, `shortcuts`
///
/// # Archetypes
///
/// `terminal`, `editor`, `media-player`, `file-manager`, `control-center`
///
/// # Examples
///
/// ```
/// use big_app_kit::desktop::BigShortcutSpec;
///
/// let spec = BigShortcutSpec::new(
///     "win.toggle-fullscreen",
///     "F11",
///     "Toggle fullscreen",
/// );
/// assert_eq!(spec.action, "win.toggle-fullscreen");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigShortcutSpec {
    /// Action name (e.g. `win.toggle-fullscreen`) dispatched on activation.
    pub action: String,
    /// GTK accelerator string (e.g. `<Primary>F`, `F11`).
    pub accelerator: String,
    /// Human-readable description shown in the shortcuts dialog.
    pub description: String,
}

impl BigShortcutSpec {
    /// Build a shortcut binding ready to be added to the shortcuts
    /// window or installed via [`install_static_accels`].
    #[must_use]
    pub fn new(
        action: impl Into<String>,
        accelerator: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            action: action.into(),
            accelerator: accelerator.into(),
            description: description.into(),
        }
    }
}

/// Display-free description of a menu: an ordered set of actions
/// addressed by a stable id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigMenuSpec {
    /// Identifier referenced when wiring the menu into a header bar or
    /// popover.
    pub id: String,
    /// Actions in display order.
    pub actions: Vec<BigActionSpec>,
}

impl BigMenuSpec {
    /// Build an empty menu spec; chain [`BigMenuSpec::action`] to add
    /// rows.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            actions: Vec::new(),
        }
    }

    /// Append `action` to the menu.
    #[must_use]
    pub fn action(mut self, action: BigActionSpec) -> Self {
        self.actions.push(action);
        self
    }
}

/// Run a standalone libadwaita application with explicit GApplication flags and
/// a shared activate handler.
///
/// Use this when the app needs single-instance, non-unique, or custom
/// active-window policy but still wants GTK application bootstrap owned by the
/// framework instead of repeated app-local boilerplate.
pub fn run_standalone_adwaita_application(
    app_id: &str,
    flags: gio::ApplicationFlags,
    on_activate: impl Fn(&adw::Application) + 'static,
) -> i32 {
    let app = Application::builder()
        .application_id(app_id)
        .flags(flags)
        .build();
    let _ = relm4::RelmApp::<()>::from_app(app.clone());
    app.connect_activate(on_activate);
    let code = app.run_with_args::<&str>(&[]);
    i32::from(u8::from(code))
}

/// Project each `gio::File` to its local filesystem path. Non-local
/// URIs (HTTP, remote SFTP, ...) are skipped because they have no
/// path component.
#[must_use]
pub fn collect_file_paths(files: &[gio::File]) -> Vec<PathBuf> {
    files
        .iter()
        .filter_map(gio::prelude::FileExt::path)
        .collect()
}

/// Same projection as [`collect_file_paths`] but returns each path as a
/// UTF-8 string (lossy conversion), matching the legacy `open_files`
/// signal contract used by older app frontends.
#[must_use]
pub fn collect_file_path_strings(files: &[gio::File]) -> Vec<String> {
    collect_file_paths(files)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Run `use_window` against the currently active window when it can
/// be downcast to `W`. Returns `true` on success, `false` when no
/// window is active or the downcast fails.
pub fn with_active_window<A, W>(app: &A, use_window: impl FnOnce(&W)) -> bool
where
    A: IsA<gtk::Application>,
    W: IsA<gtk::Window> + glib::object::ObjectType,
{
    let Some(window) = app.active_window() else {
        return false;
    };

    let Ok(window) = window.downcast::<W>() else {
        return false;
    };

    use_window(&window);
    true
}

/// Convert `files` to local paths and hand them to `open_files` on the
/// active window. Returns `false` when no window is available.
pub fn open_files_on_active_window<A, W>(
    app: &A,
    files: &[gio::File],
    open_files: impl FnOnce(&W, Vec<PathBuf>),
) -> bool
where
    A: IsA<gtk::Application>,
    W: IsA<gtk::Window> + glib::object::ObjectType,
{
    let paths = collect_file_paths(files);
    with_active_window::<A, W>(app, |window| open_files(window, paths))
}

/// String-typed variant of [`open_files_on_active_window`] for windows
/// that still expose a `Vec<String>` open contract.
pub fn open_file_path_strings_on_active_window<A, W>(
    app: &A,
    files: &[gio::File],
    open_files: impl FnOnce(&W, Vec<String>),
) -> bool
where
    A: IsA<gtk::Application>,
    W: IsA<gtk::Window> + glib::object::ObjectType,
{
    let paths = collect_file_path_strings(files);
    with_active_window::<A, W>(app, |window| open_files(window, paths))
}

/// Best-effort: errors and missing parents are ignored.
pub fn open_containing_folder(path: &Path) {
    if let Some(parent) = path.parent() {
        let uri = format!("file://{}", parent.display());
        let _ = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
    }
}

/// Hand `path` to the desktop default handler via the
/// `gio::AppInfo::launch_default_for_uri` portal. Errors are swallowed
/// because there is no useful recovery on the calling side.
pub fn open_file(path: &Path) {
    let uri = format!("file://{}", path.display());
    let _ = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>);
}

/// Highlights `path` inside the parent folder via the FileLauncher portal.
pub fn reveal_in_file_manager(path: &Path) {
    let file = gio::File::for_path(path);
    let launcher = gtk::FileLauncher::new(Some(&file));
    launcher.open_containing_folder(gtk::Window::NONE, gio::Cancellable::NONE, |_| {});
}

/// System power transition reachable from in-app UI flows; resolved to
/// a `systemctl` subcommand by [`BigSystemPowerAction::command_args`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigSystemPowerAction {
    /// Put the machine into S3/S0ix suspend (`systemctl suspend`).
    Suspend,
    /// Power the machine off (`systemctl poweroff`).
    PowerOff,
}

impl BigSystemPowerAction {
    /// Return the static `systemctl` argument slice for this action.
    #[must_use]
    pub fn command_args(self) -> &'static [&'static str] {
        match self {
            Self::Suspend => &["suspend"],
            Self::PowerOff => &["poweroff"],
        }
    }

    /// Start the system action and return immediately; the child is reaped in
    /// the background, so no zombie `systemctl` is left behind.
    ///
    /// # Errors
    ///
    /// Returns the spawn error when `systemctl` cannot be started.
    pub fn spawn_detached(self) -> Result<(), crate::subprocess::BigSubprocessError> {
        crate::subprocess::BigSubprocessSpec::builder()
            .program("systemctl")
            .args(self.command_args())
            .build()
            .spawn_detached()
    }
}

/// Bundle of packaging identifiers derived from the application id.
/// Used by `linux-packaging` audits and meson install hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigResourceSpec {
    /// Reverse-DNS app id (`"br.com.biglinux.App"`).
    pub app_id: String,
    /// `gettext` domain name (typically the binary name).
    pub gettext_domain: String,
    /// `.desktop` filename including extension.
    pub desktop_file_id: String,
    /// AppStream metainfo filename including `.metainfo.xml`.
    pub appstream_id: String,
}

impl BigResourceSpec {
    /// Build the packaging spec, auto-deriving the desktop and
    /// AppStream filenames from `app_id`.
    #[must_use]
    pub fn new(app_id: impl Into<String>, gettext_domain: impl Into<String>) -> Self {
        let app_id = app_id.into();
        Self {
            desktop_file_id: format!("{app_id}.desktop"),
            appstream_id: format!("{app_id}.metainfo.xml"),
            app_id,
            gettext_domain: gettext_domain.into(),
        }
    }
}

/// Localization configuration consumed by the gettext skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigGettextSpec {
    /// Gettext text domain (`bindtextdomain` argument).
    pub domain: String,
    /// Path inside the source tree where `po/POTFILES.in` lives;
    /// `xgettext` scans relative to this directory.
    pub potfiles_owner: String,
}

impl BigGettextSpec {
    /// Build a gettext spec from a domain and the directory that owns
    /// `po/POTFILES.in`.
    #[must_use]
    pub fn new(domain: impl Into<String>, potfiles_owner: impl Into<String>) -> Self {
        Self {
            domain: domain.into(),
            potfiles_owner: potfiles_owner.into(),
        }
    }
}

#[cfg(test)]
mod tests;
