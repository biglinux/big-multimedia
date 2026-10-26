//! "Play on a TV": what both players show to send what they are playing to a
//! Chromecast or a DLNA TV. Discovery, streaming and conversion belong to the
//! BigNetScreen daemon ([`crate::cast::daemon`]).
//!
//! [`TvLink`] is a player's one connection to it, shared by everything that
//! needs to know whether a TV is playing what this player sent: the page that
//! lists receivers ([`tv_page`], a settings-panel page in the video player and
//! a dialog in the audio player, [`present_tv_dialog`]), the card over the
//! video, and the transport controls, which drive the TV while it plays.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use big_relm4_components::time::playback_time;

use crate::cast::daemon::{Client, PlaybackStart, Receiver, State};
use crate::i18n::i18n;
use crate::settings::{Settings, keys};

mod card;
pub use card::{TvCard, TvCardAction};

/// While the page is on screen, well inside the daemon's 3-minute idle exit.
const KEEP_ALIVE: Duration = Duration::from_secs(60);

/// Frame heights offered, 0 first: the video's own size.
const HEIGHTS: [u32; 5] = [0, 720, 1080, 1440, 2160];

/// How long a switch to the next item waits for the TV to let the old go.
const SWITCH_TIMEOUT: Duration = Duration::from_secs(10);

/// What to send: a local file or an HTTP(S) stream, and where to start it.
#[derive(Clone, Debug)]
pub struct TvMedia {
    pub source: String,
    pub title: String,
    pub audio: bool,
    pub start: PlaybackStart,
}

/// Told of every change the daemon reports.
type Listener = Rc<dyn Fn()>;

/// A player's connection to BigNetScreen. Made at startup and connected on
/// first use: opening a proxy starts the daemon, which must not happen merely
/// because a player opened.
#[derive(Default)]
pub struct TvLink {
    client: RefCell<Option<Client>>,
    connecting: Cell<bool>,
    /// Why there is no client: not installed, or not reachable.
    unavailable: RefCell<Option<(String, String)>>,
    /// The daemon went away after connecting.
    lost: Cell<bool>,
    listeners: RefCell<Vec<(u64, Listener)>>,
    next_listener: Cell<u64>,
    /// The file or stream the running session was started with.
    source: RefCell<Option<String>>,
    /// Moving the TV to another item: the old session ends before the new
    /// one starts, and that gap is not the TV being stopped.
    switching: Cell<bool>,
    /// A failure nobody was waiting for (a switch), until someone shows it.
    failure: RefCell<Option<String>>,
    /// "Use the player": the TV plays on and the controls are the player's.
    /// Holds for one session; a new one has the controls again.
    released: Cell<bool>,
    /// The session `released` belongs to.
    session: RefCell<String>,
}

