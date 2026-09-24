use std::collections::HashMap;

use super::*;
use crate::settings::keys;
use big_media_components::player_chrome::{
    build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group,
};
use big_relm4_components::feedback::tooltip;
use big_relm4_components::input::zone_layout::{ZoneLayout, ZonePlacement, slot_key};
use big_relm4_components::layout::hamburger_menu::{
    BigMenuActionItem, attach_popover_to_button, build_flat_action_popover,
};
use big_relm4_components::layout::zone_applier::apply_zone_layout;

#[cfg(all(test, not(miri)))]
use big_relm4_components::layout::hamburger_menu::{build_flat_menu_model, menu_action_names};

#[derive(Clone)]
pub(crate) struct ButtonLayoutWidgets {
    pub(super) start_zone: gtk::Box,
    pub(super) center_zone: gtk::Box,
    pub(super) end_zone: gtk::Box,
    pub(super) volume_box: gtk::Box,
    pub(super) speed_box: gtk::Box,
    pub(super) voice_box: gtk::Box,
    pub(super) previous_button: gtk::Button,
    pub(super) play_button: gtk::Button,
    pub(super) next_button: gtk::Button,
    pub(super) export_menu_button: gtk::Button,
    pub(super) export_menu_popover: gtk::Popover,
    pub(super) subtitle_menu_button: gtk::Button,
    pub(super) audio_track_menu_button: gtk::Button,
    pub(super) fullscreen_button: gtk::Button,
}

impl ButtonLayoutWidgets {
    /// Reparent the OSD controls into their bottom-bar zones and apply
    /// per-control visibility from the `ui-zone-*` / `ui-show-*` settings, via
    /// the shared leak-safe [`apply_zone_layout`] primitive.
    ///
    /// The three `ui-zone-bottom-*` CSVs become a
    /// [`ZoneLayout`](big_relm4_components::input::zone_layout::ZoneLayout) (each
    /// token mapped through `control_name`, dropping unknown/legacy-aliased
    /// entries); the applier reparents each control into its zone and appends any
    /// control the CSVs omit to its default fallback slot. Visibility comes from
    /// the `ui-show-*` flags — the voice control additionally gated on a
    /// noise-reduction backend being installed.
    pub(crate) fn apply_settings(&self, settings: &Settings) {
        let start = slot_key("bottom", ZonePlacement::Start);
        let center = slot_key("bottom", ZonePlacement::Center);
        let end = slot_key("bottom", ZonePlacement::End);

        // The three saved CSVs → an ordered ZoneLayout (aliases normalised,
        // unknown ids dropped). Cross-zone dedup + missing-default fallback are
        // the applier's job.
        let mut layout = ZoneLayout::default();
        for (csv_key, slot) in [
            (keys::UI_ZONE_BOTTOM_LEFT, &start),
            (keys::UI_ZONE_BOTTOM_CENTER, &center),
            (keys::UI_ZONE_BOTTOM_RIGHT, &end),
        ] {
            let ids: Vec<String> = settings
                .get_string(csv_key)
                .split(',')
                .map(str::trim)
                .map(control_name)
                .filter(|name| !name.is_empty())
                .map(String::from)
                .collect();
            layout.slots.insert(slot.clone(), ids);
        }

        let controls: HashMap<String, gtk::Widget> = [
            ("volume", self.volume_box.clone().upcast::<gtk::Widget>()),
            ("speed", self.speed_box.clone().upcast()),
            ("voice", self.voice_box.clone().upcast()),
            ("prev", self.previous_button.clone().upcast()),
            ("play", self.play_button.clone().upcast()),
            ("next", self.next_button.clone().upcast()),
            ("edit-export", self.export_menu_button.clone().upcast()),
            ("subtitle", self.subtitle_menu_button.clone().upcast()),
            ("audio-track", self.audio_track_menu_button.clone().upcast()),
            ("fullscreen", self.fullscreen_button.clone().upcast()),
        ]
        .into_iter()
        .map(|(id, widget)| (id.to_owned(), widget))
        .collect();

        let slots = [
            (start.as_str(), &self.start_zone),
            (center.as_str(), &self.center_zone),
            (end.as_str(), &self.end_zone),
        ];

        let fallback = [
            ("volume", start.as_str()),
            ("speed", start.as_str()),
            ("voice", start.as_str()),
            ("prev", center.as_str()),
            ("play", center.as_str()),
            ("next", center.as_str()),
            ("edit-export", end.as_str()),
            ("subtitle", end.as_str()),
            ("audio-track", end.as_str()),
            ("fullscreen", end.as_str()),
        ];

        // Visibility: the `ui-show-*` flags. `edit-export` has no flag (always
        // shown → absent here so the applier leaves it visible). Voice is also
        // gated on a real noise-reduction backend.
        let visibility: HashMap<String, bool> = [
            ("volume", settings.get_bool(keys::UI_SHOW_VOLUME)),
            ("speed", settings.get_bool(keys::UI_SHOW_SPEED)),
            (
                "voice",
                settings.get_bool(keys::UI_SHOW_VOICE)
                    && crate::shared::audio_effects::has_noise_reduction_backend(),
            ),
            ("prev", settings.get_bool(keys::UI_SHOW_PREV)),
            ("play", settings.get_bool(keys::UI_SHOW_PLAY)),
            ("next", settings.get_bool(keys::UI_SHOW_NEXT)),
            ("subtitle", settings.get_bool(keys::UI_SHOW_SUBTITLE)),
            ("audio-track", settings.get_bool(keys::UI_SHOW_AUDIO_TRACK)),
            ("fullscreen", settings.get_bool(keys::UI_SHOW_FULLSCREEN)),
        ]
        .into_iter()
        .map(|(id, visible)| (id.to_owned(), visible))
        .collect();

        apply_zone_layout(&layout, &controls, &slots, &fallback, &visibility);
    }
}

