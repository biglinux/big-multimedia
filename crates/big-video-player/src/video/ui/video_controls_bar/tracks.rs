use super::*;
use gtk::gio;

impl VideoControlsBar {
    /// Build the subtitle + audio-track popover menus from the mpv track list.
    /// Each entry installs an action that selects the track via the live mpv
    /// backend (read from the shared slot at click time).
    pub(super) fn populate_track_menus(&self, tracks: &[TrackInfo]) {
        let sub_menu = gio::Menu::new();
        let sub_actions = gio::SimpleActionGroup::new();
        sub_menu.append(
            Some(&i18n("Disable Subtitles")),
            Some("sub.select--disabled"),
        );
        install_track_action(&sub_actions, "select--disabled", &self.mpv, -1, true);
        for track in tracks.iter().filter(|t| t.track_type == "sub") {
            let action = format!("select-{}", track.id);
            install_track_action(&sub_actions, &action, &self.mpv, track.id, true);
            sub_menu.append(Some(&track_label(track)), Some(&format!("sub.{action}")));
        }
        self.subtitle_menu_button.set_menu_model(Some(&sub_menu));
        if let Some(popover) = self.subtitle_menu_button.popover() {
            popover.insert_action_group("sub", Some(&sub_actions));
        }

        let audio_menu = gio::Menu::new();
        let audio_actions = gio::SimpleActionGroup::new();
        for track in tracks.iter().filter(|t| t.track_type == "audio") {
            let action = format!("select-{}", track.id);
            install_track_action(&audio_actions, &action, &self.mpv, track.id, false);
            audio_menu.append(Some(&track_label(track)), Some(&format!("audio.{action}")));
        }
        self.audio_track_menu_button
            .set_menu_model(Some(&audio_menu));
        if let Some(popover) = self.audio_track_menu_button.popover() {
            popover.insert_action_group("audio", Some(&audio_actions));
        }
    }
}

/// Install a stateless action that selects a subtitle / audio track on the live
/// mpv backend. `id == -1` disables the subtitle track.
fn install_track_action(
    group: &gio::SimpleActionGroup,
    name: &str,
    mpv: &MpvSlot,
    id: i64,
    is_subtitle: bool,
) {
    let mpv = mpv.clone();
    big_app_kit::desktop::install_action(group, name, move || {
        if let Some(mpv) = mpv.borrow().as_ref() {
            if is_subtitle {
                mpv.set_subtitle_track(id);
            } else {
                mpv.set_audio_track(id);
            }
        }
    });
}

/// Human-readable track label (id + title + lang + codec).
fn track_label(track: &TrackInfo) -> String {
    big_media_components::track_labels::mpv_track_label(
        track.id,
        &track.title,
        &track.lang,
        &track.codec,
        track.external,
        big_media_components::track_labels::TrackLabelText {
            track: &i18n("Track"),
            external: &i18n("external"),
        },
    )
}
