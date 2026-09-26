use super::*;
use big_relm4_components::layout::hamburger_menu::{
    BigMenuActionItem, populate_flat_action_popover,
};
use gtk::gio;

impl VideoControlsBar {
    /// Build the subtitle + audio-track popover menus from the mpv track list.
    ///
    /// The subtitle popover carries two sections — **Primary** and **Secondary**
    /// subtitles — so the user can pick each track from the bar (secondary subs
    /// were previously reachable only through a keyboard toggle / Preferences).
    /// Each entry drives the live mpv backend read from the shared slot.
    pub(super) fn populate_track_menus(&self, tracks: &[TrackMetadata]) {
        let subtitle_tracks: Vec<&TrackMetadata> =
            tracks.iter().filter(|t| t.track_type == "sub").collect();

        let rows = gtk::Box::new(gtk::Orientation::Vertical, 0);
        rows.set_margin_top(6);
        rows.set_margin_bottom(6);
        rows.set_margin_start(6);
        rows.set_margin_end(6);

        // Always available — load an external subtitle file, even for a video
        // that ships no subtitle tracks of its own.
        rows.append(&track_button(
            &i18n("Load Subtitle File…"),
            &self.subtitle_menu_popover,
            {
                let mpv = self.mpv.clone();
                let anchor = self.subtitle_menu_button.clone();
                move || open_subtitle_file_dialog(&mpv, &anchor)
            },
        ));
        rows.append(&gtk::Separator::new(gtk::Orientation::Horizontal));

        rows.append(&section_label(&i18n("Primary Subtitles")));
        rows.append(&track_button(
            &i18n("Disable Subtitles"),
            &self.subtitle_menu_popover,
            {
                let mpv = self.mpv.clone();
                move || set_primary_subtitle(&mpv, -1)
            },
        ));
        for track in &subtitle_tracks {
            let id = track.id;
            rows.append(&track_button(
                &track_label(track),
                &self.subtitle_menu_popover,
                {
                    let mpv = self.mpv.clone();
                    move || set_primary_subtitle(&mpv, id)
                },
            ));
        }

        rows.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        rows.append(&section_label(&i18n("Secondary Subtitles")));
        rows.append(&track_button(
            &i18n("Disable Secondary Subtitles"),
            &self.subtitle_menu_popover,
            {
                let mpv = self.mpv.clone();
                move || set_secondary_subtitle(&mpv, -1)
            },
        ));
        for track in &subtitle_tracks {
            let id = track.id;
            rows.append(&track_button(
                &track_label(track),
                &self.subtitle_menu_popover,
                {
                    let mpv = self.mpv.clone();
                    move || set_secondary_subtitle(&mpv, id)
                },
            ));
        }

        rows.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        rows.append(&section_label(&i18n("Subtitles early or late")));
        rows.append(&subtitle_timing_row(&self.mpv));

        self.subtitle_menu_popover.add_css_class("menu");
        self.subtitle_menu_popover.set_child(Some(&rows));

        let audio_actions = gio::SimpleActionGroup::new();
        let mut audio_items = Vec::new();
        for track in tracks.iter().filter(|t| t.track_type == "audio") {
            let action = format!("select-{}", track.id);
            install_audio_track_action(&audio_actions, &action, &self.mpv, track.id);
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

/// Open a file chooser for an external subtitle and add it via mpv `sub-add`.
fn open_subtitle_file_dialog(mpv: &MpvSlot, anchor: &gtk::Button) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&i18n("Subtitle files")));
    for ext in ["srt", "ass", "ssa", "sub", "vtt", "idx", "sup", "smi"] {
        filter.add_suffix(ext);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);

    let dialog = gtk::FileDialog::builder()
        .title(i18n("Load Subtitle File"))
        .modal(true)
        .filters(&filters)
        .build();
    let window = anchor.root().and_downcast::<gtk::Window>();
    let mpv = mpv.clone();
    dialog.open(window.as_ref(), gio::Cancellable::NONE, move |result| {
        let Ok(file) = result else { return };
        let Some(path) = file.path() else { return };
        if let Some(mpv) = mpv.borrow().as_ref() {
            mpv.add_subtitle_file(&path.to_string_lossy());
        }
    });
}

/// Select the primary subtitle track (`id == -1` disables it).
fn set_primary_subtitle(mpv: &MpvSlot, id: i64) {
    if let Some(mpv) = mpv.borrow().as_ref() {
        mpv.set_subtitle_track(id);
    }
}

/// Select the secondary subtitle track (`id == -1` disables it).
fn set_secondary_subtitle(mpv: &MpvSlot, id: i64) {
    if let Some(mpv) = mpv.borrow().as_ref() {
        if id < 0 {
            mpv.disable_secondary_sub();
        } else {
            mpv.set_secondary_sid(id);
        }
    }
}

/// A flat popover row that runs `on_click` then dismisses the popover.
fn track_button(label: &str, popover: &gtk::Popover, on_click: impl Fn() + 'static) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.set_has_frame(false);
    button.add_css_class("flat");
    let popover = popover.downgrade();
    button.connect_clicked(move |_| {
        on_click();
        if let Some(popover) = popover.upgrade() {
            popover.popdown();
        }
    });
    button
}

/// "Earlier  −0.3 s  Later" for text that shows before or after the speech;
/// the popover stays open so it can be nudged until it matches.
fn subtitle_timing_row(mpv: &MpvSlot) -> gtk::Box {
    let row = gtk::Box::builder()
        .spacing(4)
        .margin_start(6)
        .margin_end(6)
        .build();
    let delay = mpv
        .borrow()
        .as_ref()
        .map_or(0.0, |mpv| mpv.get_property_f64("sub-delay"));
    let value = gtk::Label::builder()
        .label(format!("{delay:+.1} s"))
        .hexpand(true)
        .width_chars(7)
        .build();
    // A chip or shortcut may have changed it since the menu was built.
    value.connect_map({
        let mpv = mpv.clone();
        move |label| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                label.set_label(&format!("{:+.1} s", mpv.get_property_f64("sub-delay")));
            }
        }
    });
    for (text, step) in [(i18n("Earlier"), -0.1), (i18n("Later"), 0.1)] {
        let button = gtk::Button::with_label(&text);
        button.add_css_class("flat");
        let mpv = mpv.clone();
        let label = value.downgrade();
        button.connect_clicked(move |_| {
            if let Some(mpv) = mpv.borrow().as_ref() {
                let delay = ((mpv.get_property_f64("sub-delay") + step) * 10.0).round() / 10.0;
                mpv.set_sub_delay(delay);
                if let Some(label) = label.upgrade() {
                    label.set_label(&format!("{delay:+.1} s"));
                }
            }
        });
        if step < 0.0 {
            row.append(&button);
            row.append(&value);
        } else {
            row.append(&button);
        }
    }
    row
}

/// A dim section heading inside a popover.
fn section_label(text: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.add_css_class("caption-heading");
    label.set_margin_start(6);
    label.set_margin_top(4);
    label
}

/// Install a stateless action that selects an audio track on the live mpv backend.
fn install_audio_track_action(group: &gio::SimpleActionGroup, name: &str, mpv: &MpvSlot, id: i64) {
    let mpv = mpv.clone();
    big_app_kit::desktop::install_action(group, name, move || {
        if let Some(mpv) = mpv.borrow().as_ref() {
            mpv.set_audio_track(id);
        }
    });
}

/// Human-readable track label (id + title + lang + codec).
fn track_label(track: &TrackMetadata) -> String {
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