fn control_name(name: &str) -> &'static str {
    match name {
        "volume" => "volume",
        "speed" => "speed",
        "voice" => "voice",
        "prev" | "previous" => "prev",
        "play" => "play",
        "next" => "next",
        "edit-export" | "export" => "edit-export",
        "subtitle" | "subtitles" => "subtitle",
        "audio-track" | "audio" => "audio-track",
        "fullscreen" => "fullscreen",
        _ => "",
    }
}

/// START box: volume / speed / voice hover popovers (shared builders), with the
/// exclusive-hover group wiring from the old `setup_hover_popovers`. Returns the
/// box plus the three hover popovers so the shell's OSD auto-hide can keep the
/// OSD shown while one is open.
/// START box widgets: the reparentable control boxes, the readout value labels
/// the responsive breakpoint hides on a narrow bar, and the hover popovers.
pub(super) struct StartBoxWidgets {
    pub(super) start_box: gtk::Box,
    pub(super) volume_box: gtk::Box,
    pub(super) speed_box: gtk::Box,
    pub(super) voice_box: gtk::Box,
    pub(super) hover_popovers: Vec<gtk::Popover>,
    pub(super) readout_labels: [gtk::Label; 3],
    /// Volume scale + its button, plus the guard that lets the shell mirror an
    /// external mixer change onto the scale without pushing it back to the server.
    pub(super) volume_scale: gtk::Scale,
    pub(super) volume_button: gtk::Button,
    pub(super) suppress_volume: std::rc::Rc<std::cell::Cell<bool>>,
}

