use super::*;
use crate::settings::keys;
use big_media_components::player_chrome::{
    build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group,
};
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::hamburger_menu::{set_flat_action_popover, BigMenuActionItem};

#[cfg(test)]
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
    pub(super) export_menu_button: gtk::MenuButton,
    pub(super) subtitle_menu_button: gtk::MenuButton,
    pub(super) audio_track_menu_button: gtk::MenuButton,
    pub(super) fullscreen_button: gtk::Button,
}

#[derive(Clone, Copy)]
enum ButtonZone {
    Left,
    Center,
    Right,
}

impl ButtonLayoutWidgets {
    pub(crate) fn apply_settings(&self, settings: &Settings) {
        let mut moved = Vec::new();
        self.apply_zone(
            ButtonZone::Left,
            &settings.get_string(keys::UI_ZONE_BOTTOM_LEFT),
            &mut moved,
        );
        self.apply_zone(
            ButtonZone::Center,
            &settings.get_string(keys::UI_ZONE_BOTTOM_CENTER),
            &mut moved,
        );
        self.apply_zone(
            ButtonZone::Right,
            &settings.get_string(keys::UI_ZONE_BOTTOM_RIGHT),
            &mut moved,
        );
        self.append_missing_defaults(&mut moved);
        self.apply_visibility(settings);
    }

