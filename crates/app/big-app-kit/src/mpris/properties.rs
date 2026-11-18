use super::metadata::build_metadata;
use super::types::{
    CommandCallback, LoopStatus, MprisCommand as C, PlaybackStatus, PlayerState, StateCallback,
    micros, rate, volume,
};
use glib::Variant;
use glib::variant::{ObjectPath, ToVariant};
use gtk::{gio, glib};

pub(super) const CHANGING: &[&str] = &[
    "PlaybackStatus",
    "Metadata",
    "Volume",
    "Rate",
    "LoopStatus",
    "Shuffle",
    "CanPlay",
    "CanPause",
    "CanSeek",
    "CanGoNext",
    "CanGoPrevious",
];
// CanControl is intrinsic to this service; Position must not emit PropertiesChanged.
pub(super) fn player_property(prop: &str, s: &PlayerState) -> Option<Variant> {
    match prop {
        "PlaybackStatus" => Some(s.status.as_str().to_variant()),
        "Metadata" => Some(build_metadata(s)),
        "Position" => Some(micros(s.position_secs).to_variant()), // unknown duration is not zero position
        "Volume" => Some(volume(s.volume_pct).to_variant()),
        "Rate" => Some(rate(s.rate).to_variant()),
        "MinimumRate" => Some(0.25_f64.to_variant()),
        "MaximumRate" => Some(4.0_f64.to_variant()),
        "CanControl" => Some(true.to_variant()),
        "CanPlay" => Some(s.can_play.to_variant()),
        "CanPause" => Some(s.can_pause.to_variant()),
        "CanSeek" => Some(s.can_seek.to_variant()),
        "CanGoNext" => Some(s.can_next.to_variant()),
        "CanGoPrevious" => Some(s.can_prev.to_variant()),
        "LoopStatus" => Some(s.loop_status.as_str().to_variant()),
        "Shuffle" => Some(s.shuffle.to_variant()),
        _ => None,
    }
}
pub(super) fn get_player_property(prop: &str, state: &StateCallback) -> Option<Variant> {
    player_property(prop, &state())
}
pub(super) fn get_root_property(prop: &str, desktop: &str) -> Option<Variant> {
    match prop {
        "Identity" => Some(
            (if desktop.contains("AudioPlayer") {
                "Big Audio Player"
            } else if desktop.contains("VideoPlayer") {
                "Big Video Player"
            } else {
                "Big Media Player"
            })
            .to_variant(),
        ),
        "DesktopEntry" => Some(desktop.to_variant()),
        "CanQuit" | "CanRaise" => Some(true.to_variant()),
        "HasTrackList" | "Fullscreen" | "CanSetFullscreen" => Some(false.to_variant()),
        "SupportedUriSchemes" => Some(vec!["file", "http", "https"].to_variant()),
        "SupportedMimeTypes" if desktop.contains("VideoPlayer") => Some(
            vec![
                "video/mp4",
                "video/x-matroska",
                "video/webm",
                "video/quicktime",
                "video/x-msvideo",
                "video/mpeg",
            ]
            .to_variant(),
        ),
        "SupportedMimeTypes" => Some(
            vec![
                "audio/mpeg",
                "audio/flac",
                "audio/ogg",
                "audio/opus",
                "audio/mp4",
                "audio/wav",
                "audio/aac",
            ]
            .to_variant(),
        ),
        _ => None,
    }
}
/// Decodes a player method call. `Ok(None)` is a valid call the state ignores,
/// such as a seek for a track other than the current one.
fn method_command(
    method: &str,
    params: &Variant,
    state: &PlayerState,
) -> Result<Option<C>, &'static str> {
    let playing = state.status == PlaybackStatus::Playing;
    let without_args = match method {
        "Play" => Some(state.can_play.then_some(C::Play)),
        "Pause" => Some(state.can_pause.then_some(C::Pause)),
        "PlayPause" if playing => Some(state.can_pause.then_some(C::Pause)),
        "PlayPause" => Some(state.can_play.then_some(C::Play)),
        "Stop" => Some(Some(C::Stop)),
        "Next" => Some(state.can_next.then_some(C::Next)),
        "Previous" => Some(state.can_prev.then_some(C::Previous)),
        _ => None,
    };
    if let Some(command) = without_args {
        return if params.is_type(glib::VariantTy::UNIT) {
            Ok(command)
        } else {
            Err("InvalidArgs")
        };
    }
    match method {
        "Seek" => {
            let (offset_us,) = params.get::<(i64,)>().ok_or("InvalidArgs")?;
            let track_id = state.track_id.clone().filter(|_| state.can_seek);
            Ok(track_id.map(|track_id| C::Seek {
                track_id,
                offset_us,
            }))
        }
        "SetPosition" => {
            let (track, position_us) = params.get::<(ObjectPath, i64)>().ok_or("InvalidArgs")?;
            let duration = micros(state.duration_secs);
            let valid = state.can_seek
                && state.track_id.as_deref() == Some(track.as_str())
                && position_us >= 0
                && (duration == 0 || position_us <= duration);
            Ok(valid.then(|| C::SetPosition {
                track_id: track.as_str().to_owned(),
                position_us,
            }))
        }
        "OpenUri" => {
            let (uri,) = params.get::<(String,)>().ok_or("InvalidArgs")?;
            let scheme = big_os_kit::url::uri_scheme(&uri).ok_or("InvalidArgs")?;
            if scheme.eq_ignore_ascii_case("file") {
                let path = big_os_kit::url::file_uri_path(&uri)
                    .and_then(|path| path.into_os_string().into_string().ok())
                    .ok_or("InvalidArgs")?;
                Ok(Some(C::OpenUri(path)))
            } else if ["http", "https"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
            {
                Ok(Some(C::OpenUri(uri)))
            } else {
                Err("NotSupported")
            }
        }
        _ => Err("UnknownMethod"),
    }
}
pub(super) fn handle_player_method(
    method: &str,
    params: Variant,
    invocation: gio::DBusMethodInvocation,
    command: &CommandCallback,
    state: &StateCallback,
) {
    match method_command(method, &params, &state()) {
        Ok(value) => {
            if let Some(value) = value {
                command(value);
            }
            invocation.return_value(None);
        }
        Err(kind) => invocation.return_dbus_error(
            &format!("org.freedesktop.DBus.Error.{kind}"),
            "MPRIS request rejected",
        ),
    }
}
pub(super) fn set_player_property(
    prop: &str,
    value: &Variant,
    command: &CommandCallback,
    state: &StateCallback,
) -> bool {
    let request = match prop {
        "Volume" => value
            .get::<f64>()
            .filter(|x| x.is_finite())
            .map(|x| C::SetVolume(x.max(0.0))),
        "Rate" => value.get::<f64>().filter(|x| x.is_finite()).and_then(|x| {
            if x == 0.0 {
                state().can_pause.then_some(C::Pause)
            } else if (0.25..=4.0).contains(&x) {
                Some(C::SetRate(x))
            } else {
                None
            }
        }),
        "LoopStatus" => value
            .get::<String>()
            .and_then(|s| LoopStatus::from_bus(&s))
            .map(C::SetLoopStatus),
        "Shuffle" => value.get::<bool>().map(C::SetShuffle),
        _ => None,
    };
    if let Some(request) = request {
        command(request);
        true
    } else {
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Oracle: MPRIS positions are non-negative microseconds in an i64.
    #[test]
    fn seconds_become_whole_microseconds() {
        assert_eq!(micros(1.5), 1_500_000);
        assert_eq!(micros(0.000_000_6), 1);
        assert_eq!(micros(-3.0), 0);
        assert_eq!(micros(f64::NAN), 0);
        assert_eq!(micros(f64::MAX), i64::MAX);
    }
    #[test]
    fn stale_set_position_is_ignored() {
        let state = PlayerState {
            track_id: Some("/br/com/biglinux/Track/2".into()),
            can_seek: true,
            duration_secs: 10.0,
            ..Default::default()
        };
        let args = |track: &str| (ObjectPath::try_from(track).unwrap(), 2_i64).to_variant();
        assert_eq!(
            method_command("SetPosition", &args("/br/com/biglinux/Track/1"), &state),
            Ok(None)
        );
        assert!(matches!(
            method_command("SetPosition", &args("/br/com/biglinux/Track/2"), &state),
            Ok(Some(C::SetPosition { position_us: 2, .. }))
        ));
    }
    /// Oracle: the MPRIS spec. A negative volume means 0.0, a rate of 0.0 means
    /// Pause, and a rate outside MinimumRate..MaximumRate (0.25..4.0) is refused.
    #[test]
    fn volume_and_rate_writes_are_bounded() {
        let cases = [
            ("Volume", f64::NAN, true, None),
            ("Volume", f64::INFINITY, true, None),
            ("Volume", f64::NEG_INFINITY, true, None),
            ("Volume", -0.5, true, Some(C::SetVolume(0.0))),
            ("Volume", 0.5, true, Some(C::SetVolume(0.5))),
            ("Rate", f64::NAN, true, None),
            ("Rate", f64::INFINITY, true, None),
            ("Rate", 0.1, true, None),
            ("Rate", 4.5, true, None),
            ("Rate", -1.0, true, None),
            ("Rate", 0.25, true, Some(C::SetRate(0.25))),
            ("Rate", 4.0, true, Some(C::SetRate(4.0))),
            ("Rate", 0.0, true, Some(C::Pause)),
            ("Rate", 0.0, false, None),
        ];
        for (property, value, can_pause, expected) in cases {
            let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let sink = std::sync::Arc::clone(&sent);
            let command: CommandCallback = Box::new(move |request| {
                sink.lock().unwrap().push(request);
            });
            let state: StateCallback = Box::new(move || PlayerState {
                can_pause,
                ..Default::default()
            });
            let accepted = set_player_property(property, &value.to_variant(), &command, &state);
            assert_eq!(
                *sent.lock().unwrap(),
                Vec::from_iter(expected.clone()),
                "{property} {value}"
            );
            assert_eq!(accepted, expected.is_some(), "{property} {value}");
        }
    }

    /// Oracle: the MPRIS spec's three LoopStatus values, case-sensitive.
    #[test]
    fn loop_status_writes_take_only_mpris_values() {
        let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&sent);
        let command: CommandCallback = Box::new(move |request| {
            sink.lock().unwrap().push(request);
        });
        let state: StateCallback = Box::new(PlayerState::default);
        for value in ["None", "Track", "Playlist", "track", ""] {
            set_player_property("LoopStatus", &value.to_variant(), &command, &state);
        }
        assert_eq!(
            *sent.lock().unwrap(),
            [
                C::SetLoopStatus(LoopStatus::None),
                C::SetLoopStatus(LoopStatus::Track),
                C::SetLoopStatus(LoopStatus::Playlist),
            ]
        );
        for status in [LoopStatus::None, LoopStatus::Track, LoopStatus::Playlist] {
            let shown = PlayerState {
                loop_status: status,
                ..Default::default()
            };
            assert_eq!(
                player_property("LoopStatus", &shown)
                    .unwrap()
                    .get::<String>(),
                Some(status.as_str().to_owned())
            );
        }
    }

    #[test]
    fn open_uri_resolves_local_files() {
        let state = PlayerState::default();
        let open = |uri: &str| method_command("OpenUri", &(uri,).to_variant(), &state);
        for (uri, expected) in [
            (
                "file:///home/u/a%20b.mp3",
                Ok(Some(C::OpenUri("/home/u/a b.mp3".into()))),
            ),
            (
                "file://localhost/x.mp3",
                Ok(Some(C::OpenUri("/x.mp3".into()))),
            ),
            ("file://server/x.mp3", Err("InvalidArgs")),
            ("file:///x%FF.mp3", Err("InvalidArgs")),
            (
                "https://e.example/a.mp3",
                Ok(Some(C::OpenUri("https://e.example/a.mp3".into()))),
            ),
            ("smb://server/x.mp3", Err("NotSupported")),
            ("", Err("InvalidArgs")),
        ] {
            assert_eq!(open(uri), expected, "{uri}");
        }
    }

    #[test]
    fn unknown_method_is_an_error() {
        let state = PlayerState::default();
        assert_eq!(
            method_command("Bogus", &().to_variant(), &state),
            Err("UnknownMethod")
        );
        assert_eq!(
            method_command("Play", &(1_i64,).to_variant(), &state),
            Err("InvalidArgs")
        );
    }
    #[test]
    fn position_reported_without_duration() {
        let state = PlayerState {
            position_secs: 12.5,
            ..Default::default()
        };
        assert_eq!(
            player_property("Position", &state).unwrap().get::<i64>(),
            Some(12_500_000)
        );
        assert!(!CHANGING.contains(&"Position"));
        assert!(!CHANGING.contains(&"CanControl"));
    }
}