pub(super) fn build_start_box(settings: &Settings, mpv: &MpvSlot) -> StartBoxWidgets {
    let start_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(2)
        .valign(gtk::Align::Center)
        .build();

    // Volume.
    let volume_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let volume_button = osd_button("audio-volume-high-symbolic", &i18n("Mute"));
    let volume_label = gtk::Label::builder()
        .label("100")
        .width_chars(3)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    volume_box.append(&volume_button);
    volume_box.append(&volume_label);
    start_box.append(&volume_box);

    // Speed.
    let speed_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let speed_button = osd_button("speedometer-symbolic", &i18n("Speed"));
    let speed_label = gtk::Label::builder()
        .label("1.00x")
        .width_chars(5)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    speed_box.append(&speed_button);
    speed_box.append(&speed_label);
    start_box.append(&speed_box);

    // Voice enhancement.
    let voice_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let voice_button = osd_button(
        "audio-input-microphone-symbolic",
        &i18n("Voice Enhancement"),
    );
    let voice_label = gtk::Label::builder()
        .label("0")
        .width_chars(6)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    voice_box.append(&voice_button);
    voice_box.append(&voice_label);
    start_box.append(&voice_box);

    let maximum_volume = f64::from(settings.get_int(keys::VOLUME_MAX)).max(100.0);
    let initial_volume = settings.get_double(keys::VOLUME) * 100.0;
    let (volume_popover, volume_scale) =
        build_volume_popover(&volume_box, maximum_volume, initial_volume);
    // Set true while mirroring an external mixer change onto the scale, so the
    // handler updates only the readout/icon and does not push the value back to
    // the sound server (or clear mute) — which would fight the external change.
    let suppress_volume = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        // Capture the button weakly: the scale's handler must not keep the
        // button alive (mutual cycle → leak on close).
        let mpv = mpv.clone();
        let volume_button_weak = volume_button.downgrade();
        // Weak label too: the popover can outlive teardown ordering — any
        // strong sibling capture here gets pinned with it.
        let volume_label_weak = volume_label.downgrade();
        let suppress_volume = suppress_volume.clone();
        volume_scale.connect_value_changed(move |scale| {
            let value = scale.value();
            if let Some(volume_label) = volume_label_weak.upgrade() {
                volume_label.set_label(&format!("{value:.0}"));
            }
            if let Some(volume_button) = volume_button_weak.upgrade() {
                volume_button.set_icon_name(volume_icon(value));
            }
            if suppress_volume.get() {
                return;
            }
            if let Some(mpv) = mpv.borrow().as_ref() {
                mpv.set_volume(value);
                mpv.set_mute(false);
            }
        });
    }
    // Mute toggle on the volume button. Uses the handler's own button argument
    // (no self-clone) and a weak scale ref (no mutual cycle).
    {
        let mpv = mpv.clone();
        let scale = volume_scale.downgrade();
        volume_button.connect_clicked(move |button| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                let muted = !mpv.get_mute();
                mpv.set_mute(muted);
                let fallback = scale.upgrade().map_or(0.0, |scale| scale.value());
                button.set_icon_name(if muted {
                    "audio-volume-muted-symbolic"
                } else {
                    volume_icon(fallback)
                });
            }
        });
    }

    let speed_popover = build_speed_popover(&speed_box, &speed_button, &speed_label, {
        let mpv = mpv.clone();
        move |speed| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                mpv.set_speed(speed);
            }
        }
    });

    let stored_noise_reduction_strength = settings.get_double(keys::NOISE_STRENGTH).max(0.0);
    let initial_noise_reduction_percent =
        (stored_noise_reduction_strength * 100.0).clamp(0.0, 100.0);
    let previous_noise_reduction_active =
        Rc::new(std::cell::Cell::new(stored_noise_reduction_strength > 0.0));
    let voice_popover = build_voice_popover(
        &voice_box,
        &voice_button,
        &voice_label,
        initial_noise_reduction_percent,
        {
            let settings = settings.clone();
            let mpv = mpv.clone();
            move |pct| {
                crate::shared::popovers::handle_voice_strength_changed(
                    &settings,
                    mpv.borrow().as_ref().map(Arc::as_ref),
                    pct,
                    &previous_noise_reduction_active,
                );
            }
        },
        {
            let settings = settings.clone();
            // Weak: this closure lives on the popover, which the voice button's
            // handler keeps alive — a strong voice_box capture closes a
            // box→button→popover→box cycle that survives popover unparent.
            let voice_box = voice_box.downgrade();
            move || {
                if let Some(window) = voice_box
                    .upgrade()
                    .and_then(|b| b.root())
                    .and_then(|r| r.downcast::<gtk::Window>().ok())
                {
                    crate::shared::popovers::show_nr_settings_dialog(&settings, &window);
                }
            }
        },
    );

    // Hide the voice control if no noise-reduction backend is installed (old
    // `setup_hover_popovers` did the same).
    if !crate::shared::audio_effects::has_noise_reduction_backend() {
        voice_box.set_visible(false);
    }

    let _hover_group = setup_exclusive_hover_popover_group([
        (&volume_box, &volume_popover),
        (&speed_box, &speed_popover),
        (&voice_box, &voice_popover),
    ]);
    // The hover group keeps the popovers alive via the boxes' controllers; the
    // boxes are owned by `start_box`, so the returned handle can drop here.

    StartBoxWidgets {
        start_box,
        volume_box,
        speed_box,
        voice_box,
        hover_popovers: vec![volume_popover, speed_popover, voice_popover],
        readout_labels: [volume_label, speed_label, voice_label],
        volume_scale,
        volume_button,
        suppress_volume,
    }
}

