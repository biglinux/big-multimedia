// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared audio playback bar for BigLinux media apps.
//!
//! Unlike a video transport, the audio one
//! carries hover-driven volume/speed/voice controls, shuffle/repeat toggles,
//! a lyrics toggle and a playlist button, plus a status label and a blurred
//! album-art background — and it has no fullscreen or frame-step controls.
//!
//! This builder owns the widget tree only. mpv/MPRIS/settings side effects and
//! the hover popovers are wired by the consuming app against the exposed
//! accessors (the popover factories live above this crate, in
//! `big-media-runtime`).

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use relm4::gtk;

/// Translated tooltip/accessible labels for the audio transport controls.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioTransportLabels {
    pub seek: String,
    pub cut_timeline: String,
    pub play_pause: String,
    pub previous: String,
    pub next: String,
    pub shuffle: String,
    pub repeat: String,
    pub volume: String,
    pub speed: String,
    pub voice: String,
    pub trim_export: String,
    pub lyrics: String,
    pub playlist: String,
    pub status: String,
    pub album_art: String,
}

/// Builder spec for [`BigAudioTransport`].
///
/// Use [`BigAudioTransportSpec::new`] for the full player bar; clear
/// [`include_voice`](Self::include_voice) when no noise-reduction backend is
/// available so the voice control is omitted.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigAudioTransportSpec {
    pub labels: BigAudioTransportLabels,
    pub include_voice: bool,
}

impl BigAudioTransportSpec {
    /// Build the default audio player transport spec (voice control included).
    #[must_use]
    pub fn new(labels: BigAudioTransportLabels) -> Self {
        Self {
            labels,
            include_voice: true,
        }
    }
}

/// Audio transport widget tree: seek row, hover volume/speed/voice, transport
/// buttons, shuffle/repeat/lyrics/playlist, status label, blurred art overlay.
///
/// Built once per player via [`BigAudioTransport::new`]; the accessors expose
/// each interactive widget so the app can wire mpv/MPRIS side effects and the
/// hover popovers.
///
/// # Examples
///
/// ```ignore
/// use big_media_components::playback::audio_transport::{
///     BigAudioTransport, BigAudioTransportLabels, BigAudioTransportSpec,
/// };
///
/// let bar = BigAudioTransport::new(
///     BigAudioTransportSpec::new(BigAudioTransportLabels::english()),
/// );
/// toolbar_view.add_bottom_bar(bar.root());
/// bar.play_button().connect_clicked(|_| toggle_playback());
/// ```
#[derive(Debug, Clone)]
pub struct BigAudioTransport {
    root: gtk::Overlay,
    playback_bar: gtk::Box,
    seek_bar: gtk::Scale,
    cut_timeline: gtk::DrawingArea,
    bar_stack: gtk::Stack,
    position_label: gtk::Inscription,
    duration_label: gtk::Label,
    volume_hover_box: gtk::Box,
    volume_button: gtk::Button,
    volume_label: gtk::Label,
    speed_hover_box: gtk::Box,
    speed_button: gtk::Button,
    speed_label: gtk::Label,
    voice_hover_box: gtk::Box,
    voice_button: gtk::Button,
    voice_label: gtk::Label,
    prev_button: gtk::Button,
    play_button: gtk::Button,
    next_button: gtk::Button,
    status_label: gtk::Label,
    trim_export_button: gtk::Button,
    shuffle_button: gtk::ToggleButton,
    repeat_button: gtk::ToggleButton,
    lyrics_button: gtk::ToggleButton,
    playlist_button: gtk::Button,
    blur_bg: gtk::Picture,
}

