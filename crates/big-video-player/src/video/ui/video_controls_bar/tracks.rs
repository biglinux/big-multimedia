use super::*;
use big_relm4_components::layout::hamburger_menu::{
    populate_flat_action_popover, BigMenuActionItem,
};
use gtk::gio;

impl VideoControlsBar {
    /// Build the subtitle + audio-track popover menus from the mpv track list.
    /// Each entry installs an action that selects the track via the live mpv
    /// backend (read from the shared slot at click time).
    pub(super) fn populate_track_menus(&self, tracks: &[TrackInfo]) {
        let sub_actions = gio::SimpleActionGroup::new();
        let mut subtitle_items = vec![BigMenuActionItem::new(
            i18n("Disable Subtitles"),
            "sub.select--disabled",
        )];
        install_track_action(&sub_actions, "select--disabled", &self.mpv, -1, true);
        for track in tracks.iter().filter(|t| t.track_type == "sub") {
            let action = format!("select-{}", track.id);
            install_track_action(&sub_actions, &action, &self.mpv, track.id, true);
            subtitle_items.push(BigMenuActionItem::new(
                track_label(track),
                format!("sub.{action}"),
            ));
        }
        populate_flat_action_popover(
            &self.subtitle_menu_popover,
            &self.subtitle_menu_button,
            &subtitle_items,
        );
        self.subtitle_menu_popover
            .insert_action_group("sub", Some(&sub_actions));

        let audio_actions = gio::SimpleActionGroup::new();
        let mut audio_items = Vec::new();
        for track in tracks.iter().filter(|t| t.track_type == "audio") {
            let action = format!("select-{}", track.id);
            install_track_action(&audio_actions, &action, &self.mpv, track.id, false);
            audio_items.push(BigMenuActionItem::new(
                track_label(track),
                format!("audio.{action}"),
            ));
        }
        populate_flat_action_popover(
            &self.audio_track_menu_popover,
            &self.audio_track_menu_button,
            &audio_items,
        );
        self.audio_track_menu_popover
            .insert_action_group("audio", Some(&audio_actions));
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
