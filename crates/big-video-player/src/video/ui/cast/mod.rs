//! App-local Relm4 cast (Chromecast) device picker dialog (video port, V5).
//!
//! Drives the **shared** [`CastSessionController`](crate::cast::chromecast::CastSessionController)
//! engine — only the UI is video-local. Discovery polls every 800 ms and the
//! list rebuilds only when the device count changes (keeps selection steady,
//! mirroring the legacy `device_dialog`). Picking a device reads the currently
//! loaded media from the shell's mpv handle, decides the cast strategy with the
//! pure `cast_target::decide_cast_target` helper (URL / direct file /
//! transcode / error), and runs the (blocking) probe + cast calls off the GTK
//! main loop, marshalling the result back into the component's own input.
//!
//! Push/intent model: the dialog emits `VideoCastDialogOutput` when a cast
//! starts or stops so the shell can pause local playback and reflect cast
//! status; everything cast-engine-side it does itself via the `CastSessionController`.
//!
pub mod cast_target;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::list::action_nav_row::{BigActionNavRow, BigActionNavRowSpec};
use gtk::{gio, glib};
use log::error;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::cast::chromecast::{self, CastCommand, CastSessionController, DiscoveredCastDevice};
use crate::cast::transcode::TranscodeFilters;
use crate::i18n::{i18n, ni18n};
use crate::player::mpv_backend::MpvBackend;

use cast_target::{CastTarget, CastTargetError, decide_cast_target};

/// Shared handle to the shell's live mpv backend (the dialog reads the current
/// file + position when a device is picked). `None` until the surface realizes.
type MpvHandle = Rc<RefCell<Option<Arc<MpvBackend>>>>;
type FilterSnapshot = Rc<dyn Fn() -> TranscodeFilters>;

/// Result of an off-thread cast attempt, posted back to the component.
#[derive(Debug)]
pub enum CastAttempt {
    /// Cast started on `device_name`; `transcoding` flags the ffmpeg path.
    Started {
        device_name: String,
        transcoding: bool,
        resume_secs: f64,
    },
    /// Cast failed with a user-facing message.
    Failed(String),
}

/// Init payload: the shared cast engine + the shell's mpv handle.
pub struct VideoCastDialogInit {
    /// Shared Chromecast engine (from `big-media-runtime`).
    pub cast_engine: Arc<CastSessionController>,
    /// The shell's live mpv backend handle (read on device pick).
    pub mpv: MpvHandle,
    /// Snapshot provider for transcoded cast filters.
    pub filter_snapshot: FilterSnapshot,
}

/// Messages into the cast dialog.
#[derive(Debug)]
pub enum VideoCastDialogInput {
    /// Re-scan: clear and restart discovery.
    Refresh,
    /// The dialog was reopened: restart discovery and the poll timer (the timer
    /// stops when the dialog closes).
    Reopened,
    /// A device row was activated → attempt to cast the loaded media to it.
    DevicePicked(usize),
    /// An off-thread cast attempt resolved.
    Attempt(CastAttempt),
    /// Stop the active cast session.
    StopCast,
}

/// Intents the shell acts on (pause local playback, reflect status).
#[derive(Debug)]
pub enum VideoCastDialogOutput {
    /// A cast started; the shell should pause local playback. `transcoding`
    /// flags the ffmpeg path (shown in the shell's status toast).
    Started {
        /// Display name of the device casting to.
        device_name: String,
        /// Whether the cast goes through ffmpeg transcoding.
        transcoding: bool,
    },
    /// The cast stopped; the shell may resume / clear status.
    Stopped,
}

/// Relm4 cast device picker.
pub struct VideoCastDialog {
    cast_engine: Arc<CastSessionController>,
    mpv: MpvHandle,
    filter_snapshot: FilterSnapshot,
    list_box: gtk::ListBox,
    status_label: gtk::Label,
    spinner: adw::Spinner,
    stop_button: gtk::Button,
    /// Device count last rendered, so the list rebuilds only on change.
    rendered_count: usize,
    /// Whether the discovery-poll timer is currently running (guards against
    /// spawning a duplicate timer when the dialog is reopened).
    polling: Rc<Cell<bool>>,
}