impl TvLink {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Reach the daemon unless already connected or trying; listeners hear
    /// the outcome. A daemon that exited while idle is started again.
    fn connect(self: &Rc<Self>) {
        if self
            .client
            .borrow()
            .as_ref()
            .is_some_and(|client| !client.alive())
        {
            self.lost.set(false);
            self.client.take();
        }
        if self.client.borrow().is_some() || self.connecting.replace(true) {
            return;
        }
        let link = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let installed = Client::available().await.unwrap_or(false);
            let result = if installed {
                Some(Client::connect().await)
            } else {
                None
            };
            let Some(link) = link.upgrade() else {
                return;
            };
            link.connecting.set(false);
            match result {
                None => {
                    link.unavailable.replace(Some((
                        i18n("BigNetScreen is not installed"),
                        i18n("Install it to play on a TV or Chromecast."),
                    )));
                }
                Some(Err(error)) => {
                    link.unavailable.replace(Some((
                        i18n("Could not reach BigNetScreen"),
                        error.message().to_string(),
                    )));
                }
                Some(Ok(client)) => {
                    let weak = Rc::downgrade(&link);
                    // Held for the player's life: the link is the player's.
                    let _ = client.connect_changed(move || {
                        if let Some(link) = weak.upgrade() {
                            link.notify();
                        }
                    });
                    link.unavailable.take();
                    link.client.replace(Some(client.clone()));
                    glib::spawn_future_local(async move {
                        let _ = client.rescan().await;
                    });
                }
            }
            link.notify();
        });
    }

    /// After the daemon went away: drop the old connection and reach it again.
    fn reconnect(self: &Rc<Self>) {
        self.lost.set(false);
        self.client.take();
        self.notify();
        self.connect();
    }

    fn client(&self) -> Option<Client> {
        self.client.borrow().clone()
    }

    fn connected(&self) -> bool {
        self.client.borrow().is_some()
    }

    fn lost(&self) -> bool {
        self.lost.get()
    }

    fn unavailable(&self) -> Option<(String, String)> {
        self.unavailable.borrow().clone()
    }

    /// Everything the daemon reports; empty before connecting.
    fn state(&self) -> State {
        let state = self.client.borrow().as_ref().map(Client::state);
        match state {
            Some(Ok(state)) => state,
            Some(Err(_)) => {
                self.lost.set(true);
                State::default()
            }
            None => State::default(),
        }
    }

    /// The session this player started, while a TV plays it.
    pub fn playing(&self) -> Option<State> {
        let state = Some(self.state()).filter(|state| state.owned && state.active);
        let session = state.as_ref().map_or("", |state| state.session_id.as_str());
        if *self.session.borrow() != session && !self.switching.get() {
            self.session.replace(session.to_string());
            self.released.set(false);
        }
        state
    }

    /// The session the player's controls drive: playing, and not released.
    pub fn attached(&self) -> Option<State> {
        self.playing().filter(|_| !self.released.get())
    }

    pub fn released(&self) -> bool {
        self.released.get()
    }

    /// Give the controls back to the player; the TV plays on.
    pub fn release(&self) {
        self.released.set(true);
        self.notify();
    }

    /// Let the controls drive the TV again.
    pub fn attach(&self) {
        self.released.set(false);
        self.notify();
    }

    /// True while a switch to another item is under way.
    pub fn switching(&self) -> bool {
        self.switching.get()
    }

    /// The file or stream the TV was last started with.
    pub fn source(&self) -> Option<String> {
        self.source.borrow().clone()
    }

    pub fn take_failure(&self) -> Option<String> {
        self.failure.take()
    }

    /// Run `changed` on every change the daemon reports; returns what
    /// [`Self::unsubscribe`] takes.
    pub fn subscribe(&self, changed: impl Fn() + 'static) -> u64 {
        let id = self.next_listener.get() + 1;
        self.next_listener.set(id);
        self.listeners.borrow_mut().push((id, Rc::new(changed)));
        id
    }

    fn unsubscribe(&self, id: u64) {
        self.listeners.borrow_mut().retain(|(each, _)| *each != id);
    }

    fn notify(&self) {
        // A listener may subscribe or unsubscribe while being told.
        let listeners: Vec<_> = self
            .listeners
            .borrow()
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect();
        for listener in listeners {
            listener();
        }
    }

    /// Send a player command to the running session: `play`, `pause`,
    /// `seek-to` (seconds), `volume` (0–1), `mute`, `unmute` or `stop`.
    pub fn control(self: &Rc<Self>, command: &'static str, value: f64) {
        let Some(client) = self.client() else {
            return;
        };
        let Some(state) = self.playing() else {
            return;
        };
        let link = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let result = client.control(&state.session_id, command, value).await;
            if let (Err(error), Some(link)) = (result, link.upgrade()) {
                link.failure.replace(Some(error.message().to_string()));
                link.notify();
            }
        });
    }

    /// Start `media` on `receiver`.
    async fn start(&self, receiver: &str, media: &TvMedia) -> Result<(), glib::Error> {
        let client = self.client().ok_or_else(|| {
            glib::Error::new(
                gtk::gio::IOErrorEnum::NotConnected,
                &i18n("BigNetScreen is unavailable"),
            )
        })?;
        let remote = media.source.starts_with("http://") || media.source.starts_with("https://");
        if remote {
            let content_type = if media.audio {
                "audio/mpeg"
            } else {
                "video/mp4"
            };
            client
                .start_url(
                    receiver,
                    &media.source,
                    content_type,
                    &media.title,
                    media.audio,
                    media.start,
                )
                .await?;
        } else {
            client
                .start(receiver, std::slice::from_ref(&media.source), media.start)
                .await?;
        }
        self.source.replace(Some(media.source.clone()));
        Ok(())
    }

    /// Play `media` on `receiver` in place of whatever this player had there:
    /// the player went to the next or previous item, or the last one ended.
    pub fn switch_to(self: &Rc<Self>, receiver: String, media: TvMedia) {
        let Some(client) = self.client() else {
            return;
        };
        if self.switching.replace(true) {
            return;
        }
        let playing = self.playing();
        self.notify();
        let link = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            if let Some(state) = playing {
                let _ = client.control(&state.session_id, "stop", 0.0).await;
            }
            // The daemon starts only while idle: wait for the old to go.
            let deadline = std::time::Instant::now() + SWITCH_TIMEOUT;
            while client.state().is_ok_and(|now| now.active) && std::time::Instant::now() < deadline
            {
                glib::timeout_future(Duration::from_millis(100)).await;
            }
            let Some(link) = link.upgrade() else {
                return;
            };
            if let Err(error) = link.start(&receiver, &media).await {
                link.failure.replace(Some(failure_text(&error)));
            }
            link.switching.set(false);
            link.notify();
        });
    }
}

