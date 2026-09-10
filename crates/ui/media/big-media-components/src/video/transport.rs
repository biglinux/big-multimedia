// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared video transport controls for BigLinux media apps.

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use relm4::gtk;

/// Translated tooltip/accessible labels for the transport buttons.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoTransportLabels {
    pub timeline: String,
    pub play_pause: String,
    pub mark_point: String,
    pub seek_back: String,
    pub seek_forward: String,
    pub previous_frame: String,
    pub next_frame: String,
    pub previous: String,
    pub next: String,
    pub fullscreen: String,
    pub playback_position: String,
    pub playback_duration: String,
}

impl BigVideoTransportLabels {
    /// Hard-coded English defaults useful for unit tests and previews.
    #[must_use]
    pub fn english() -> Self {
        Self {
            timeline: "Timeline".into(),
            play_pause: "Play/Pause".into(),
            mark_point: "Mark point".into(),
            seek_back: "Seek back".into(),
            seek_forward: "Seek forward".into(),
            previous_frame: "Previous frame".into(),
            next_frame: "Next frame".into(),
            previous: "Previous".into(),
            next: "Next".into(),
            fullscreen: "Fullscreen".into(),
            playback_position: "Playback position".into(),
            playback_duration: "Playback duration".into(),
        }
    }
}

/// Builder spec for [`BigVideoTransport`].
///
/// Pick a preset constructor — [`BigVideoTransportSpec::new`] for player
/// mode or [`BigVideoTransportSpec::editor`] for editor mode — and tweak
/// the fields as needed. Use [`resolved`](Self::resolved) to inspect the
/// computed button count in tests.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoTransportSpec {
    pub labels: BigVideoTransportLabels,
    pub include_mark: bool,
    pub include_skip: bool,
    pub include_seek_step: bool,
    pub include_frame_step: bool,
    pub append_fullscreen: bool,
    pub timeline_can_target: bool,
}

impl BigVideoTransportSpec {
    /// Build the default player transport spec (seek step + fullscreen).
    #[must_use]
    pub fn new(labels: BigVideoTransportLabels) -> Self {
        Self {
            labels,
            include_mark: false,
            include_skip: false,
            include_seek_step: true,
            include_frame_step: false,
            append_fullscreen: true,
            timeline_can_target: true,
        }
    }

    /// Build the editor transport spec (mark + seek step + frame step, no fullscreen).
    #[must_use]
    pub fn editor(labels: BigVideoTransportLabels) -> Self {
        Self {
            labels,
            include_mark: true,
            include_skip: false,
            include_seek_step: true,
            include_frame_step: true,
            append_fullscreen: false,
            timeline_can_target: false,
        }
    }

    /// Compute the button count and capability flags implied by this spec.
    #[must_use]
    pub fn resolved(&self) -> BigVideoTransportResolved {
        let mut buttons = 1;
        buttons += usize::from(self.include_mark);
        buttons += usize::from(self.include_skip) * 2;
        buttons += usize::from(self.include_seek_step) * 2;
        buttons += usize::from(self.include_frame_step) * 2;
        buttons += usize::from(self.append_fullscreen);

        BigVideoTransportResolved {
            button_count: buttons,
            timeline_can_target: self.timeline_can_target,
            has_precise_seek: self.include_seek_step || self.include_frame_step,
        }
    }
}

/// Resolved view of a [`BigVideoTransportSpec`] for tests and validators.
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoTransportResolved {
    pub button_count: usize,
    pub timeline_can_target: bool,
    pub has_precise_seek: bool,
}

/// Video transport widget tree: scrub bar, play/pause/seek/frame buttons, fullscreen.
///
/// Built once per video panel via [`BigVideoTransport::new`]; subsequent
/// accessors expose individual buttons for signal wiring.
///
/// # Examples
///
/// ```ignore
/// use big_media_components::video::transport::{
///     BigVideoTransport, BigVideoTransportLabels, BigVideoTransportSpec,
/// };
///
/// let transport = BigVideoTransport::new(
///     BigVideoTransportSpec::new(BigVideoTransportLabels::english()),
/// );
/// container.append(transport.root());
/// transport.play_button().connect_clicked(|_| toggle_playback());
/// ```
#[derive(Debug, Clone)]
pub struct BigVideoTransport {
    root: gtk::Box,
    slider_overlay: gtk::Overlay,
    controls_box: gtk::Box,
    timeline: gtk::Scale,
    position_label: gtk::Label,
    trailing_label: gtk::Label,
    mark_button: gtk::Button,
    seek_back_button: gtk::Button,
    frame_back_button: gtk::Button,
    play_button: gtk::Button,
    frame_fwd_button: gtk::Button,
    seek_fwd_button: gtk::Button,
    prev_button: gtk::Button,
    next_button: gtk::Button,
    fullscreen_button: gtk::Button,
}

