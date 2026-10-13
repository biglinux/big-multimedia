//! Player-side D-Bus client for BigNetScreen's shared Chromecast/DLNA daemon.
//! Discovery and transport stay in the daemon. All calls here are asynchronous.

use gio::prelude::*;
use glib::variant::StaticVariantType;

const SERVICE: &str = "br.com.biglinux.BigNetScreen.Service";
const PATH: &str = "/br/com/biglinux/BigNetScreen";
const INTERFACE: &str = "br.com.biglinux.BigNetScreen1";

type ReceiverWire = (
    String,
    String,
    String,
    String,
    bool,
    String,
    String,
    u32,
    u32,
    u32,
);
type MediaWire = (
    bool,
    String,
    Vec<String>,
    u32,
    u32,
    u64,
    u64,
    bool,
    bool,
    bool,
    bool,
    String,
    String,
);
type SessionWire = (String, String, String, f64, bool, bool, bool);

#[derive(Clone, Copy, Debug)]
pub struct PlaybackStart {
    pub seconds: f64,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
    /// Height of the 16:9 frame a TV is sent, 0 for the video's own size.
    pub height: u32,
}

impl Default for PlaybackStart {
    fn default() -> Self {
        Self {
            seconds: 0.0,
            paused: false,
            volume: 1.0,
            muted: false,
            height: 0,
        }
    }
}

