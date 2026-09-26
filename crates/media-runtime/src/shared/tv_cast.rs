//! "Play on a TV": what both players show to send what they are playing to a
//! Chromecast or a DLNA TV. Discovery, streaming and conversion belong to the
//! BigNetScreen daemon ([`crate::cast::daemon`]); this page lists its
//! receivers, starts a session, and shows that session's pause/stop while it
//! runs. The video player shows it as a page of its settings panel, the audio
//! player in a dialog ([`present_tv_dialog`]). Leaving the page leaves the TV
//! playing; coming back shows the controls again.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::cast::daemon::{Client, PlaybackStart, Receiver, State};
use crate::i18n::i18n;
use crate::settings::{Settings, keys};

/// While the page is on screen, well inside the daemon's 3-minute idle exit.
const KEEP_ALIVE: Duration = Duration::from_secs(60);

/// Frame heights offered, 0 first: the video's own size.
const HEIGHTS: [u32; 5] = [0, 720, 1080, 1440, 2160];

/// What to send: a local file or an HTTP(S) stream, and where to start it.
#[derive(Clone, Debug)]
pub struct TvMedia {
    pub source: String,
    pub title: String,
    pub audio: bool,
    pub start: PlaybackStart,
}

/// What choosing a TV row does.
type StartOn = Rc<dyn Fn(&adw::ActionRow)>;

/// One discovered TV's row, kept to show "starting" on the one chosen.
struct TvRow {
    receiver: Receiver,
    row: adw::ActionRow,
    /// "go" chevron or a spinner.
    suffix: gtk::Stack,
}

struct View {
    now: adw::PreferencesGroup,
    now_row: adw::ActionRow,
    pause: gtk::Button,
    tvs: adw::PreferencesGroup,
    /// Searching, nothing found, BigNetScreen missing, a failed start.
    message: adw::ActionRow,
    message_icon: gtk::Image,
    message_spinner: adw::Spinner,
    rescan: gtk::Button,
    options: adw::PreferencesGroup,
    rows: RefCell<Vec<TvRow>>,
    /// What choosing a TV row does; rows are rebuilt as TVs come and go.
    activate: std::cell::OnceCell<StartOn>,
    client: RefCell<Option<Client>>,
    /// Signal handlers and the keep-alive, while the page is on screen.
    attached: RefCell<Option<([glib::SignalHandlerId; 2], glib::SourceId)>>,
    /// Receiver id whose start is in flight: one start at a time.
    starting: RefCell<Option<String>>,
    /// The last start that failed, until the next attempt or search.
    failure: RefCell<Option<String>>,
    /// The daemon could not be reached; nothing else to show.
    unavailable: RefCell<Option<(String, String)>>,
    /// The daemon went away after connecting: "Search again" reconnects.
    lost: Cell<bool>,
    mapped: Cell<bool>,
    /// False in a player that only sends sound: no resolution to choose.
    pictures: bool,
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
        i18n("That TV is no longer on the network. Search again.")
    } else {
        message.to_string()
    }
}

fn resolution_label(height: u32) -> String {
    match height {
        0 => i18n("Same as the video"),
        720 => "720p (HD)".into(),
        1080 => "1080p (Full HD)".into(),
        1440 => "1440p (Quad HD)".into(),
        _ => "2160p (4K)".into(),
    }
}

