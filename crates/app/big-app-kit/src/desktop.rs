// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Desktop integration contracts.

use std::path::Path;

use gtk::gio;
use relm4::gtk;

pub use crate::actions::{
    install_action, install_action_enabled, install_quit_action, install_static_accels,
};

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

#[cfg(test)]
mod tests;
