//! "Play on a TV": the picker both players open to send what they are playing
//! to a Chromecast or a DLNA TV. Discovery, streaming and conversion belong to
//! the BigNetScreen daemon ([`crate::cast::daemon`]); this dialog lists its
//! receivers, starts a session, and shows that session's pause/stop while it
//! runs. Closing the dialog leaves the TV playing; reopening it shows the
//! controls again.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::cast::daemon::{Client, PlaybackStart, Receiver, State};
use crate::i18n::i18n;

/// While the dialog is open, well inside the daemon's 3-minute idle exit.
const KEEP_ALIVE: Duration = Duration::from_secs(60);

/// What to send: a local file or an HTTP(S) stream, and where to start it.
#[derive(Clone, Debug)]
pub struct TvMedia {
    pub source: String,
    pub title: String,
    pub audio: bool,
    pub start: PlaybackStart,
}

struct View {
    status: gtk::Label,
    spinner: adw::Spinner,
    receivers: gtk::ListBox,
    now_playing: adw::PreferencesGroup,
    now_title: gtk::Label,
    now_position: gtk::Label,
    pause: gtk::Button,
    stop: gtk::Button,
    rescan: gtk::Button,
    shown: RefCell<Vec<Receiver>>,
    /// Set while a start is in flight, so a second click does not start twice.
    starting: std::cell::Cell<bool>,
}

fn protocol_name(protocol: &str) -> String {
    if protocol == "dlna" {
        i18n("Smart TV (DLNA)")
    } else {
        i18n("Chromecast")
    }
}

/// Same format as the controls bar's time labels.
fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (hours, minutes, secs) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

/// Words for the daemon's errors that a viewer can act on.
fn failure_text(error: &glib::Error) -> String {
    let message = error.message();
    if message.contains("not a format") {
        i18n("This TV cannot play this file.")
    } else if message.contains("already running") {
        i18n("Something is already playing on a TV. Stop it first.")
    } else if message.contains("unknown-receiver") {
        i18n("That TV is no longer on the network.")
    } else {
        i18n("Could not start on the TV: {error}").replace("{error}", message)
    }
}

/// Open the picker over `parent`. `media` reads what the player shows at the
/// moment a TV is chosen (None when nothing is loaded), so the TV starts where
/// the viewer is; `on_started` runs once a TV plays it.
pub fn present_tv_dialog(
    parent: &impl IsA<gtk::Widget>,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: impl Fn() + 'static,
) {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .margin_top(12)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&content)
            .build(),
    ));
    let dialog = adw::Dialog::builder()
        .title(i18n("Play on a TV"))
        .content_width(440)
        // Room for the controls and a few TVs; the list scrolls past that.
        .content_height(460)
        .child(&toolbar)
        .build();

    let status_row = gtk::Box::builder().spacing(8).build();
    let spinner = adw::Spinner::new();
    let status = gtk::Label::builder()
        .label(i18n("Looking for TVs on this network…"))
        .wrap(true)
        .xalign(0.0)
        .hexpand(true)
        .build();
    status_row.append(&spinner);
    status_row.append(&status);
    content.append(&status_row);

    let now_playing = adw::PreferencesGroup::builder()
        .title(i18n("Now on the TV"))
        .visible(false)
        .build();
    let now_title = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["heading"])
        .build();
    let now_position = gtk::Label::builder()
        .xalign(0.0)
        .css_classes(["dim-label", "numeric"])
        .build();
    let pause = gtk::Button::builder()
        .label(i18n("Pause"))
        .css_classes(["pill"])
        .build();
    let stop = gtk::Button::builder()
        .label(i18n("Stop on the TV"))
        .css_classes(["pill", "destructive-action"])
        .build();
    let buttons = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::Start)
        .margin_top(6)
        .build();
    buttons.append(&pause);
    buttons.append(&stop);
    let now_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .build();
    now_box.append(&now_title);
    now_box.append(&now_position);
    now_box.append(&buttons);
    now_playing.add(&now_box);
    content.append(&now_playing);

    let receivers = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .visible(false)
        .build();
    receivers.update_property(&[gtk::accessible::Property::Label(&i18n("TVs found"))]);
    content.append(&receivers);

    let rescan = gtk::Button::builder()
        .label(i18n("Search again"))
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .visible(false)
        .build();
    content.append(&rescan);

    let view = Rc::new(View {
        status,
        spinner,
        receivers,
        now_playing,
        now_title,
        now_position,
        pause,
        stop,
        rescan,
        shown: RefCell::new(Vec::new()),
        starting: std::cell::Cell::new(false),
    });
    dialog.present(Some(parent));

    let on_started: Rc<dyn Fn()> = Rc::new(on_started);
    let dialog_weak = dialog.downgrade();
    glib::spawn_future_local(async move {
        if !Client::available().await.unwrap_or(false) {
            view.spinner.set_visible(false);
            view.status.set_label(&i18n(
                "To play on a TV or Chromecast, install BigNetScreen.",
            ));
            return;
        }
        let client = match Client::connect().await {
            Ok(client) => client,
            Err(error) => {
                view.spinner.set_visible(false);
                view.status.set_label(error.message());
                return;
            }
        };
        let Some(dialog) = dialog_weak.upgrade() else {
            return;
        };
        let refresh = {
            let client = client.clone();
            let view = view.clone();
            move || match client.state() {
                Ok(state) => render(&view, &state),
                Err(error) => view.status.set_label(error.message()),
            }
        };
        let handlers = client.connect_changed(refresh.clone());
        refresh();
        let _ = client.rescan().await;
        let _ = client.keep_alive().await;
        let alive = glib::timeout_add_local(KEEP_ALIVE, {
            let client = client.clone();
            move || {
                let client = client.clone();
                glib::spawn_future_local(async move {
                    let _ = client.keep_alive().await;
                });
                glib::ControlFlow::Continue
            }
        });
        wire_actions(&view, &client, media, on_started, refresh);
        let handlers = RefCell::new(Some((handlers, alive)));
        dialog.connect_closed(move |_| {
            if let Some((handlers, alive)) = handlers.take() {
                client.disconnect(handlers);
                alive.remove();
            }
        });
    });
}

