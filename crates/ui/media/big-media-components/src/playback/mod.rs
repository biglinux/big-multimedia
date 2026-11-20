//! Playback controls and queue view.

use relm4::gtk;
use relm4::gtk::prelude::*;

pub mod audio_transport;
pub mod cut_timeline;
pub mod queue;

/// Show on a play/pause `button` the action a click takes: `play_label` with
/// the play icon while `paused`, `pause_label` with the pause icon otherwise.
/// The label is both the tooltip and the accessible name.
pub fn set_play_pause(button: &gtk::Button, paused: bool, play_label: &str, pause_label: &str) {
    let (icon, action) = if paused {
        ("media-playback-start-symbolic", play_label)
    } else {
        ("media-playback-pause-symbolic", pause_label)
    };
    button.set_icon_name(icon);
    button.set_tooltip_text(Some(action));
    button.update_property(&[gtk::accessible::Property::Label(action)]);
}