impl BigAudioTransport {
    /// Build the audio transport widget tree from `spec`.
    #[must_use]
    pub fn new(spec: BigAudioTransportSpec) -> Self {
        let labels = &spec.labels;

        let playback_bar = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(0)
            .vexpand(false)
            .css_classes(["toolbar", "playback-bar"])
            .build();

        // ── Seek row: position · scale · duration ──
        let (seek_bar, cut_timeline, bar_stack, position_label, duration_label) =
            build_seek_row(labels);
        let seek_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .margin_start(16)
            .margin_end(16)
            .margin_top(6)
            .build();
        seek_row.append(&position_label);
        seek_row.append(&bar_stack);
        seek_row.append(&duration_label);
        playback_bar.append(&seek_row);

        // ── Controls row (CenterBox) ──
        let controls_row = gtk::CenterBox::builder()
            .margin_start(12)
            .margin_end(12)
            .margin_top(4)
            .margin_bottom(8)
            .build();

        // start: volume / speed / voice
        let (volume_hover_box, volume_button, volume_label) =
            build_hover_control("audio-volume-high-symbolic", &labels.volume, "100", 3);
        let (speed_hover_box, speed_button, speed_label) = build_hover_control(
            "power-profile-performance-symbolic",
            &labels.speed,
            "1.00x",
            5,
        );
        let (voice_hover_box, voice_button, voice_label) =
            build_hover_control("audio-input-microphone-symbolic", &labels.voice, "0", 3);
        let start_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .valign(gtk::Align::Center)
            .build();
        start_box.append(&volume_hover_box);
        start_box.append(&speed_hover_box);
        if spec.include_voice {
            start_box.append(&voice_hover_box);
        }
        controls_row.set_start_widget(Some(&start_box));

        // center: prev / play / next
        let prev_button =
            tooltip::icon_button("media-skip-backward-symbolic", &labels.previous, &[]);
        prev_button.set_has_frame(false);
        let play_button = tooltip::icon_button(
            "media-playback-start-symbolic",
            &labels.play_pause,
            &["circular", "suggested-action"],
        );
        play_button.set_size_request(42, 42);
        let next_button = tooltip::icon_button("media-skip-forward-symbolic", &labels.next, &[]);
        next_button.set_has_frame(false);
        let center_box = gtk::Box::builder()
            .spacing(4)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .build();
        center_box.append(&prev_button);
        center_box.append(&play_button);
        center_box.append(&next_button);
        controls_row.set_center_widget(Some(&center_box));

        // end: status / trim-export / shuffle / repeat / lyrics / playlist
        let status_label = gtk::Label::builder()
            .label("")
            .xalign(1.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(20)
            .css_classes(["caption"])
            .build();
        status_label.update_property(&[gtk::accessible::Property::Label(&labels.status)]);
        let trim_export_button =
            tooltip::icon_button("edit-cut-symbolic", &labels.trim_export, &[]);
        trim_export_button.set_has_frame(false);
        let shuffle_button = tooltip::icon_toggle_button(
            "media-playlist-shuffle-symbolic",
            &labels.shuffle,
            false,
            &[],
        );
        shuffle_button.set_has_frame(false);
        let repeat_button = tooltip::icon_toggle_button(
            "media-playlist-repeat-symbolic",
            &labels.repeat,
            false,
            &[],
        );
        repeat_button.set_has_frame(false);
        let lyrics_button =
            tooltip::icon_toggle_button("format-text-italic-symbolic", &labels.lyrics, false, &[]);
        lyrics_button.set_has_frame(false);
        let playlist_button = tooltip::icon_button("view-list-symbolic", &labels.playlist, &[]);
        playlist_button.set_has_frame(false);
        let end_box = gtk::Box::builder()
            .spacing(4)
            .valign(gtk::Align::Center)
            .build();
        end_box.append(&status_label);
        end_box.append(&trim_export_button);
        end_box.append(&shuffle_button);
        end_box.append(&repeat_button);
        end_box.append(&lyrics_button);
        end_box.append(&playlist_button);
        controls_row.set_end_widget(Some(&end_box));

        playback_bar.append(&controls_row);

        // ── Overlay: blurred album-art background behind the bar ──
        // Expand + FILL on both axes so the cover always covers the full bar
        // regardless of its intrinsic aspect ratio.
        let blur_bg = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Cover)
            .can_target(false)
            .can_focus(false)
            .visible(false)
            .hexpand(true)
            .vexpand(true)
            .halign(gtk::Align::Fill)
            .valign(gtk::Align::Fill)
            .css_classes(["blur-bg"])
            .build();
        blur_bg.update_property(&[gtk::accessible::Property::Label(&labels.album_art)]);

        let root = gtk::Overlay::new();
        root.set_child(Some(&playback_bar));
        root.add_overlay(&blur_bg);

        Self {
            root,
            playback_bar,
            seek_bar,
            cut_timeline,
            bar_stack,
            position_label,
            duration_label,
            volume_hover_box,
            volume_button,
            volume_label,
            speed_hover_box,
            speed_button,
            speed_label,
            voice_hover_box,
            voice_button,
            voice_label,
            prev_button,
            play_button,
            next_button,
            status_label,
            trim_export_button,
            shuffle_button,
            repeat_button,
            lyrics_button,
            playlist_button,
            blur_bg,
        }
    }