impl PlaybackStart {
    fn wire(self) -> (f64, bool, f64, bool, u32) {
        (
            self.seconds,
            self.paused,
            self.volume,
            self.muted,
            self.height,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receiver {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub receivers: Vec<Receiver>,
    pub status: String,
    pub detail: String,
    pub session_id: String,
    pub receiver_id: String,
    pub owned: bool,
    pub active: bool,
    pub title: String,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    pub volume: Option<f64>,
    pub muted: Option<bool>,
}

#[derive(Clone)]
pub struct Client {
    proxy: gio::DBusProxy,
    daemon_owner: String,
}

fn invalid_reply() -> glib::Error {
    glib::Error::new(
        gio::IOErrorEnum::InvalidData,
        "BigNetScreen returned an incompatible reply",
    )
}

impl Client {
    /// Check installation/activation without starting discovery or a daemon.
    pub async fn available() -> Result<bool, glib::Error> {
        let bus = gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_AUTO_START,
            None,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await?;
        for method in ["ListActivatableNames", "ListNames"] {
            let reply = bus
                .call_future(method, None, gio::DBusCallFlags::NO_AUTO_START, 3000)
                .await?;
            let (names,) = reply.get::<(Vec<String>,)>().ok_or_else(invalid_reply)?;
            if names.iter().any(|name| name == SERVICE) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Call when the user opens sharing. D-Bus activates an installed daemon.
    pub async fn connect() -> Result<Self, glib::Error> {
        let proxy = gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::NONE,
            None,
            SERVICE,
            PATH,
            INTERFACE,
        )
        .await?;
        if proxy
            .cached_property("PlayerApi")
            .and_then(|v| v.get::<u32>())
            != Some(2)
        {
            return Err(glib::Error::new(
                gio::IOErrorEnum::NotSupported,
                "Update BigNetScreen to use player controls",
            ));
        }
        let daemon_owner = proxy
            .name_owner()
            .ok_or_else(|| {
                glib::Error::new(
                    gio::IOErrorEnum::NotConnected,
                    "BigNetScreen is unavailable",
                )
            })?
            .to_string();
        Ok(Self {
            proxy,
            daemon_owner,
        })
    }

    /// Cache notifications run on the main context that created the client.
    /// Disconnect the returned handlers when the consuming view closes.
    pub fn connect_changed(&self, changed: impl Fn() + 'static) -> [glib::SignalHandlerId; 2] {
        let changed = std::rc::Rc::new(changed);
        let properties = changed.clone();
        [
            self.proxy
                .connect_local("g-properties-changed", false, move |_| {
                    properties();
                    None
                }),
            self.proxy
                .connect_notify_local(Some("g-name-owner"), move |_, _| changed()),
        ]
    }

    pub fn disconnect(&self, handlers: [glib::SignalHandlerId; 2]) {
        for handler in handlers {
            self.proxy.disconnect(handler);
        }
    }

    /// Whether the daemon this client reached still runs; it exits when idle.
    pub fn alive(&self) -> bool {
        self.proxy.name_owner().as_deref() == Some(&self.daemon_owner)
    }

    pub fn state(&self) -> Result<State, glib::Error> {
        if !self.alive() {
            return Err(glib::Error::new(
                gio::IOErrorEnum::NotConnected,
                "BigNetScreen disconnected",
            ));
        }
        let property = |name| self.proxy.cached_property(name).ok_or_else(invalid_reply);
        let receivers = property("Receivers")?;
        if receivers.type_() != Vec::<ReceiverWire>::static_variant_type().as_ref()
            || receivers.n_children() > 128
        {
            return Err(invalid_reply());
        }
        let receivers = receivers
            .get::<Vec<ReceiverWire>>()
            .ok_or_else(invalid_reply)?
            .into_iter()
            .filter(|v| {
                v.4 && matches!(v.2.as_str(), "chromecast" | "dlna")
                    && v.3.parse::<std::net::IpAddr>().is_ok()
            })
            .map(|v| Receiver {
                id: v.0,
                name: v.1,
                protocol: v.2,
                state: v.5,
            })
            .collect();
        let (status, _, _, mut detail) = property("Status")?
            .get::<(String, u32, String, String)>()
            .ok_or_else(invalid_reply)?;
        let (session_id, owner, receiver_id, volume, muted, can_volume, can_mute) =
            property("PlayerSession")?
                .get::<SessionWire>()
                .ok_or_else(invalid_reply)?;
        let media = property("Media")?;
        // Validate the tuple without copying the potentially 1000-file queue
        // on each position update. This player only needs current-item fields.
        if media.type_() != MediaWire::static_variant_type().as_ref() {
            return Err(invalid_reply());
        }
        let error: String = media.child_get(11);
        let control_error: String = media.child_get(12);
        if !error.is_empty() {
            detail = error;
        }
        if !control_error.is_empty() {
            detail = control_error;
        }
        let owned = !session_id.is_empty()
            && self.proxy.connection().unique_name().as_deref() == Some(owner.as_str());
        Ok(State {
            receivers,
            status,
            detail,
            session_id,
            receiver_id,
            owned,
            active: media.child_get(0),
            title: media.child_get(1),
            position: media.child_get::<u64>(5) as f64 / 1000.0,
            duration: media.child_get::<u64>(6) as f64 / 1000.0,
            paused: media.child_get(7),
            can_pause: media.child_get(8),
            can_seek: media.child_get(9),
            volume: (can_volume && volume.is_finite() && (0.0..=1.0).contains(&volume))
                .then_some(volume),
            muted: can_mute.then_some(muted),
        })
    }

    async fn call(
        &self,
        method: &str,
        args: Option<&glib::Variant>,
    ) -> Result<glib::Variant, glib::Error> {
        // Address the original daemon connection. A restarted service must
        // never receive commands from a stale player session.
        self.proxy
            .connection()
            .call_future(
                Some(&self.daemon_owner),
                PATH,
                INTERFACE,
                method,
                args,
                None,
                gio::DBusCallFlags::NO_AUTO_START,
                15000,
            )
            .await
    }

    pub async fn rescan(&self) -> Result<(), glib::Error> {
        self.call("Rescan", None).await.map(|_| ())
    }

    /// Keeps the daemon's discovery running while a picker is open (it
    /// exits after a few idle minutes otherwise).
    pub async fn keep_alive(&self) -> Result<(), glib::Error> {
        self.call("KeepAlive", None).await.map(|_| ())
    }

    pub async fn start(
        &self,
        receiver: &str,
        paths: &[String],
        start: PlaybackStart,
    ) -> Result<String, glib::Error> {
        if paths.is_empty() || paths.len() > 1000 {
            return Err(glib::Error::new(
                gio::IOErrorEnum::InvalidArgument,
                "Select between 1 and 1000 files",
            ));
        }
        let reply = self
            .call(
                "StartPlayer",
                Some(&(receiver, paths, start.wire()).to_variant()),
            )
            .await?;
        reply
            .get::<(String,)>()
            .map(|v| v.0)
            .ok_or_else(invalid_reply)
    }

    /// Play an HTTP(S) stream (online video, radio) on `receiver`.
    pub async fn start_url(
        &self,
        receiver: &str,
        uri: &str,
        content_type: &str,
        title: &str,
        audio: bool,
        start: PlaybackStart,
    ) -> Result<String, glib::Error> {
        let reply = self
            .call(
                "StartPlayerUrl",
                Some(&(receiver, uri, content_type, title, audio, start.wire()).to_variant()),
            )
            .await?;
        reply
            .get::<(String,)>()
            .map(|v| v.0)
            .ok_or_else(invalid_reply)
    }

    pub async fn control(&self, id: &str, command: &str, value: f64) -> Result<(), glib::Error> {
        self.call(
            "ControlPlayer",
            Some(&(id, command, value, "").to_variant()),
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires BIG_MEDIA_TEST_BNS_DAEMON and an isolated activation bus"]
    fn daemon_exposes_player_contract() {
        let executable = std::env::var("BIG_MEDIA_TEST_BNS_DAEMON").unwrap();
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                context.block_on(async {
                    let bus = gio::DBusProxy::for_bus_future(
                        gio::BusType::Session,
                        gio::DBusProxyFlags::DO_NOT_AUTO_START,
                        None,
                        "org.freedesktop.DBus",
                        "/org/freedesktop/DBus",
                        "org.freedesktop.DBus",
                    )
                    .await
                    .unwrap();
                    let has_owner = || {
                        bus.call_future(
                            "NameHasOwner",
                            Some(&(SERVICE,).to_variant()),
                            gio::DBusCallFlags::NO_AUTO_START,
                            3000,
                        )
                    };
                    assert_eq!(has_owner().await.unwrap().get::<(bool,)>(), Some((false,)));
                    assert!(Client::available().await.unwrap());
                    assert_eq!(
                        has_owner().await.unwrap().get::<(bool,)>(),
                        Some((false,)),
                        "checking installation must not activate discovery"
                    );
                    let client = Client::connect().await.unwrap();
                    let (pid,) = bus
                        .call_future(
                            "GetConnectionUnixProcessID",
                            Some(&(client.daemon_owner.as_str(),).to_variant()),
                            gio::DBusCallFlags::NO_AUTO_START,
                            3000,
                        )
                        .await
                        .unwrap()
                        .get::<(u32,)>()
                        .unwrap();
                    assert_eq!(
                        std::fs::read_link(format!("/proc/{pid}/exe")).unwrap(),
                        std::fs::canonicalize(executable).unwrap(),
                        "D-Bus must activate the tested build"
                    );
                    let state = client.state().unwrap();
                    assert!(!state.active && !state.owned);
                    assert!(state.receivers.is_empty());
                    assert!(state.session_id.is_empty());
                    assert!(
                        client
                            .start("missing", &[], PlaybackStart::default())
                            .await
                            .is_err()
                    );
                    assert!(client.control("stale-session", "pause", 0.0).await.is_err());
                    assert!(
                        client
                            .control("stale-session", "volume", f64::NAN)
                            .await
                            .is_err()
                    );
                    assert!(client.state().unwrap().session_id.is_empty());
                });
            })
            .unwrap();
    }
}
