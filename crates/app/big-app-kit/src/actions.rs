// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! GIO action and accelerator installers.

use gtk::gio;
use gtk::prelude::*;
use relm4::gtk;

/// Register the standard `app.quit` action on `app`. Activating it
/// terminates the GApplication loop.
pub fn install_quit_action<A>(app: &A)
where
    A: IsA<gio::Application> + IsA<gio::ActionMap> + Clone + 'static,
{
    let quit_app = app.clone();
    let quit = gio::ActionEntry::builder("quit")
        .activate(move |_, _, _| quit_app.quit())
        .build();
    app.add_action_entries([quit]);
}

/// Bind each `(action, [accelerators])` pair on `app` so the listed
/// keystrokes trigger the matching action.
pub fn install_static_accels<A>(app: &A, accels: &[(&str, &[&str])])
where
    A: IsA<gtk::Application>,
{
    for (action, bindings) in accels {
        app.set_accels_for_action(action, bindings);
    }
}

/// Register a parameterless [`gio::SimpleAction`] (enabled by default)
/// on `action_map`. Convenience wrapper over
/// [`install_action_enabled`].
pub fn install_action<M>(
    action_map: &M,
    name: &str,
    on_activate: impl Fn() + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    install_action_enabled(action_map, name, true, on_activate)
}

/// Register a [`gio::SimpleAction`] on `action_map` with an initial enabled flag.
///
/// Toggling [`gio::SimpleAction::set_enabled`] on the returned action drives
/// the sensitivity of every widget bound to `<group>.<name>` through the
/// [`gtk::Widget`] `action-name` property, so callers can grey-out menus
/// and buttons without tracking individual widget references.
pub fn install_action_enabled<M>(
    action_map: &M,
    name: &str,
    enabled: bool,
    on_activate: impl Fn() + 'static,
) -> gio::SimpleAction
where
    M: IsA<gio::ActionMap>,
{
    let action = gio::SimpleAction::new(name, None);
    action.set_enabled(enabled);
    action.connect_activate(move |_, _| on_activate());
    action_map.add_action(&action);
    action
}
