// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Desktop integration contracts.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::{Poll, Waker};

use gtk::gio;
use gtk::glib::{self, variant::ToVariant};
use relm4::gtk;

pub use crate::actions::{install_action, install_action_enabled, install_quit_action};

const PORTAL_NAME: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_OPEN_URI: &str = "org.freedesktop.portal.OpenURI";
const PORTAL_REQUEST: &str = "org.freedesktop.portal.Request";

/// What the portal opens: a web address, a file or folder in its handler, or
/// the folder holding a file with that file selected.
enum PortalTarget {
    Uri(String),
    File(PathBuf),
    Selected(PathBuf),
}

/// Opens `target` through the desktop portal, so its program starts outside
/// this process and its AppArmor profile; without a working portal, `fallback`
/// launches it directly.
fn open_through_portal(target: PortalTarget, fallback: impl FnOnce() + 'static) {
    glib::spawn_future_local(async move {
        if let Err(error) = portal_call(target).await {
            log::warn!("desktop portal could not open it ({error}); launching directly");
            fallback();
        }
    });
}

async fn portal_call(target: PortalTarget) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    let options = glib::VariantDict::new(None).end();
    let (method, path) = match target {
        PortalTarget::Uri(uri) => {
            let parameters =
                glib::Variant::tuple_from_iter(["".to_variant(), uri.to_variant(), options]);
            return call_open_uri(&bus, "OpenURI", &parameters, None).await;
        }
        PortalTarget::File(path) => ("OpenFile", path),
        PortalTarget::Selected(path) => ("OpenDirectory", path),
    };
    // A file on a network mount can take long to open: not on the main loop.
    let file = gio::spawn_blocking(move || std::fs::File::open(path))
        .await
        .map_err(|_| glib::Error::new(gio::IOErrorEnum::Failed, "opening the file panicked"))?
        .map_err(|error| glib::Error::new(gio::IOErrorEnum::Failed, &error.to_string()))?;
    let fds = gio::UnixFDList::from_array([file]);
    let parameters = glib::Variant::tuple_from_iter([
        "".to_variant(),
        glib::variant::Handle(0).to_variant(),
        options,
    ]);
    call_open_uri(&bus, method, &parameters, Some(&fds)).await
}

/// Calls `method` and waits for its request's `Response`: the reply only says
/// the portal took the request. A person who cancels the chooser gets no
/// fallback; a failure does.
async fn call_open_uri(
    bus: &gio::DBusConnection,
    method: &str,
    parameters: &glib::Variant,
    fds: Option<&gio::UnixFDList>,
) -> Result<(), glib::Error> {
    // Subscribed before the call, so a fast response is not missed.
    let responses = PortalResponses::subscribe(bus);
    let (reply, _) = bus
        .call_with_unix_fd_list_future(
            Some(PORTAL_NAME),
            PORTAL_PATH,
            PORTAL_OPEN_URI,
            method,
            Some(parameters),
            Some(glib::VariantTy::new("(o)").expect("valid type string")),
            gio::DBusCallFlags::NONE,
            -1,
            fds,
        )
        .await?;
    let handle = reply.child_value(0);
    let code = responses.response(handle.str().unwrap_or_default()).await;
    match code {
        Some(0 | 1) => Ok(()),
        _ => Err(glib::Error::new(
            gio::IOErrorEnum::Failed,
            &format!("the portal answered {code:?}"),
        )),
    }
}

/// Response codes by request path, and the task waiting on them.
type Responses = (Vec<(String, Option<u32>)>, Option<Waker>);

/// The portal's `Response` signals, kept from subscription on.
struct PortalResponses {
    received: Rc<RefCell<Responses>>,
    _subscription: gio::SignalSubscription,
}

impl PortalResponses {
    fn subscribe(bus: &gio::DBusConnection) -> Self {
        let received = Rc::new(RefCell::new(Responses::default()));
        let sink = Rc::clone(&received);
        let subscription = bus.subscribe_to_signal(
            Some(PORTAL_NAME),
            Some(PORTAL_REQUEST),
            Some("Response"),
            None,
            None,
            gio::DBusSignalFlags::NONE,
            move |signal| {
                let code = signal
                    .parameters
                    .try_child_value(0)
                    .and_then(|code| code.get::<u32>());
                let (responses, waker) = &mut *sink.borrow_mut();
                responses.push((signal.object_path.to_owned(), code));
                if let Some(waker) = waker.take() {
                    waker.wake();
                }
            },
        );
        Self {
            received,
            _subscription: subscription,
        }
    }

    /// The response code of the request at `handle`, once it arrives.
    async fn response(&self, handle: &str) -> Option<u32> {
        std::future::poll_fn(|context| {
            let (responses, waker) = &mut *self.received.borrow_mut();
            match responses.iter().find(|(path, _)| path == handle) {
                Some((_, code)) => Poll::Ready(*code),
                None => {
                    *waker = Some(context.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }
}

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
    let file = gio::File::for_path(path);
    open_through_portal(PortalTarget::File(path.to_owned()), move || {
        gtk::FileLauncher::new(Some(&file)).launch(
            gtk::Window::NONE,
            gio::Cancellable::NONE,
            |_| {},
        );
    });
}

/// Opens the folder holding `path` with `path` selected.
pub fn reveal_in_file_manager(path: &Path) {
    let file = gio::File::for_path(path);
    open_through_portal(PortalTarget::Selected(path.to_owned()), move || {
        gtk::FileLauncher::new(Some(&file)).open_containing_folder(
            gtk::Window::NONE,
            gio::Cancellable::NONE,
            |_| {},
        );
    });
}

/// Opens the web address `uri` in the browser.
pub fn open_uri(uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    open_through_portal(PortalTarget::Uri(uri.to_owned()), move || {
        launcher.launch(gtk::Window::NONE, gio::Cancellable::NONE, |_| {});
    });
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