    /// Borrow the root [`gtk::Overlay`] (bar + blurred background).
    #[must_use]
    pub fn root(&self) -> &gtk::Overlay {
        &self.root
    }

    /// Borrow the playback bar [`gtk::Box`] (seek row + controls).
    #[must_use]
    pub fn playback_bar(&self) -> &gtk::Box {
        &self.playback_bar
    }

    /// Borrow the seek scale (range `0..=100`).
    #[must_use]
    pub fn seek_bar(&self) -> &gtk::Scale {
        &self.seek_bar
    }

    /// Borrow the trim-mode cut timeline.
    #[must_use]
    pub fn cut_timeline(&self) -> &gtk::DrawingArea {
        &self.cut_timeline
    }

    /// Show either the normal seek scale or the trim-mode cut timeline.
    pub fn set_cut_timeline_visible(&self, visible: bool) {
        self.bar_stack
            .set_visible_child_name(if visible { "cut" } else { "seek" });
        self.cut_timeline.queue_draw();
    }

    /// Borrow the elapsed-position label.
    #[must_use]
    pub fn position_label(&self) -> &gtk::Inscription {
        &self.position_label
    }

    /// Borrow the total-duration label.
    #[must_use]
    pub fn duration_label(&self) -> &gtk::Label {
        &self.duration_label
    }

    /// Borrow the volume hover box (popover anchor).
    #[must_use]
    pub fn volume_hover_box(&self) -> &gtk::Box {
        &self.volume_hover_box
    }

    /// Borrow the volume/mute button.
    #[must_use]
    pub fn volume_button(&self) -> &gtk::Button {
        &self.volume_button
    }

    /// Borrow the volume percent label.
    #[must_use]
    pub fn volume_label(&self) -> &gtk::Label {
        &self.volume_label
    }

    /// Borrow the speed hover box (popover anchor).
    #[must_use]
    pub fn speed_hover_box(&self) -> &gtk::Box {
        &self.speed_hover_box
    }

    /// Borrow the speed button.
    #[must_use]
    pub fn speed_button(&self) -> &gtk::Button {
        &self.speed_button
    }

    /// Borrow the speed label.
    #[must_use]
    pub fn speed_label(&self) -> &gtk::Label {
        &self.speed_label
    }

    /// Borrow the voice hover box (popover anchor).
    #[must_use]
    pub fn voice_hover_box(&self) -> &gtk::Box {
        &self.voice_hover_box
    }

    /// Borrow the voice-enhancement button.
    #[must_use]
    pub fn voice_button(&self) -> &gtk::Button {
        &self.voice_button
    }

    /// Borrow the voice-strength label.
    #[must_use]
    pub fn voice_label(&self) -> &gtk::Label {
        &self.voice_label
    }

    /// Borrow the "previous track" button.
    #[must_use]
    pub fn previous_button(&self) -> &gtk::Button {
        &self.prev_button
    }

    /// Borrow the play/pause button.
    #[must_use]
    pub fn play_button(&self) -> &gtk::Button {
        &self.play_button
    }

    /// Borrow the "next track" button.
    #[must_use]
    pub fn next_button(&self) -> &gtk::Button {
        &self.next_button
    }

    /// Borrow the status label.
    #[must_use]
    pub fn status_label(&self) -> &gtk::Label {
        &self.status_label
    }

    /// Borrow the trim/export button.
    #[must_use]
    pub fn trim_export_button(&self) -> &gtk::Button {
        &self.trim_export_button
    }