impl Component for VideoCastDialog {
    type Init = VideoCastDialogInit;
    type Input = VideoCastDialogInput;
    type Output = VideoCastDialogOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_start(16)
            .margin_end(16)
            .margin_top(8)
            .margin_bottom(16)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let status_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::Center)
            .build();
        let spinner = adw::Spinner::new();
        status_box.append(&spinner);
        let status_label = gtk::Label::builder()
            .label(i18n("Searching for devices…"))
            .css_classes(["dim-label"])
            .build();
        status_box.append(&status_label);
        root.append(&status_box);

        let list_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["boxed-list"])
            .vexpand(true)
            .build();
        let pick_sender = sender.clone();
        list_box.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            pick_sender.input(VideoCastDialogInput::DevicePicked(index));
        });
        let scroll = gtk::ScrolledWindow::builder()
            .child(&list_box)
            .vexpand(true)
            .min_content_height(150)
            .build();
        root.append(&scroll);

        let button_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::End)
            .build();
        let refresh_button = gtk::Button::builder()
            .label(i18n("Refresh"))
            .css_classes(["suggested-action"])
            .build();
        let refresh_sender = sender.clone();
        refresh_button.connect_clicked(move |_| {
            refresh_sender.input(VideoCastDialogInput::Refresh);
        });
        let stop_button = gtk::Button::builder()
            .label(i18n("Stop"))
            .css_classes(["destructive-action"])
            .sensitive(false)
            .build();
        let stop_sender = sender.clone();
        stop_button.connect_clicked(move |_| {
            stop_sender.input(VideoCastDialogInput::StopCast);
        });
        button_box.append(&refresh_button);
        button_box.append(&stop_button);
        root.append(&button_box);

        let tip = gtk::Label::builder()
            .label(i18n("Click on a device to start casting"))
            .wrap(true)
            .css_classes(["dim-label"])
            .xalign(0.0)
            .margin_top(4)
            .build();
        root.append(&tip);

        let model = Self {
            cast_engine: init.cast_engine,
            mpv: init.mpv,
            filter_snapshot: init.filter_snapshot,
            list_box,
            status_label,
            spinner,
            stop_button,
            rendered_count: 0,
            polling: Rc::new(Cell::new(false)),
        };

        model.cast_engine.start_discovery(10);
        model.spawn_discovery_poll();

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoCastDialogInput::Refresh => {
                self.cast_engine.clear_devices();
                self.rendered_count = 0;
                self.refresh_device_list();
                self.status_label.set_text(&i18n("Searching for devices…"));
                self.spinner.set_visible(true);
                self.cast_engine.start_discovery(10);
            }
            VideoCastDialogInput::Reopened => {
                self.cast_engine.clear_devices();
                self.rendered_count = 0;
                self.refresh_device_list();
                self.status_label.set_text(&i18n("Searching for devices…"));
                self.spinner.set_visible(true);
                self.cast_engine.start_discovery(10);
                // The poll timer broke when the dialog last closed; restart it
                // (guarded — no-op if somehow still running).
                self.spawn_discovery_poll();
            }
            VideoCastDialogInput::DevicePicked(index) => self.attempt_cast(index, &sender),
            VideoCastDialogInput::Attempt(attempt) => self.on_attempt(attempt, &sender),
            VideoCastDialogInput::StopCast => {
                self.cast_engine.stop_casting();
                self.stop_button.set_sensitive(false);
                let _ = sender.output(VideoCastDialogOutput::Stopped);
            }
        }
    }
}

impl VideoCastDialog {
    pub(crate) fn filter_sync(&self) -> Rc<dyn Fn(TranscodeFilters, f64)> {
        let cast_engine = Arc::clone(&self.cast_engine);
        Rc::new(move |filters, position_secs| {
            if !cast_engine.is_casting() {
                return;
            }
            let result = if cast_engine.is_transcoding() {
                cast_engine.transcode_update_filters(filters, position_secs)
            } else if filters.has_any_filter() {
                cast_engine.switch_to_transcoding(filters, position_secs)
            } else {
                Ok(())
            };
            if let Err(error) = result {
                error!("video: cast filter sync failed: {error}");
            }
        })
    }

    /// Poll discovery every 800 ms: keep the list current and restart a fresh
    /// discovery sweep whenever the previous one finished (the dialog stays
    /// "live" while open). Stops when the component's input channel closes.
    fn spawn_discovery_poll(&self) {
        // Guard against a duplicate timer (init + every Reopened both call this).
        if self.polling.get() {
            return;
        }
        self.polling.set(true);
        let cast_engine = Arc::clone(&self.cast_engine);
        let list_box = self.list_box.clone();
        let status_label = self.status_label.clone();
        let spinner = self.spinner.clone();
        let polling = Rc::clone(&self.polling);
        let mut rendered_count = self.rendered_count;
        glib::timeout_add_local(Duration::from_millis(800), move || {
            // The list_box is a strong ref; if the dialog is gone the widget is
            // unparented but still alive — the check below ends the poll once the
            // box has no parent (dialog closed). Reopening restarts it.
            if list_box.root().is_none() {
                polling.set(false);
                return glib::ControlFlow::Break;
            }
            rendered_count = render_devices(&list_box, &status_label, &cast_engine, rendered_count);
            if cast_engine.is_discovering() {
                spinner.set_visible(true);
            } else {
                spinner.set_visible(false);
                cast_engine.start_discovery(10);
            }
            glib::ControlFlow::Continue
        });
    }

    /// Rebuild the device list immediately (used after a refresh clears it).
    fn refresh_device_list(&mut self) {
        self.rendered_count = render_devices(
            &self.list_box,
            &self.status_label,
            &self.cast_engine,
            self.rendered_count,
        );
    }