/// What choosing a TV row does.
type StartOn = Rc<dyn Fn(&Receiver)>;

/// One discovered TV's row, kept to show "starting" on the one chosen.
struct TvRow {
    receiver: Receiver,
    row: adw::ActionRow,
    /// "go" chevron or a spinner.
    suffix: gtk::Stack,
}

struct View {
    link: Rc<TvLink>,
    settings: Settings,
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
    /// The link subscription and the keep-alive, while the page is on screen.
    attached: RefCell<Option<(u64, glib::SourceId)>>,
    /// Receiver id whose start is in flight: one start at a time.
    starting: RefCell<Option<String>>,
    /// The last start that failed, until the next attempt or search.
    failure: RefCell<Option<String>>,
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

/// The frame height chosen on the page; 0 keeps the video's own.
pub fn saved_height(settings: &Settings) -> u32 {
    u32::try_from(settings.get_int(keys::TV_RESOLUTION)).unwrap_or(0)
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

/// The TV's name and kind, for whoever shows the running session.
fn receiver_line(state: &State) -> String {
    state
        .receivers
        .iter()
        .find(|r| r.id == state.receiver_id)
        .map_or_else(String::new, |r| {
            format!("{} · {}", r.name, protocol_name(&r.protocol))
        })
}

/// The page, ready to be placed in a panel or dialog. `media` reads what the
/// player shows at the moment a TV is chosen (None when nothing is loaded), so
/// the TV starts where the viewer is; `on_started` runs once a TV plays it.
/// `pictures` is false for a player that only ever sends sound: it has no
/// resolution to choose.
pub fn tv_page(
    link: &Rc<TvLink>,
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
    let saved = saved_height(settings);
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
        link: link.clone(),
        settings: settings.clone(),
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
        attached: RefCell::new(None),
        starting: RefCell::new(None),
        failure: RefCell::new(None),
        pictures,
    });
    view.options.set_visible(view.pictures);

    wire_actions(&view, media, Rc::new(on_started), &stop);

    // Updates and the keep-alive only while the page is on screen: they hold
    // the page, and a page nobody sees must not keep itself or the daemon up.
    root.connect_map({
        let view = view.clone();
        move |_| attach(&view)
    });
    root.connect_unmap({
        let view = view.clone();
        move |_| detach(&view)
    });
    link.connect();
    root.upcast()
}

fn attach(view: &Rc<View>) {
    if view.attached.borrow().is_some() {
        return;
    }
    let listener = view.link.subscribe({
        let view = Rc::downgrade(view);
        move || {
            if let Some(view) = view.upgrade() {
                render(&view);
            }
        }
    });
    let keep_alive = {
        let link = Rc::downgrade(&view.link);
        move || {
            if let Some(client) = link.upgrade().and_then(|link| link.client()) {
                glib::spawn_future_local(async move {
                    let _ = client.keep_alive().await;
                });
            }
        }
    };
    keep_alive();
    let alive = glib::timeout_add_local(KEEP_ALIVE, move || {
        keep_alive();
        glib::ControlFlow::Continue
    });
    view.attached.replace(Some((listener, alive)));
    render(view);
}

fn detach(view: &View) {
    if let Some((listener, alive)) = view.attached.take() {
        view.link.unsubscribe(listener);
        big_gtk_kit::source::remove(alive);
    }
}

