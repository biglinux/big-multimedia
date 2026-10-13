// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Desktop integration contracts.

use std::path::Path;

use gtk::gio;
use relm4::gtk;

pub use crate::actions::{
    install_action, install_action_enabled, install_quit_action, install_static_accels,
};

/// Opens the folder holding `path` in the file manager. Best effort: a
/// missing parent or a launch failure is ignored.
pub fn open_containing_folder(path: &Path) {
    if let Some(parent) = path.parent() {
        open_file(parent);
    }
}

/// Opens `path` with the desktop's default handler without blocking the main
/// loop. Best effort: the caller has no recovery for a launch failure.
pub fn open_file(path: &Path) {
    let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
    launcher.launch(gtk::Window::NONE, gio::Cancellable::NONE, |_| {});
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