    /// Attempt to cast the loaded media to the device at `index`.
    fn attempt_cast(&self, index: usize, sender: &ComponentSender<Self>) {
        let devices = self.cast_engine.get_devices();
        let Some(device) = devices.get(index).cloned() else {
            return;
        };
        let (file_path, position_secs) = {
            let mpv_ref = self.mpv.borrow();
            let Some(mpv) = mpv_ref.as_ref() else {
                sender.input(VideoCastDialogInput::Attempt(CastAttempt::Failed(i18n(
                    "No media loaded",
                ))));
                return;
            };
            (mpv.get_current_file(), mpv.get_position())
        };
        let Some(local_ip) = chromecast::get_local_ip() else {
            sender.input(VideoCastDialogInput::Attempt(CastAttempt::Failed(i18n(
                "Could not determine local IP",
            ))));
            return;
        };

        let cast_engine = Arc::clone(&self.cast_engine);
        let filters = (self.filter_snapshot)();
        let sender = sender.clone();
        glib::spawn_future_local(async move {
            let attempt = start_cast(
                &cast_engine,
                &device,
                file_path.as_deref(),
                &local_ip,
                position_secs,
                filters,
            )
            .await;
            sender.input(VideoCastDialogInput::Attempt(attempt));
        });
    }

    /// Reflect a resolved cast attempt: on success seek the device to the local
    /// position, flip the Stop button on, and tell the shell to pause; on
    /// failure surface a translated error toast through the dialog status.
    fn on_attempt(&mut self, attempt: CastAttempt, sender: &ComponentSender<Self>) {
        match attempt {
            CastAttempt::Started {
                device_name,
                transcoding,
                resume_secs,
            } => {
                if resume_secs > 1.0 {
                    self.cast_engine
                        .send_command(CastCommand::Seek(resume_secs));
                }
                self.stop_button.set_sensitive(true);
                self.status_label
                    .set_text(&format!("{} {device_name}", i18n("Connected to")));
                let _ = sender.output(VideoCastDialogOutput::Started {
                    device_name,
                    transcoding,
                });
            }
            CastAttempt::Failed(message) => {
                self.status_label.set_text(&message);
            }
        }
    }
}

/// Rebuild the device list only when the count changed; return the new count.
fn render_devices(
    list_box: &gtk::ListBox,
    status_label: &gtk::Label,
    cast_engine: &Arc<CastSessionController>,
    rendered_count: usize,
) -> usize {
    let devices = cast_engine.get_devices();
    if devices.len() != rendered_count {
        while let Some(row) = list_box.row_at_index(0) {
            list_box.remove(&row);
        }
        for device in &devices {
            let row = BigActionNavRow::new(
                BigActionNavRowSpec::new(cast_target::device_label(&device.name, &device.model))
                    .subtitle(cast_target::device_subtitle(&device.host, device.port)),
            )
            .into_root();
            list_box.append(&row);
        }
    }
    if devices.is_empty() {
        status_label.set_text(&if cast_engine.is_discovering() {
            i18n("Searching for devices…")
        } else {
            i18n("No devices found")
        });
    } else {
        let n = devices.len() as u64;
        status_label.set_text(
            &ni18n("{} device found", "{} devices found", n).replace("{}", &n.to_string()),
        );
    }
    devices.len()
}

/// Run the (blocking) probe + cast engine calls off the GTK main loop and
/// map the result to a [`CastAttempt`]. Pure-logic decision via
/// [`decide_cast_target`]; the I/O is the runtime cast engine's.
async fn start_cast(
    cast_engine: &Arc<CastSessionController>,
    device: &DiscoveredCastDevice,
    file_path: Option<&str>,
    local_ip: &str,
    position_secs: f64,
    filters: TranscodeFilters,
) -> CastAttempt {
    // A remote URL needs no probe; a local file is probed off-thread first.
    let direct_compatible = match file_path {
        Some(path) if !path.starts_with("http://") && !path.starts_with("https://") => {
            let path = path.to_string();
            gio::spawn_blocking(move || crate::cast::probe::probe_cast_compatibility(&path))
                .await
                .ok()
                .and_then(Result::ok)
                .is_some_and(|probe| probe.direct_compatible)
        }
        _ => false,
    };

    let target = decide_cast_target(
        file_path,
        direct_compatible,
        crate::cast::gpu::ffmpeg_available(),
    );
    let path = file_path.unwrap_or_default();
    let result = match target {
        CastTarget::Url { mime } => cast_engine.cast_url(device, path, mime),
        CastTarget::DirectFile => cast_engine.cast_file(device, path, local_ip),
        CastTarget::Transcode => {
            cast_engine.cast_file_transcoded(device, path, local_ip, filters, position_secs)
        }
        CastTarget::Error(CastTargetError::NoMedia) => {
            return CastAttempt::Failed(i18n("No media loaded"));
        }
        CastTarget::Error(CastTargetError::FfmpegMissing) => {
            return CastAttempt::Failed(i18n("ffmpeg is not installed — required for transcoding"));
        }
    };

    match result {
        Ok(()) => CastAttempt::Started {
            device_name: device.name.clone(),
            transcoding: cast_engine.is_transcoding(),
            resume_secs: position_secs,
        },
        Err(error) => {
            error!("video: cast failed: {error}");
            CastAttempt::Failed(error.to_string())
        }
    }
}