/// The page, ready to be placed in a panel or dialog. `media` reads what the
/// player shows at the moment a TV is chosen (None when nothing is loaded), so
/// the TV starts where the viewer is; `on_started` runs once a TV plays it.
/// `pictures` is false for a player that only ever sends sound: it has no
/// resolution to choose.
pub fn tv_page(
    settings: &Settings,
    pictures: bool,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: impl Fn() + 'static,
) -> gtk::Widget {
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();

    let now = adw::PreferencesGroup::builder()
        .title(i18n("Now on the TV"))
        .visible(false)
        .build();
    let now_row = adw::ActionRow::builder().title_lines(2).build();
    now_row.add_prefix(&gtk::Image::from_icon_name("tv-symbolic"));
    let pause = gtk::Button::builder()
        .icon_name("media-playback-pause-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["circular"])
        .build();
    let stop = gtk::Button::builder()
        .icon_name("media-playback-stop-symbolic")
        .tooltip_text(i18n("Stop on the TV"))
        .valign(gtk::Align::Center)
        .css_classes(["circular", "destructive-action"])
        .build();
    stop.update_property(&[gtk::accessible::Property::Label(&i18n("Stop on the TV"))]);
    now_row.add_suffix(&pause);
    now_row.add_suffix(&stop);
    now.add(&now_row);
    root.append(&now);

    let rescan = gtk::Button::builder()
        .icon_name("view-refresh-symbolic")
        .tooltip_text(i18n("Search again"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    rescan.update_property(&[gtk::accessible::Property::Label(&i18n("Search again"))]);
    let tvs = adw::PreferencesGroup::builder()
        .title(i18n("TVs on this network"))
        .description(i18n("Starts on the TV where it is now, and pauses here."))
        .header_suffix(&rescan)
        .build();
    let message_icon = gtk::Image::builder().visible(false).build();
    let message_spinner = adw::Spinner::new();
    let message = adw::ActionRow::builder()
        .title(i18n("Looking for TVs…"))
        .build();
    message.add_prefix(&message_spinner);
    message.add_prefix(&message_icon);
    tvs.add(&message);
    root.append(&tvs);

    let heights = gtk::StringList::new(&[]);
    for height in HEIGHTS {
        heights.append(&resolution_label(height));
    }
    let saved = u32::try_from(settings.get_int(keys::TV_RESOLUTION)).unwrap_or(0);
    let resolution = adw::ComboRow::builder()
        .title(i18n("Resolution"))
        .subtitle(i18n("Size of the picture sent to the TV"))
        .model(&heights)
        .selected(
            HEIGHTS
                .iter()
                .position(|&h| h == saved)
                .and_then(|i| u32::try_from(i).ok())
                .unwrap_or(0),
        )
        .build();
    resolution.connect_selected_notify({
        let settings = settings.clone();
        move |row| {
            let height = HEIGHTS.get(row.selected() as usize).copied().unwrap_or(0);
            let _ = settings.put_int(keys::TV_RESOLUTION, height as i32);
        }
    });
    let options = adw::PreferencesGroup::new();
    options.add(&resolution);
    root.append(&options);

    let view = Rc::new(View {
        now,
        now_row,
        pause,
        tvs,
        message,
        message_icon,
        message_spinner,
        rescan,
        options,
        rows: RefCell::new(Vec::new()),
        activate: std::cell::OnceCell::new(),
        client: RefCell::new(None),
        attached: RefCell::new(None),
        starting: RefCell::new(None),
        failure: RefCell::new(None),
        unavailable: RefCell::new(None),
        lost: Cell::new(false),
        mapped: Cell::new(false),
        pictures,
    });
    view.options.set_visible(view.pictures);

    let height = move || {
        HEIGHTS
            .get(resolution.selected() as usize)
            .copied()
            .unwrap_or(0)
    };
    wire_actions(&view, media, height, Rc::new(on_started), &stop);

    // Signals and the keep-alive only while the page is on screen: they hold
    // the page, and a page nobody sees must not keep itself or the daemon up.
    root.connect_map({
        let view = view.clone();
        move |_| {
            view.mapped.set(true);
            attach(&view);
        }
    });
    root.connect_unmap({
        let view = view.clone();
        move |_| {
            view.mapped.set(false);
            detach(&view);
        }
    });

    glib::spawn_future_local(connect_daemon(view.clone()));
    root.upcast()
}

/// Reach the daemon (D-Bus starts it on demand) and begin a search.
async fn connect_daemon(view: Rc<View>) {
    if !Client::available().await.unwrap_or(false) {
        view.unavailable.replace(Some((
            i18n("BigNetScreen is not installed"),
            i18n("Install it to play on a TV or Chromecast."),
        )));
        render(&view, &State::default());
        return;
    }
    match Client::connect().await {
        Ok(client) => {
            view.client.replace(Some(client.clone()));
            if view.mapped.get() {
                attach(&view);
            }
            let _ = client.rescan().await;
        }
        Err(error) => {
            view.unavailable.replace(Some((
                i18n("Could not reach BigNetScreen"),
                error.message().to_string(),
            )));
            render(&view, &State::default());
        }
    }
}

fn refresh(view: &View) {
    let state = view.client.borrow().as_ref().map(Client::state);
    match state {
        Some(Ok(state)) => render(view, &state),
        Some(Err(_)) => {
            view.lost.set(true);
            render(view, &State::default());
        }
        None => {}
    }
}

fn attach(view: &Rc<View>) {
    let Some(client) = view.client.borrow().clone() else {
        return;
    };
    if view.attached.borrow().is_some() {
        return;
    }
    let handlers = client.connect_changed({
        let view = Rc::downgrade(view);
        move || {
            if let Some(view) = view.upgrade() {
                refresh(&view);
            }
        }
    });
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
    view.attached.replace(Some((handlers, alive)));
    glib::spawn_future_local(async move {
        let _ = client.keep_alive().await;
    });
    refresh(view);
}

fn detach(view: &View) {
    let attached = view.attached.take();
    if let (Some((handlers, alive)), Some(client)) = (attached, view.client.borrow().as_ref()) {
        client.disconnect(handlers);
        alive.remove();
    }
}

fn wire_actions(
    view: &Rc<View>,
    media: impl Fn() -> Option<TvMedia> + 'static,
    height: impl Fn() -> u32 + 'static,
    on_started: Rc<dyn Fn()>,
    stop: &gtk::Button,
) {
    let media = Rc::new(media);
    let height = Rc::new(height);
    let weak = Rc::downgrade(view);
    let _ = view.activate.set(Rc::new(move |row: &adw::ActionRow| {
        if let Some(view) = weak.upgrade() {
            start_on(&view, row, &media, &height, &on_started);
        }
    }));

    let control = |command: &'static str| {
        let view = Rc::downgrade(view);
        move |_: &gtk::Button| {
            let Some(view) = view.upgrade() else {
                return;
            };
            let Some(client) = view.client.borrow().clone() else {
                return;
            };
            let Ok(state) = client.state() else {
                return;
            };
            let command = match command {
                "toggle" if state.paused => "play",
                "toggle" => "pause",
                other => other,
            };
            let view = Rc::downgrade(&view);
            glib::spawn_future_local(async move {
                let result = client.control(&state.session_id, command, 0.0).await;
                if let Some(view) = view.upgrade() {
                    if let Err(error) = result {
                        view.failure.replace(Some(error.message().to_string()));
                    }
                    refresh(&view);
                }
            });
        }
    };
    view.pause.connect_clicked(control("toggle"));
    stop.connect_clicked(control("stop"));
    view.rescan.connect_clicked({
        let view = Rc::downgrade(view);
        move |_| {
            let Some(view) = view.upgrade() else {
                return;
            };
            view.failure.take();
            if view.lost.replace(false) {
                detach(&view);
                view.client.take();
                render(&view, &State::default());
                glib::spawn_future_local(connect_daemon(view));
                return;
            }
            let Some(client) = view.client.borrow().clone() else {
                return;
            };
            glib::spawn_future_local(async move {
                let _ = client.rescan().await;
            });
        }
    });
}

fn start_on(
    view: &Rc<View>,
    row: &adw::ActionRow,
    media: &Rc<impl Fn() -> Option<TvMedia> + 'static>,
    height: &Rc<impl Fn() -> u32 + 'static>,
    on_started: &Rc<dyn Fn()>,
) {
    let Some(receiver) = view
        .rows
        .borrow()
        .iter()
        .find(|tv| &tv.row == row)
        .map(|tv| tv.receiver.clone())
    else {
        return;
    };
    let Some(client) = view.client.borrow().clone() else {
        return;
    };
    if view.starting.borrow().is_some() {
        return;
    }
    let Some(mut media) = media() else {
        view.failure
            .replace(Some(i18n("Play something first, then choose the TV.")));
        refresh(view);
        return;
    };
    media.start.height = height();
    view.failure.take();
    view.starting.replace(Some(receiver.id.clone()));
    refresh(view);
    let view = Rc::downgrade(view);
    let on_started = on_started.clone();
    glib::spawn_future_local(async move {
        let remote = media.source.starts_with("http://") || media.source.starts_with("https://");
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
        let Some(view) = view.upgrade() else {
            return;
        };
        view.starting.take();
        match result {
            Ok(_) => on_started(),
            Err(error) => {
                view.failure.replace(Some(failure_text(&error)));
            }
        }
        refresh(&view);
    });
}

fn render(view: &View, state: &State) {
    let playing_here = state.owned && state.active;
    view.now.set_visible(playing_here);
    // One TV at a time: while ours plays, its controls are the whole page.
    view.tvs.set_visible(!playing_here);
    view.options.set_visible(!playing_here && view.pictures);
    if playing_here {
        let tv = state
            .receivers
            .iter()
            .find(|r| r.id == state.receiver_id)
            .map_or_else(String::new, |r| r.name.clone());
        let position = if state.duration > 0.0 {
            format!("{} / {}", clock(state.position), clock(state.duration))
        } else {
            clock(state.position)
        };
        // The title arrives a moment after the session starts.
        view.now_row.set_title(&if state.title.is_empty() {
            i18n("Starting…")
        } else {
            glib::markup_escape_text(&state.title).to_string()
        });
        view.now_row
            .set_subtitle(&glib::markup_escape_text(&format!("{tv} · {position}")));
        let (icon, label) = if state.paused {
            ("media-playback-start-symbolic", i18n("Continue"))
        } else {
            ("media-playback-pause-symbolic", i18n("Pause"))
        };
        view.pause.set_icon_name(icon);
        view.pause.set_tooltip_text(Some(&label));
        view.pause
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        view.pause.set_sensitive(state.can_pause);
    }

    let same = view
        .rows
        .borrow()
        .iter()
        .map(|tv| &tv.receiver)
        .eq(state.receivers.iter());
    if !same {
        for tv in view.rows.take() {
            view.tvs.remove(&tv.row);
        }
        let rows = state
            .receivers
            .iter()
            .map(|receiver| {
                let row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&receiver.name))
                    .activatable(true)
                    .build();
                row.add_prefix(&gtk::Image::from_icon_name(
                    if receiver.protocol == "dlna" {
                        "video-display-symbolic"
                    } else {
                        "tv-symbolic"
                    },
                ));
                let suffix = gtk::Stack::new();
                suffix.add_named(&gtk::Image::from_icon_name("go-next-symbolic"), Some("go"));
                suffix.add_named(&adw::Spinner::new(), Some("starting"));
                row.add_suffix(&suffix);
                if let Some(activate) = view.activate.get().cloned() {
                    row.connect_activated(move |row| activate(row));
                }
                view.tvs.add(&row);
                TvRow {
                    receiver: receiver.clone(),
                    row,
                    suffix,
                }
            })
            .collect();
        view.rows.replace(rows);
    }
    let starting = view.starting.borrow().clone();
    for tv in view.rows.borrow().iter() {
        let this = starting.as_deref() == Some(tv.receiver.id.as_str());
        tv.suffix
            .set_visible_child_name(if this { "starting" } else { "go" });
        tv.row.set_subtitle(&if this {
            i18n("Starting…")
        } else {
            protocol_name(&tv.receiver.protocol)
        });
        tv.row.set_sensitive(starting.is_none());
    }

    let found = !state.receivers.is_empty();
    let searching = state.status == "searching";
    view.rescan
        .set_sensitive(!searching && view.client.borrow().is_some());
    // What the list cannot say by itself, most urgent first.
    let unavailable = view.unavailable.borrow().clone();
    let failure = view.failure.borrow().clone();
    let (spinning, icon, title, subtitle) = if let Some((title, subtitle)) = unavailable {
        (false, "dialog-warning-symbolic", title, subtitle)
    } else if view.lost.get() {
        (
            false,
            "dialog-warning-symbolic",
            i18n("BigNetScreen stopped"),
            i18n("Search again to reconnect."),
        )
    } else if let Some(failure) = failure {
        (
            false,
            "dialog-warning-symbolic",
            i18n("Could not start on the TV"),
            failure,
        )
    } else if !state.detail.is_empty() {
        (
            false,
            "dialog-warning-symbolic",
            state.detail.clone(),
            String::new(),
        )
    } else if found {
        (false, "", String::new(), String::new())
    } else if searching || view.client.borrow().is_none() {
        (true, "", i18n("Looking for TVs…"), String::new())
    } else {
        (
            false,
            "network-offline-symbolic",
            i18n("No TV found"),
            i18n("Check that the TV is on and on the same network as this computer."),
        )
    };
    view.message.set_visible(!title.is_empty());
    view.message.set_title(&glib::markup_escape_text(&title));
    view.message
        .set_subtitle(&glib::markup_escape_text(&subtitle));
    view.message_spinner.set_visible(spinning);
    view.message_icon.set_visible(!icon.is_empty());
    view.message_icon.set_icon_name(Some(icon));
}

/// The audio player's entry: the page in a dialog over `parent`, for sound
/// only.
pub fn present_tv_dialog(
    parent: &impl IsA<gtk::Widget>,
    settings: &Settings,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: impl Fn() + 'static,
) {
    let page = tv_page(settings, false, media, on_started);
    page.set_margin_top(6);
    page.set_margin_bottom(18);
    page.set_margin_start(18);
    page.set_margin_end(18);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(
        &gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&page)
            .build(),
    ));
    adw::Dialog::builder()
        .title(i18n("Play on a TV"))
        .content_width(480)
        // Fixed: the page grows as TVs are found, and a dialog sized to its
        // first "Looking for TVs…" would scroll its own heading away.
        .content_height(420)
        .child(&toolbar)
        .build()
        .present(Some(parent));
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