fn wire_actions(
    view: &Rc<View>,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: Rc<dyn Fn()>,
    stop: &gtk::Button,
) {
    let weak = Rc::downgrade(view);
    let _ = view.activate.set(Rc::new(move |receiver: &Receiver| {
        if let Some(view) = weak.upgrade() {
            start_on(&view, receiver, &media, &on_started);
        }
    }));

    view.pause.connect_clicked({
        let link = Rc::downgrade(&view.link);
        move |_| {
            let Some(link) = link.upgrade() else {
                return;
            };
            if let Some(state) = link.playing() {
                link.control(if state.paused { "play" } else { "pause" }, 0.0);
            }
        }
    });
    stop.connect_clicked({
        let link = Rc::downgrade(&view.link);
        move |_| {
            if let Some(link) = link.upgrade() {
                link.control("stop", 0.0);
            }
        }
    });
    view.rescan.connect_clicked({
        let view = Rc::downgrade(view);
        move |_| {
            let Some(view) = view.upgrade() else {
                return;
            };
            view.failure.take();
            if view.link.lost() {
                view.link.reconnect();
                return;
            }
            if let Some(client) = view.link.client() {
                glib::spawn_future_local(async move {
                    let _ = client.rescan().await;
                });
            }
        }
    });
}

fn start_on(
    view: &Rc<View>,
    receiver: &Receiver,
    media: &impl Fn() -> Option<TvMedia>,
    on_started: &Rc<dyn Fn()>,
) {
    if view.starting.borrow().is_some() {
        return;
    }
    let Some(mut media) = media() else {
        view.failure
            .replace(Some(i18n("Play something first, then choose the TV.")));
        render(view);
        return;
    };
    media.start.height = saved_height(&view.settings);
    view.failure.take();
    view.starting.replace(Some(receiver.id.clone()));
    render(view);
    // The player pauses even when the page closed while the TV started.
    let link = view.link.clone();
    let receiver = receiver.id.clone();
    let view = Rc::downgrade(view);
    let on_started = on_started.clone();
    glib::spawn_future_local(async move {
        let result = link.start(&receiver, &media).await;
        if result.is_ok() {
            on_started();
        }
        let Some(view) = view.upgrade() else {
            return;
        };
        view.starting.take();
        if let Err(error) = result {
            view.failure.replace(Some(failure_text(&error)));
        }
        render(&view);
    });
}

fn render(view: &View) {
    let state = view.link.state();
    let playing_here = state.owned && state.active;
    view.now.set_visible(playing_here);
    // One TV at a time: while ours plays, its controls are the whole page.
    view.tvs.set_visible(!playing_here);
    view.options.set_visible(!playing_here && view.pictures);
    if playing_here {
        render_now(view, &state);
    }
    render_rows(view, &state);
    render_message(view, &state);
}

/// The running session's row: title, TV, position and pause.
fn render_now(view: &View, state: &State) {
    let position = if state.duration > 0.0 {
        format!(
            "{} / {}",
            playback_time(state.position),
            playback_time(state.duration)
        )
    } else {
        playback_time(state.position)
    };
    // The title arrives a moment after the session starts.
    view.now_row.set_title(&if state.title.is_empty() {
        i18n("Starting…")
    } else {
        glib::markup_escape_text(&state.title).to_string()
    });
    view.now_row
        .set_subtitle(&glib::markup_escape_text(&format!(
            "{} · {position}",
            receiver_line(state)
        )));
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

/// One row per discovered TV, rebuilt only when the list changes.
fn render_rows(view: &View, state: &State) {
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
                    let receiver = receiver.clone();
                    row.connect_activated(move |_| activate(&receiver));
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
}

/// What the list cannot say by itself, most urgent first.
fn render_message(view: &View, state: &State) {
    let found = !state.receivers.is_empty();
    let searching = state.status == "searching";
    view.rescan
        .set_sensitive(!searching && view.link.connected());
    let failure = view.failure.borrow().clone();
    let (spinning, icon, title, subtitle) = if let Some((title, subtitle)) = view.link.unavailable()
    {
        (false, "dialog-warning-symbolic", title, subtitle)
    } else if view.link.lost() {
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
    } else if searching || !view.link.connected() {
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
    link: &Rc<TvLink>,
    settings: &Settings,
    media: impl Fn() -> Option<TvMedia> + 'static,
    on_started: impl Fn() + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Play on a TV"))
        .content_width(480)
        // Fixed: the page grows as TVs are found, and a dialog sized to its
        // first "Looking for TVs…" would scroll its own heading away.
        .content_height(420)
        .build();
    // The strip above the transport shows the session, so the dialog closes
    // once the TV plays.
    let closing = dialog.downgrade();
    let page = tv_page(link, settings, false, media, move || {
        on_started();
        if let Some(dialog) = closing.upgrade() {
            dialog.close();
        }
    });
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
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(parent));
}