impl BigVideoTransport {
    /// Build the transport widget tree from `spec`.
    #[must_use]
    pub fn new(spec: BigVideoTransportSpec) -> Self {
        let labels = spec.labels.clone();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::End)
            .hexpand(true)
            .css_classes(["osd", "toolbar"])
            .build();

        let timeline = gtk::Scale::builder()
            .orientation(gtk::Orientation::Horizontal)
            .hexpand(true)
            .draw_value(false)
            .build();
        timeline.update_property(&[gtk::accessible::Property::Label(&labels.timeline)]);
        timeline.set_range(0.0, 1.0);
        timeline.set_margin_start(8);
        timeline.set_margin_end(8);
        timeline.set_can_target(spec.timeline_can_target);

        let slider_overlay = gtk::Overlay::new();
        slider_overlay.set_child(Some(&timeline));
        root.append(&slider_overlay);

        let controls_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(2)
            .halign(gtk::Align::Center)
            .margin_top(2)
            .margin_bottom(2)
            .build();

        let mark_button = icon_button("bookmark-new-symbolic", &labels.mark_point);
        let seek_back_button = icon_button("media-seek-backward-symbolic", &labels.seek_back);
        let frame_back_button = icon_button("go-previous-symbolic", &labels.previous_frame);
        let play_button = icon_button("media-playback-start-symbolic", &labels.play_pause);
        let frame_fwd_button = icon_button("go-next-symbolic", &labels.next_frame);
        let seek_fwd_button = icon_button("media-seek-forward-symbolic", &labels.seek_forward);
        let prev_button = icon_button("media-skip-backward-symbolic", &labels.previous);
        let next_button = icon_button("media-skip-forward-symbolic", &labels.next);
        let fullscreen_button = icon_button("view-fullscreen-symbolic", &labels.fullscreen);

        if spec.include_mark {
            controls_box.append(&mark_button);
            controls_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        }
        if spec.include_skip {
            controls_box.append(&prev_button);
        }
        if spec.include_seek_step {
            controls_box.append(&seek_back_button);
        }
        if spec.include_frame_step {
            controls_box.append(&frame_back_button);
        }
        controls_box.append(&play_button);
        if spec.include_frame_step {
            controls_box.append(&frame_fwd_button);
        }
        if spec.include_seek_step {
            controls_box.append(&seek_fwd_button);
        }
        if spec.include_skip {
            controls_box.append(&next_button);
        }
        if spec.append_fullscreen {
            controls_box.append(&gtk::Separator::new(gtk::Orientation::Vertical));
            controls_box.append(&fullscreen_button);
        }
        root.append(&controls_box);

        let time_row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(4)
            .build();
        let position_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(["monospace", "dim-label", "caption"])
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        let trailing_label = gtk::Label::builder()
            .label("0:00")
            .css_classes(["monospace", "dim-label", "caption"])
            .halign(gtk::Align::End)
            .build();
        time_row.append(&position_label);
        time_row.append(&trailing_label);
        root.append(&time_row);

        Self {
            root,
            slider_overlay,
            controls_box,
            timeline,
            position_label,
            trailing_label,
            mark_button,
            seek_back_button,
            frame_back_button,
            play_button,
            frame_fwd_button,
            seek_fwd_button,
            prev_button,
            next_button,
            fullscreen_button,
        }
    }

    /// Borrow the root [`gtk::Box`].
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }

    /// Borrow the overlay that hosts the scrub bar (apps add segment overlays here).
    #[must_use]
    pub fn slider_overlay(&self) -> &gtk::Overlay {
        &self.slider_overlay
    }

    /// Borrow the controls row (play, seek, fullscreen) for appending auxiliary widgets.
    #[must_use]
    pub fn controls_box(&self) -> &gtk::Box {
        &self.controls_box
    }

    /// Borrow the scrub bar scale.
    #[must_use]
    pub fn timeline(&self) -> &gtk::Scale {
        &self.timeline
    }

    /// Borrow the leading label (position / duration text).
    #[must_use]
    pub fn position_label(&self) -> &gtk::Label {
        &self.position_label
    }

    /// Borrow the trailing label (frame info or auxiliary text).
    #[must_use]
    pub fn trailing_label(&self) -> &gtk::Label {
        &self.trailing_label
    }

    /// Borrow the "mark point" button (editor mode).
    #[must_use]
    pub fn mark_button(&self) -> &gtk::Button {
        &self.mark_button
    }

    /// Borrow the "seek backward" button.
    #[must_use]
    pub fn seek_back_button(&self) -> &gtk::Button {
        &self.seek_back_button
    }

    /// Borrow the "previous frame" button.
    #[must_use]
    pub fn frame_back_button(&self) -> &gtk::Button {
        &self.frame_back_button
    }

    /// Borrow the play/pause button.
    #[must_use]
    pub fn play_button(&self) -> &gtk::Button {
        &self.play_button
    }

    /// Borrow the "next frame" button.
    #[must_use]
    pub fn frame_forward_button(&self) -> &gtk::Button {
        &self.frame_fwd_button
    }

    /// Borrow the "seek forward" button.
    #[must_use]
    pub fn seek_forward_button(&self) -> &gtk::Button {
        &self.seek_fwd_button
    }

    /// Borrow the "previous track" button (player mode).
    #[must_use]
    pub fn previous_button(&self) -> &gtk::Button {
        &self.prev_button
    }

    /// Borrow the "next track" button (player mode).
    #[must_use]
    pub fn next_button(&self) -> &gtk::Button {
        &self.next_button
    }

    /// Borrow the fullscreen toggle button.
    #[must_use]
    pub fn fullscreen_button(&self) -> &gtk::Button {
        &self.fullscreen_button
    }

    /// Update the play button's icon to reflect `paused`.
    pub fn set_paused(&self, paused: bool) {
        set_play_button_paused(&self.play_button, paused);
    }
}

