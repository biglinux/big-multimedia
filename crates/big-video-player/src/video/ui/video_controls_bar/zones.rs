use super::*;
use crate::settings::keys;
use big_media_components::player_chrome::{
    build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group,
};

#[derive(Clone)]
pub(crate) struct ButtonLayoutWidgets {
    pub(super) start_zone: gtk::Box,
    pub(super) center_zone: gtk::Box,
    pub(super) end_zone: gtk::Box,
    pub(super) volume_box: gtk::Box,
    pub(super) speed_box: gtk::Box,
    pub(super) voice_box: gtk::Box,
    pub(super) prev_btn: gtk::Button,
    pub(super) play_btn: gtk::Button,
    pub(super) next_btn: gtk::Button,
    pub(super) export_btn: gtk::MenuButton,
    pub(super) subtitle_btn: gtk::MenuButton,
    pub(super) audio_track_btn: gtk::MenuButton,
    pub(super) fullscreen_btn: gtk::Button,
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
        self.prev_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_PREV));
        self.play_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_PLAY));
        self.next_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_NEXT));
        self.subtitle_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_SUBTITLE));
        self.audio_track_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_AUDIO_TRACK));
        self.fullscreen_btn
            .set_visible(settings.get_bool(keys::UI_SHOW_FULLSCREEN));
    }

    fn control_widget(&self, name: &str) -> Option<gtk::Widget> {
        match control_name(name) {
            "volume" => Some(self.volume_box.clone().upcast()),
            "speed" => Some(self.speed_box.clone().upcast()),
            "voice" => Some(self.voice_box.clone().upcast()),
            "prev" => Some(self.prev_btn.clone().upcast()),
            "play" => Some(self.play_btn.clone().upcast()),
            "next" => Some(self.next_btn.clone().upcast()),
            "edit-export" => Some(self.export_btn.clone().upcast()),
            "subtitle" => Some(self.subtitle_btn.clone().upcast()),
            "audio-track" => Some(self.audio_track_btn.clone().upcast()),
            "fullscreen" => Some(self.fullscreen_btn.clone().upcast()),
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
    let volume_btn = osd_button("audio-volume-high-symbolic", &i18n("Mute"));
    let volume_label = gtk::Label::builder()
        .label("100")
        .width_chars(3)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    volume_box.append(&volume_btn);
    volume_box.append(&volume_label);
    start_box.append(&volume_box);

    // Speed.
    let speed_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let speed_btn = osd_button("speedometer-symbolic", &i18n("Speed"));
    let speed_label = gtk::Label::builder()
        .label("1.00x")
        .width_chars(5)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    speed_box.append(&speed_btn);
    speed_box.append(&speed_label);
    start_box.append(&speed_box);

    // Voice enhancement.
    let voice_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    let voice_btn = osd_button(
        "audio-input-microphone-symbolic",
        &i18n("Voice Enhancement"),
    );
    let voice_label = gtk::Label::builder()
        .label("0")
        .width_chars(6)
        .xalign(0.0)
        .css_classes(["osd-value-label"])
        .build();
    voice_box.append(&voice_btn);
    voice_box.append(&voice_label);
    start_box.append(&voice_box);

    let max_vol = f64::from(settings.get_int(keys::VOLUME_MAX)).max(100.0);
    let initial_vol = settings.get_double(keys::VOLUME) * 100.0;
    let (vol_popover, vol_scale) = build_volume_popover(&volume_box, max_vol, initial_vol);
    {
        // Capture the button weakly: the scale's handler must not keep the
        // button alive (mutual cycle → leak on close).
        let mpv = mpv.clone();
        let vbtn = volume_btn.downgrade();
        // Weak label too: the popover can outlive teardown ordering — any
        // strong sibling capture here gets pinned with it.
        let vlabel = volume_label.downgrade();
        vol_scale.connect_value_changed(move |s| {
            let value = s.value();
            if let Some(vlabel) = vlabel.upgrade() {
                vlabel.set_label(&format!("{value:.0}"));
            }
            if let Some(vbtn) = vbtn.upgrade() {
                vbtn.set_icon_name(volume_icon(value));
            }
            if let Some(mpv) = mpv.borrow().as_ref() {
                mpv.set_volume(value);
                mpv.set_mute(false);
            }
        });
    }
    // Mute toggle on the volume button. Uses the handler's own `btn` argument
    // (no self-clone) and a weak scale ref (no mutual cycle).
    {
        let mpv = mpv.clone();
        let scale = vol_scale.downgrade();
        volume_btn.connect_clicked(move |btn| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                let muted = !mpv.get_mute();
                mpv.set_mute(muted);
                let fallback = scale.upgrade().map_or(0.0, |s| s.value());
                btn.set_icon_name(if muted {
                    "audio-volume-muted-symbolic"
                } else {
                    volume_icon(fallback)
                });
            }
        });
    }

    let spd_popover = build_speed_popover(&speed_box, &speed_btn, &speed_label, {
        let mpv = mpv.clone();
        move |speed| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                mpv.set_speed(speed);
            }
        }
    });

    let stored_nr = settings.get_double(keys::NOISE_STRENGTH).max(0.0);
    let initial_pct = (stored_nr * 100.0).clamp(0.0, 100.0);
    let prev_active = Rc::new(std::cell::Cell::new(stored_nr > 0.0));
    let voice_popover = build_voice_popover(
        &voice_box,
        &voice_btn,
        &voice_label,
        initial_pct,
        {
            let settings = settings.clone();
            let mpv = mpv.clone();
            move |pct| {
                crate::shared::popovers::handle_voice_strength_changed(
                    &settings,
                    mpv.borrow().as_ref().map(Arc::as_ref),
                    pct,
                    &prev_active,
                );
            }
        },
        {
            let settings = settings.clone();
            // Weak: this closure lives on the popover, which voice_btn's
            // handler keeps alive — a strong voice_box capture closes a
            // box→btn→popover→box cycle that survives popover unparent.
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
        (&volume_box, &vol_popover),
        (&speed_box, &spd_popover),
        (&voice_box, &voice_popover),
    ]);
    // The hover group keeps the popovers alive via the boxes' controllers; the
    // boxes are owned by `start_box`, so the returned handle can drop here.

    (
        start_box,
        volume_box,
        speed_box,
        voice_box,
        vec![vol_popover, spd_popover, voice_popover],
    )
}