/// CENTER box: previous · play/pause · next.
pub(super) fn build_center_box() -> (gtk::Box, gtk::Button, gtk::Button, gtk::Button) {
    let center = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();

    let previous_button = osd_button("media-skip-backward-symbolic", &i18n("Previous"));
    let play_button = osd_button("media-playback-start-symbolic", &i18n("Play"));
    play_button.add_css_class("osd-btn-play");
    let next_button = osd_button("media-skip-forward-symbolic", &i18n("Next"));

    center.append(&previous_button);
    center.append(&play_button);
    center.append(&next_button);
    (center, previous_button, play_button, next_button)
}

/// END box: edit-export popover · subtitle popover · audio-track popover · fullscreen.
pub(super) struct EndBoxWidgets {
    pub(super) end_box: gtk::Box,
    pub(super) export_menu_button: gtk::Button,
    pub(super) export_menu_popover: gtk::Popover,
    pub(super) subtitle_menu_button: gtk::Button,
    pub(super) subtitle_menu_popover: gtk::Popover,
    pub(super) audio_track_menu_button: gtk::Button,
    pub(super) audio_track_menu_popover: gtk::Popover,
    pub(super) fullscreen_button: gtk::Button,
}

pub(super) fn build_end_box() -> EndBoxWidgets {
    let end_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    // Edit & Export: cut / extract audio / convert — moved out of the hamburger
    // menu to a footer button next to the subtitle control. Items target the same
    // `win.*` window actions, resolved against the player window's action group.
    // Plain button + popover parented to the button, NOT
    // `build_flat_action_popover_button`: that helper wraps the button in a box
    // and installs a destroy handler on the wrapper that unparents the popover
    // when the wrapper drops — which orphaned this popover so it opened at the
    // window origin (top-left) then crashed.
    let export_menu_button =
        tooltip::icon_button("edit-cut-symbolic", &i18n("Edit & Export"), &["osd-btn"]);
    let export_menu_popover = build_flat_action_popover(&export_menu_button, &export_menu_items());
    setup_end_menu_popover(&export_menu_button, &export_menu_popover);

    let subtitle_menu_button = tooltip::icon_button(
        "media-view-subtitles-symbolic",
        &i18n("Subtitles"),
        &["osd-btn"],
    );
    let subtitle_menu_popover = gtk::Popover::new();
    setup_end_menu_popover(&subtitle_menu_button, &subtitle_menu_popover);

    let audio_track_menu_button = tooltip::icon_button(
        "audio-x-generic-symbolic",
        &i18n("Audio Track"),
        &["osd-btn"],
    );
    let audio_track_menu_popover = gtk::Popover::new();
    setup_end_menu_popover(&audio_track_menu_button, &audio_track_menu_popover);

    let fullscreen_button = osd_button("view-fullscreen-symbolic", &i18n("Fullscreen"));

    end_box.append(&export_menu_button);
    end_box.append(&subtitle_menu_button);
    end_box.append(&audio_track_menu_button);
    end_box.append(&fullscreen_button);
    EndBoxWidgets {
        end_box,
        export_menu_button,
        export_menu_popover,
        subtitle_menu_button,
        subtitle_menu_popover,
        audio_track_menu_button,
        audio_track_menu_popover,
        fullscreen_button,
    }
}