    fn apply_zone(&self, zone: ButtonZone, csv: &str, moved: &mut Vec<&'static str>) {
        for name in csv
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let name = control_name(name);
            if name.is_empty() || moved.contains(&name) {
                continue;
            }
            if self.move_control(zone, name) {
                moved.push(name);
            }
        }
    }

    fn append_missing_defaults(&self, moved: &mut Vec<&'static str>) {
        for (name, zone) in [
            ("volume", ButtonZone::Left),
            ("speed", ButtonZone::Left),
            ("voice", ButtonZone::Left),
            ("prev", ButtonZone::Center),
            ("play", ButtonZone::Center),
            ("next", ButtonZone::Center),
            ("edit-export", ButtonZone::Right),
            ("subtitle", ButtonZone::Right),
            ("audio-track", ButtonZone::Right),
            ("fullscreen", ButtonZone::Right),
        ] {
            if !moved.contains(&name) && self.move_control(zone, name) {
                moved.push(name);
            }
        }
    }

    fn move_control(&self, zone: ButtonZone, name: &str) -> bool {
        let Some(widget) = self.control_widget(name) else {
            return false;
        };
        let target = self.zone_box(zone);
        if let Some(parent) = widget.parent() {
            if parent == target.clone().upcast::<gtk::Widget>() {
                return true;
            }
            widget.unparent();
        }
        target.append(&widget);
        true
    }

    fn apply_visibility(&self, settings: &Settings) {
        self.volume_box
            .set_visible(settings.get_bool(keys::UI_SHOW_VOLUME));
        self.speed_box
            .set_visible(settings.get_bool(keys::UI_SHOW_SPEED));
        self.voice_box.set_visible(
            settings.get_bool(keys::UI_SHOW_VOICE)
                && crate::shared::audio_effects::has_noise_reduction_backend(),
        );
        self.previous_button
            .set_visible(settings.get_bool(keys::UI_SHOW_PREV));
        self.play_button
            .set_visible(settings.get_bool(keys::UI_SHOW_PLAY));
        self.next_button
            .set_visible(settings.get_bool(keys::UI_SHOW_NEXT));
        self.subtitle_menu_button
            .set_visible(settings.get_bool(keys::UI_SHOW_SUBTITLE));
        self.audio_track_menu_button
            .set_visible(settings.get_bool(keys::UI_SHOW_AUDIO_TRACK));
        self.fullscreen_button
            .set_visible(settings.get_bool(keys::UI_SHOW_FULLSCREEN));
    }

    fn control_widget(&self, name: &str) -> Option<gtk::Widget> {
        match control_name(name) {
            "volume" => Some(self.volume_box.clone().upcast()),
            "speed" => Some(self.speed_box.clone().upcast()),
            "voice" => Some(self.voice_box.clone().upcast()),
            "prev" => Some(self.previous_button.clone().upcast()),
            "play" => Some(self.play_button.clone().upcast()),
            "next" => Some(self.next_button.clone().upcast()),
            "edit-export" => Some(self.export_menu_button.clone().upcast()),
            "subtitle" => Some(self.subtitle_menu_button.clone().upcast()),
            "audio-track" => Some(self.audio_track_menu_button.clone().upcast()),
            "fullscreen" => Some(self.fullscreen_button.clone().upcast()),
            _ => None,
        }
    }

    fn zone_box(&self, zone: ButtonZone) -> &gtk::Box {
        match zone {
            ButtonZone::Left => &self.start_zone,
            ButtonZone::Center => &self.center_zone,
            ButtonZone::Right => &self.end_zone,
        }
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
pub(super) fn build_start_box(
    settings: &Settings,
    mpv: &MpvSlot,
) -> (gtk::Box, gtk::Box, gtk::Box, gtk::Box, Vec<gtk::Popover>) {
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
    {
        // Capture the button weakly: the scale's handler must not keep the
        // button alive (mutual cycle → leak on close).
        let mpv = mpv.clone();
        let volume_button_weak = volume_button.downgrade();
        // Weak label too: the popover can outlive teardown ordering — any
        // strong sibling capture here gets pinned with it.
        let volume_label_weak = volume_label.downgrade();
        volume_scale.connect_value_changed(move |scale| {
            let value = scale.value();
            if let Some(volume_label) = volume_label_weak.upgrade() {
                volume_label.set_label(&format!("{value:.0}"));
            }
            if let Some(volume_button) = volume_button_weak.upgrade() {
                volume_button.set_icon_name(volume_icon(value));
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

    (
        start_box,
        volume_box,
        speed_box,
        voice_box,
        vec![volume_popover, speed_popover, voice_popover],
    )
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

/// END box: edit-export MenuButton · subtitle MenuButton · audio-track
/// MenuButton · fullscreen.
pub(super) fn build_end_box() -> (
    gtk::Box,
    gtk::MenuButton,
    gtk::MenuButton,
    gtk::MenuButton,
    gtk::Button,
) {
    let end_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();

    // Edit & Export: cut / extract audio / convert — moved out of the hamburger
    // menu to a footer button next to the subtitle control. Items target the same
    // `win.*` window actions, resolved against the player window's action group.
    let export_menu_button =
        tooltip::icon_menu_button("edit-cut-symbolic", &i18n("Edit & Export"), &["osd-btn"]);
    export_menu_button.set_direction(gtk::ArrowType::Up);
    set_flat_action_popover(&export_menu_button, &export_menu_items());

    let subtitle_menu_button = tooltip::icon_menu_button(
        "media-view-subtitles-symbolic",
        &i18n("Subtitles"),
        &["osd-btn"],
    );
    subtitle_menu_button.set_direction(gtk::ArrowType::Up);

    let audio_track_menu_button = tooltip::icon_menu_button(
        "audio-x-generic-symbolic",
        &i18n("Audio Track"),
        &["osd-btn"],
    );
    audio_track_menu_button.set_direction(gtk::ArrowType::Up);

    let fullscreen_button = osd_button("view-fullscreen-symbolic", &i18n("Fullscreen"));

    end_box.append(&export_menu_button);
    end_box.append(&subtitle_menu_button);
    end_box.append(&audio_track_menu_button);
    end_box.append(&fullscreen_button);
    (
        end_box,
        export_menu_button,
        subtitle_menu_button,
        audio_track_menu_button,
        fullscreen_button,
    )
}

/// The Edit & Export menu (cut · extract audio · convert), targeting the
/// player window's `win.*` actions. Keep it flat so AT-SPI exposes named rows
/// instead of nested "category/item" structures.
fn export_menu_items() -> Vec<BigMenuActionItem> {
    vec![
        BigMenuActionItem::new(i18n("Cut Video…"), "win.trim"),
        BigMenuActionItem::new(i18n("Extract Audio as MP3"), "win.extract-audio-mp3"),
        BigMenuActionItem::new(i18n("Extract Audio as M4A"), "win.extract-audio-m4a"),
        BigMenuActionItem::new(i18n("Extract Audio as FLAC"), "win.extract-audio-flac"),
        BigMenuActionItem::new(i18n("Convert to MP4"), "win.convert-mp4"),
        BigMenuActionItem::new(i18n("Convert to MKV"), "win.convert-mkv"),
        BigMenuActionItem::new(i18n("Convert to WebM"), "win.convert-webm"),
    ]
}

#[cfg(test)]
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
fn volume_icon(value: f64) -> &'static str {
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
    fn edit_export_menu_is_flat() {
        let menu = build_export_menu_model();

        assert_eq!(
            menu_action_names(&menu),
            [
                "win.trim",
                "win.extract-audio-mp3",
                "win.extract-audio-m4a",
                "win.extract-audio-flac",
                "win.convert-mp4",
                "win.convert-mkv",
                "win.convert-webm",
            ]
        );
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }
}