/// CENTER box: previous · play/pause · next.
pub(super) fn build_center_box() -> (gtk::Box, gtk::Button, gtk::Button, gtk::Button) {
    let center = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .valign(gtk::Align::Center)
        .build();

    let prev_btn = osd_button("media-skip-backward-symbolic", &i18n("Previous"));
    let play_btn = osd_button("media-playback-start-symbolic", &i18n("Play"));
    play_btn.add_css_class("osd-btn-play");
    let next_btn = osd_button("media-skip-forward-symbolic", &i18n("Next"));

    center.append(&prev_btn);
    center.append(&play_btn);
    center.append(&next_btn);
    (center, prev_btn, play_btn, next_btn)
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
    let export_btn = gtk::MenuButton::builder()
        .icon_name("edit-cut-symbolic")
        .direction(gtk::ArrowType::Up)
        .css_classes(["osd-btn"])
        .menu_model(&build_export_menu())
        .build();
    export_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Edit & Export"))]);

    let subtitle_btn = gtk::MenuButton::builder()
        .icon_name("media-view-subtitles-symbolic")
        .direction(gtk::ArrowType::Up)
        .css_classes(["osd-btn"])
        .build();
    subtitle_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Subtitles"))]);

    let audio_track_btn = gtk::MenuButton::builder()
        .icon_name("audio-x-generic-symbolic")
        .direction(gtk::ArrowType::Up)
        .css_classes(["osd-btn"])
        .build();
    audio_track_btn.update_property(&[gtk::accessible::Property::Label(&i18n("Audio Track"))]);

    let fullscreen_btn = osd_button("view-fullscreen-symbolic", &i18n("Fullscreen"));

    end_box.append(&export_btn);
    end_box.append(&subtitle_btn);
    end_box.append(&audio_track_btn);
    end_box.append(&fullscreen_btn);
    (
        end_box,
        export_btn,
        subtitle_btn,
        audio_track_btn,
        fullscreen_btn,
    )
}

/// The Edit & Export menu (cut · extract audio · convert), targeting the
/// player window's `win.*` actions. Lives here so the footer button owns it.
fn build_export_menu() -> gio::Menu {
    let export = gio::Menu::new();
    export.append(Some(&i18n("Cut Video…")), Some("win.trim"));

    let extract = gio::Menu::new();
    extract.append(Some(&i18n("MP3")), Some("win.extract-audio-mp3"));
    extract.append(Some(&i18n("M4A")), Some("win.extract-audio-m4a"));
    extract.append(Some(&i18n("FLAC")), Some("win.extract-audio-flac"));
    export.append_submenu(Some(&i18n("Extract Audio")), &extract);

    let convert = gio::Menu::new();
    convert.append(Some(&i18n("MP4")), Some("win.convert-mp4"));
    convert.append(Some(&i18n("MKV")), Some("win.convert-mkv"));
    convert.append(Some(&i18n("WebM")), Some("win.convert-webm"));
    export.append_submenu(Some(&i18n("Convert to")), &convert);

    export
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