    /// Borrow the shuffle toggle.
    #[must_use]
    pub fn shuffle_button(&self) -> &gtk::ToggleButton {
        &self.shuffle_button
    }

    /// Borrow the repeat toggle.
    #[must_use]
    pub fn repeat_button(&self) -> &gtk::ToggleButton {
        &self.repeat_button
    }

    /// Borrow the lyrics toggle.
    #[must_use]
    pub fn lyrics_button(&self) -> &gtk::ToggleButton {
        &self.lyrics_button
    }

    /// Borrow the playlist button.
    #[must_use]
    pub fn playlist_button(&self) -> &gtk::Button {
        &self.playlist_button
    }

    /// Borrow the blurred album-art background picture.
    #[must_use]
    pub fn blur_bg(&self) -> &gtk::Picture {
        &self.blur_bg
    }

    /// Update the play button's icon to reflect `paused`.
    pub fn set_paused(&self, paused: bool) {
        self.play_button.set_icon_name(if paused {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        });
    }
}

/// Build a hover-control cluster: a flat icon button plus a caption label,
/// returned with the wrapping box used as the popover hover anchor.
fn build_hover_control(
    icon_name: &str,
    label: &str,
    initial_text: &str,
    width_chars: i32,
) -> (gtk::Box, gtk::Button, gtk::Label) {
    let hover_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(2)
        .build();
    let button = tooltip::icon_button(icon_name, label, &[]);
    button.set_has_frame(false);
    let value_label = gtk::Label::builder()
        .label(initial_text)
        .width_chars(width_chars)
        .xalign(0.0)
        .css_classes(["caption"])
        .build();
    hover_box.append(&button);
    hover_box.append(&value_label);
    (hover_box, button, value_label)
}

/// Build the seek row scale plus the position/duration labels.
/// Characters reserved for the elapsed time: "0:00" plus a spare, so digits of
/// uneven width never clip. [`time_label_chars`] widens it for long media.
pub const TIME_CHARS: u32 = 5;

/// Characters the elapsed time needs to show a position as long as `duration`.
#[must_use]
pub fn time_label_chars(duration: &str) -> u32 {
    u32::try_from(duration.chars().count()).map_or(TIME_CHARS, |chars| (chars + 1).max(TIME_CHARS))
}

fn build_seek_row(
    labels: &BigAudioTransportLabels,
) -> (
    gtk::Scale,
    gtk::DrawingArea,
    gtk::Stack,
    gtk::Inscription,
    gtk::Label,
) {
    // An inscription, not a label: its width does not follow the text, so the
    // once-a-second change repaints it alone. A GtkLabel relayouts the window,
    // and GTK's synthetic pointer motion after the relayout keeps a hovered
    // list's overlay scrollbar fading in and out, ~27 frames a second.
    let position_label = gtk::Inscription::builder()
        .text("0:00")
        .min_chars(TIME_CHARS)
        .nat_chars(TIME_CHARS)
        .xalign(1.0)
        .css_classes(["caption"])
        .build();
    let adjustment = gtk::Adjustment::new(0.0, 0.0, 100.0, 5.0, 0.0, 0.0);
    let seek_bar = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .draw_value(false)
        .hexpand(true)
        .adjustment(&adjustment)
        .build();
    seek_bar.set_tooltip_text(Some(&labels.seek));
    seek_bar.update_property(&[gtk::accessible::Property::Label(&labels.seek)]);
    let cut_timeline = gtk::DrawingArea::builder()
        .hexpand(true)
        .height_request(20)
        .build();
    cut_timeline.update_property(&[gtk::accessible::Property::Label(&labels.cut_timeline)]);
    cut_timeline.set_tooltip_text(Some(&labels.cut_timeline));
    let bar_stack = gtk::Stack::builder().hexpand(true).build();
    bar_stack.add_named(&seek_bar, Some("seek"));
    bar_stack.add_named(&cut_timeline, Some("cut"));
    bar_stack.set_visible_child_name("seek");
    let duration_label = gtk::Label::builder()
        .label("0:00")
        .width_chars(5)
        .xalign(0.0)
        .css_classes(["caption"])
        .build();
    (
        seek_bar,
        cut_timeline,
        bar_stack,
        position_label,
        duration_label,
    )
}