/// Borrowed widget references handed to [`configure_existing_transport`].
///
/// Use this when a caller built their own transport widgets (e.g. with
/// Blueprint) and only wants to retrofit the shared tooltip/AT-SPI labels.
#[allow(missing_docs)]
pub struct BigVideoTransportParts<'a> {
    pub timeline: &'a gtk::Scale,
    pub position_label: &'a gtk::Label,
    pub duration_label: &'a gtk::Label,
    pub play_button: &'a gtk::Button,
    pub prev_button: Option<&'a gtk::Button>,
    pub next_button: Option<&'a gtk::Button>,
    pub fullscreen_button: Option<&'a gtk::Button>,
}

/// Apply translated tooltips and accessible labels to existing transport widgets.
pub fn configure_existing_transport(
    parts: BigVideoTransportParts<'_>,
    labels: &BigVideoTransportLabels,
) {
    parts
        .timeline
        .update_property(&[gtk::accessible::Property::Label(&labels.timeline)]);
    tooltip::set(parts.timeline, &labels.timeline);

    parts
        .position_label
        .update_property(&[gtk::accessible::Property::Label(&labels.playback_position)]);
    parts
        .duration_label
        .update_property(&[gtk::accessible::Property::Label(&labels.playback_duration)]);

    tooltip::set(parts.play_button, &labels.play_pause);
    parts
        .play_button
        .update_property(&[gtk::accessible::Property::Label(&labels.play_pause)]);

    if let Some(prev_button) = parts.prev_button {
        tooltip::set(prev_button, &labels.previous);
        prev_button.update_property(&[gtk::accessible::Property::Label(&labels.previous)]);
    }
    if let Some(next_button) = parts.next_button {
        tooltip::set(next_button, &labels.next);
        next_button.update_property(&[gtk::accessible::Property::Label(&labels.next)]);
    }
    if let Some(fullscreen_button) = parts.fullscreen_button {
        tooltip::set(fullscreen_button, &labels.fullscreen);
        fullscreen_button.update_property(&[gtk::accessible::Property::Label(&labels.fullscreen)]);
    }
}

/// Update an external play/pause button's icon to match the `paused` state.
pub fn set_play_button_paused(play_button: &gtk::Button, paused: bool) {
    play_button.set_icon_name(if paused {
        "media-playback-start-symbolic"
    } else {
        "media-playback-pause-symbolic"
    });
}

fn icon_button(icon_name: &str, label: &str) -> gtk::Button {
    tooltip::icon_button(icon_name, label, &["flat", "circular"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_transport_contract_includes_precise_seek_controls() {
        let spec = BigVideoTransportSpec::editor(BigVideoTransportLabels::english());
        let resolved = spec.resolved();
        assert_eq!(resolved.button_count, 6);
        assert!(resolved.has_precise_seek);
        assert!(!resolved.timeline_can_target);
    }

    #[test]
    fn default_transport_contract_has_play_seek_and_fullscreen() {
        let spec = BigVideoTransportSpec::new(BigVideoTransportLabels::english());
        let resolved = spec.resolved();
        assert_eq!(resolved.button_count, 4);
        assert!(resolved.has_precise_seek);
        assert!(resolved.timeline_can_target);
    }
}
