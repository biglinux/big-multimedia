use super::metadata::build_metadata;
use super::types::{
    CommandCallback, MprisCommand as C, PlaybackStatus, PlayerState, StateCallback, micros, rate,
    volume,
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
        "LoopStatus" => Some(
            (match s.loop_status.as_str() {
                "Track" => "Track",
                "Playlist" => "Playlist",
                _ => "None",
            })
            .to_variant(),
        ),
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
fn method_command(
    method: &str,
    params: &Variant,
    state: &PlayerState,
) -> Result<Option<C>, &'static str> {
    let empty = || {
        if params.is_type(glib::VariantTy::UNIT) {
            Ok(())
        } else {
            Err("InvalidArgs")
        }
    };
    let command = match method {
        "Play" => {
            empty()?;
            state.can_play.then_some(C::Play)
        }
        "Pause" => {
            empty()?;
            state.can_pause.then_some(C::Pause)
        }
        "PlayPause" => {
            empty()?;
            if state.status == PlaybackStatus::Playing {
                state.can_pause.then_some(C::Pause)
            } else {
                state.can_play.then_some(C::Play)
            }
        }
        "Stop" => {
            empty()?;
            Some(C::Stop)
        }
        "Next" => {
            empty()?;
            state.can_next.then_some(C::Next)
        }
        "Previous" => {
            empty()?;
            state.can_prev.then_some(C::Previous)
        }
        "Seek" => {
            let (offset_us,) = params.get::<(i64,)>().ok_or("InvalidArgs")?;
            if !state.can_seek {
                None
            } else {
                state.track_id.as_ref().map(|id| C::Seek {
                    track_id: id.clone(),
                    offset_us,
                })
            }
        }
        "SetPosition" => {
            let (track, position_us) = params.get::<(ObjectPath, i64)>().ok_or("InvalidArgs")?;
            let id: &str = &track;
            let duration = micros(state.duration_secs);
            if state.can_seek
                && state.track_id.as_deref() == Some(id)
                && position_us >= 0
                && (duration == 0 || position_us <= duration)
            {
                Some(C::SetPosition {
                    track_id: id.to_owned(),
                    position_us,
                })
            } else {
                None
            }
        }
        "OpenUri" => {
            let (uri,) = params.get::<(String,)>().ok_or("InvalidArgs")?;
            let Some(scheme) = big_os_kit::url::uri_scheme(&uri) else {
                return Err("InvalidArgs");
            };
            if !["file", "http", "https"]
                .iter()
                .any(|s| scheme.eq_ignore_ascii_case(s))
            {
                return Err("NotSupported");
            }
            Some(C::OpenUri(uri))
        }
        _ => return Err("UnknownMethod"),
    };
    Ok(command)
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
            .filter(|s| matches!(s.as_str(), "None" | "Track" | "Playlist"))
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
    #[test]
    fn obsolete_seek_is_ignored_and_unknown_method_is_error() {
        let state = PlayerState {
            track_id: Some("/br/com/biglinux/Track/2".into()),
            can_seek: true,
            duration_secs: 10.0,
            ..Default::default()
        };
        let args = (
            ObjectPath::try_from("/br/com/biglinux/Track/1").unwrap(),
            2_i64,
        )
            .to_variant();
        assert_eq!(method_command("SetPosition", &args, &state), Ok(None));
        assert_eq!(
            method_command("Bogus", &().to_variant(), &state),
            Err("UnknownMethod")
        );
    }
    #[test]
    fn position_with_unknown_duration_and_intrinsic_control_are_correct() {
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