fn wire_actions(
    view: &Rc<View>,
    client: &Client,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: Rc<dyn Fn()>,
    refresh: impl Fn() + Clone + 'static,
) {
    view.receivers.connect_row_activated({
        let view = view.clone();
        let client = client.clone();
        move |_, row| {
            let Some(receiver) = usize::try_from(row.index())
                .ok()
                .and_then(|index| view.shown.borrow().get(index).cloned())
            else {
                return;
            };
            let Some(media) = media() else {
                view.status
                    .set_label(&i18n("Open a video first, then choose the TV."));
                return;
            };
            if view.starting.replace(true) {
                return;
            }
            view.spinner.set_visible(true);
            view.status
                .set_label(&i18n("Starting on {name}…").replace("{name}", &receiver.name));
            let view = view.clone();
            let client = client.clone();
            let on_started = on_started.clone();
            glib::spawn_future_local(async move {
                let remote =
                    media.source.starts_with("http://") || media.source.starts_with("https://");
                let result = if remote {
                    let content_type = if media.audio {
                        "audio/mpeg"
                    } else {
                        "video/mp4"
                    };
                    client
                        .start_url(
                            &receiver.id,
                            &media.source,
                            content_type,
                            &media.title,
                            media.audio,
                            media.start,
                        )
                        .await
                } else {
                    client
                        .start(
                            &receiver.id,
                            std::slice::from_ref(&media.source),
                            media.start,
                        )
                        .await
                };
                view.starting.set(false);
                view.spinner.set_visible(false);
                match result {
                    Ok(_) => on_started(),
                    Err(error) => view.status.set_label(&failure_text(&error)),
                }
            });
        }
    });
    let control = |command: &'static str| {
        let view = view.clone();
        let client = client.clone();
        let refresh = refresh.clone();
        move |_: &gtk::Button| {
            let Ok(state) = client.state() else {
                return;
            };
            let command = match command {
                "toggle" if state.paused => "play",
                "toggle" => "pause",
                other => other,
            };
            let client = client.clone();
            let view = view.clone();
            let refresh = refresh.clone();
            glib::spawn_future_local(async move {
                if let Err(error) = client.control(&state.session_id, command, 0.0).await {
                    view.status.set_label(error.message());
                }
                refresh();
            });
        }
    };
    view.pause.connect_clicked(control("toggle"));
    view.stop.connect_clicked(control("stop"));
    view.rescan.connect_clicked({
        let client = client.clone();
        let view = view.clone();
        move |_| {
            view.spinner.set_visible(true);
            view.status
                .set_label(&i18n("Looking for TVs on this network…"));
            let client = client.clone();
            glib::spawn_future_local(async move {
                let _ = client.rescan().await;
            });
        }
    });
}

fn render(view: &View, state: &State) {
    let playing_here = state.owned && state.active;
    view.now_playing.set_visible(playing_here);
    if playing_here {
        let receiver = state
            .receivers
            .iter()
            .find(|r| r.id == state.receiver_id)
            .map_or_else(String::new, |r| r.name.clone());
        view.now_title.set_label(
            &i18n("{title} on {tv}")
                .replace("{title}", &state.title)
                .replace("{tv}", &receiver),
        );
        view.now_position.set_label(&if state.duration > 0.0 {
            format!("{} / {}", clock(state.position), clock(state.duration))
        } else {
            clock(state.position)
        });
        view.pause.set_label(&if state.paused {
            i18n("Continue")
        } else {
            i18n("Pause")
        });
        view.pause.set_sensitive(state.can_pause);
    }

    if *view.shown.borrow() != state.receivers {
        while let Some(row) = view.receivers.row_at_index(0) {
            view.receivers.remove(&row);
        }
        for receiver in &state.receivers {
            let row = adw::ActionRow::builder()
                .title(&receiver.name)
                .subtitle(protocol_name(&receiver.protocol))
                .activatable(true)
                .build();
            row.add_prefix(&gtk::Image::from_icon_name(
                if receiver.protocol == "dlna" {
                    "video-display-symbolic"
                } else {
                    "tv-symbolic"
                },
            ));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            view.receivers.append(&row);
        }
        view.shown.replace(state.receivers.clone());
    }
    let found = !state.receivers.is_empty();
    // One TV at a time: while ours plays, its controls are the whole view.
    view.receivers.set_visible(found && !playing_here);
    view.status.set_visible(!playing_here);
    let searching = state.status == "searching";
    if view.starting.get() {
        return;
    }
    view.spinner
        .set_visible(searching && !found && !playing_here);
    view.rescan.set_visible(!searching && !playing_here);
    view.status
        .set_label(&if !state.detail.is_empty() && !playing_here {
            state.detail.clone()
        } else if found {
            i18n("Choose the TV to play on:")
        } else if searching {
            i18n("Looking for TVs on this network…")
        } else {
            i18n("No TV found. Check that it is on and on the same network.")
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_matches_the_controls_bar() {
        assert_eq!(clock(5.2), "0:05");
        assert_eq!(clock(754.0), "12:34");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(-1.0), "0:00");
    }
}