/// Anchor an end-zone menu popover to its button and open it upward, clear of the
/// bottom bar. `set_autohide(false)` matches the start-zone controls: an autohide
/// popover takes a Wayland grab that anchors to the window origin (top-left)
/// instead of the button. The button click toggles it and selecting a row pops it
/// down; the OSD keeps itself shown while it is open (see
/// `VideoControlsBar::popovers`).
fn setup_end_menu_popover(button: &gtk::Button, popover: &gtk::Popover) {
    popover.set_position(gtk::PositionType::Top);
    popover.set_autohide(false);
    attach_popover_to_button(button, popover);
}

/// The Edit & Export menu (cut · extract audio · convert), targeting the
/// player window's `win.*` actions; each hands the file to a converter GUI. Keep it flat so AT-SPI exposes named rows
/// instead of nested "category/item" structures.
fn export_menu_items() -> Vec<BigMenuActionItem> {
    vec![
        BigMenuActionItem::new(i18n("Cut Video…"), "win.trim"),
        BigMenuActionItem::new(i18n("Extract Audio…"), "win.extract-audio"),
        BigMenuActionItem::new(i18n("Convert…"), "win.convert"),
    ]
}

#[cfg(test)]
fn export_menu_action_names() -> Vec<String> {
    export_menu_items()
        .into_iter()
        .map(|menu_item| menu_item.action)
        .collect()
}

#[cfg(all(test, not(miri)))]
fn build_export_menu_model() -> gio::Menu {
    build_flat_menu_model(&export_menu_items())
}

/// OSD icon button with an accessible label.
fn osd_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .valign(gtk::Align::Center)
        .css_classes(["osd-btn"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Volume icon for a 0..max level.
pub(super) fn volume_icon(value: f64) -> &'static str {
    if value <= 0.0 {
        "audio-volume-muted-symbolic"
    } else if value < 34.0 {
        "audio-volume-low-symbolic"
    } else if value < 67.0 {
        "audio-volume-medium-symbolic"
    } else {
        "audio-volume-high-symbolic"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_export_menu_actions_are_flat_window_actions() {
        assert_eq!(
            export_menu_action_names(),
            [
                "win.trim".to_string(),
                "win.extract-audio".to_string(),
                "win.convert".to_string(),
            ]
        );
    }

    #[test]
    fn control_names_accept_canonical_and_legacy_aliases() {
        assert_eq!(control_name("volume"), "volume");
        assert_eq!(control_name("speed"), "speed");
        assert_eq!(control_name("voice"), "voice");
        assert_eq!(control_name("prev"), "prev");
        assert_eq!(control_name("previous"), "prev");
        assert_eq!(control_name("play"), "play");
        assert_eq!(control_name("next"), "next");
        assert_eq!(control_name("edit-export"), "edit-export");
        assert_eq!(control_name("export"), "edit-export");
        assert_eq!(control_name("subtitle"), "subtitle");
        assert_eq!(control_name("subtitles"), "subtitle");
        assert_eq!(control_name("audio-track"), "audio-track");
        assert_eq!(control_name("audio"), "audio-track");
        assert_eq!(control_name("fullscreen"), "fullscreen");
        assert_eq!(control_name("unknown"), "");
    }

    #[test]
    fn volume_icon_uses_muted_low_medium_high_thresholds() {
        assert_eq!(volume_icon(-1.0), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.0), "audio-volume-muted-symbolic");
        assert_eq!(volume_icon(0.1), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(33.9), "audio-volume-low-symbolic");
        assert_eq!(volume_icon(34.0), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(66.9), "audio-volume-medium-symbolic");
        assert_eq!(volume_icon(67.0), "audio-volume-high-symbolic");
        assert_eq!(volume_icon(100.0), "audio-volume-high-symbolic");
    }

    #[cfg(not(miri))]
    #[test]
    fn edit_export_menu_is_flat_gio_menu() {
        let menu = build_export_menu_model();

        assert_eq!(menu_action_names(&menu), export_menu_action_names());
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }
}
