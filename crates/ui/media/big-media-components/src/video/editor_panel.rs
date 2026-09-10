//! Reusable video editor preview panel with transport, crop and info widgets.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use relm4::gtk;

use crate::media::metadata_grid::{BigMediaMetadataGrid, BigMediaMetadataLabels};
use crate::video::aux_controls::{
    BigVideoAuxControlLabels, BigVideoAuxControlSpec, BigVideoAuxControls,
};
use crate::video::crop_controls::{BigVideoCropControls, BigVideoCropLabels};
use crate::video::transport::{BigVideoTransport, BigVideoTransportLabels, BigVideoTransportSpec};

/// Translated strings used by [`build_video_editor_panel`].
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoEditorPanelText {
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
    pub playback_speed: String,
    pub volume: String,
    pub audio_track: String,
    pub subtitle_track: String,
    pub no_tracks_loaded: String,
    pub no_subtitles: String,
    pub crop_left: String,
    pub crop_right: String,
    pub crop_top: String,
    pub crop_bottom: String,
    pub reset_crop: String,
    pub show_grid_overlay: String,
    pub resolution: String,
    pub codec: String,
    pub size: String,
    pub duration: String,
}

impl BigVideoEditorPanelText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            timeline: text("Timeline"),
            play_pause: text("Play/Pause (Space)"),
            mark_point: text("Mark point (M)"),
            seek_back: text("Seek back 5s"),
            seek_forward: text("Seek forward 5s"),
            previous_frame: text("Previous frame (,)"),
            next_frame: text("Next frame (.)"),
            previous: text("Previous"),
            next: text("Next"),
            fullscreen: text("Fullscreen (F)"),
            playback_position: text("Playback position"),
            playback_duration: text("Playback duration"),
            playback_speed: text("Playback speed"),
            volume: text("Volume"),
            audio_track: text("Audio track"),
            subtitle_track: text("Subtitle track"),
            no_tracks_loaded: text("No tracks loaded"),
            no_subtitles: text("No subtitles"),
            crop_left: text("Left"),
            crop_right: text("Right"),
            crop_top: text("Top"),
            crop_bottom: text("Bottom"),
            reset_crop: text("Reset crop"),
            show_grid_overlay: text("Show grid overlay"),
            resolution: text("Resolution:"),
            codec: text("Codec:"),
            size: text("Size:"),
            duration: text("Duration:"),
        }
    }
}

/// Widgets returned by [`build_video_editor_panel`].
///
/// Every field is a clonable handle the caller owns; signal handlers
/// drive seek, transport, crop, and information updates.
#[allow(missing_docs)]
pub struct BigVideoEditorPanelBuild {
    pub video_panel: gtk::Box,
    pub overlay: gtk::Overlay,
    pub video_area: gtk::GLArea,
    pub crop_overlay_area: gtk::DrawingArea,
    pub segment_overlay: gtk::DrawingArea,
    pub transport_overlay_box: gtk::Box,
    pub timeline: gtk::Scale,
    pub position_label: gtk::Label,
    pub frame_label: gtk::Label,
    pub play_button: gtk::Button,
    pub mark_button: gtk::Button,
    pub speed_button: gtk::Button,
    pub speed_box: gtk::Box,
    pub volume_button: gtk::Button,
    pub volume_scale: gtk::Scale,
    pub fullscreen_button: gtk::Button,
    pub audio_track_button: gtk::Button,
    pub subtitle_track_button: gtk::Button,
    pub audio_track_box: gtk::Box,
    pub subtitle_track_box: gtk::Box,
    pub seek_back_button: gtk::Button,
    pub frame_back_button: gtk::Button,
    pub frame_fwd_button: gtk::Button,
    pub seek_fwd_button: gtk::Button,
    pub crop_left: gtk::SpinButton,
    pub crop_right: gtk::SpinButton,
    pub crop_top: gtk::SpinButton,
    pub crop_bottom: gtk::SpinButton,
    pub crop_reset_button: gtk::Button,
    pub grid_button: gtk::ToggleButton,
    pub media_metadata_resolution: gtk::Label,
    pub media_metadata_codec: gtk::Label,
    pub media_metadata_file_size: gtk::Label,
    pub media_metadata_duration: gtk::Label,
}

/// Build the full video editor preview panel.
///
/// Combines an mpv-rendered `GLArea`, crop overlay, transport controls,
/// auxiliary speed/volume/track popovers, crop spinners, and an info grid
/// into a single column.
#[must_use]
pub fn build_video_editor_panel(
    text: BigVideoEditorPanelText,
    speed_labels: &[&str],
) -> BigVideoEditorPanelBuild {
    let video_panel = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let overlay = gtk::Overlay::new();
    let video_area = gtk::GLArea::builder()
        .vexpand(true)
        .hexpand(true)
        .auto_render(false)
        .has_depth_buffer(false)
        .has_stencil_buffer(false)
        .build();
    video_area.set_size_request(-1, 300);
    overlay.set_child(Some(&video_area));

    let crop_overlay_area = gtk::DrawingArea::builder()
        .hexpand(true)
        .vexpand(true)
        .build();
    crop_overlay_area.set_can_target(true);
    overlay.add_overlay(&crop_overlay_area);
    video_panel.append(&overlay);

    let segment_overlay = gtk::DrawingArea::builder()
        .hexpand(true)
        .can_target(true)
        .build();
    let transport_component =
        BigVideoTransport::new(BigVideoTransportSpec::editor(BigVideoTransportLabels {
            timeline: text.timeline,
            play_pause: text.play_pause,
            mark_point: text.mark_point,
            seek_back: text.seek_back,
            seek_forward: text.seek_forward,
            previous_frame: text.previous_frame,
            next_frame: text.next_frame,
            previous: text.previous,
            next: text.next,
            fullscreen: text.fullscreen,
            playback_position: text.playback_position,
            playback_duration: text.playback_duration,
        }));
    transport_component
        .slider_overlay()
        .add_overlay(&segment_overlay);
    let transport_overlay_box = transport_component.root().clone();
    let timeline = transport_component.timeline().clone();
    let mark_button = transport_component.mark_button().clone();
    let seek_back_button = transport_component.seek_back_button().clone();
    let frame_back_button = transport_component.frame_back_button().clone();
    let play_button = transport_component.play_button().clone();
    let frame_fwd_button = transport_component.frame_forward_button().clone();
    let seek_fwd_button = transport_component.seek_forward_button().clone();

    let aux_controls = BigVideoAuxControls::new(
        BigVideoAuxControlSpec::new(
            BigVideoAuxControlLabels {
                playback_speed: text.playback_speed,
                volume: text.volume,
                audio_track: text.audio_track,
                subtitle_track: text.subtitle_track,
                no_tracks_loaded: text.no_tracks_loaded,
                no_subtitles: text.no_subtitles,
            },
            speed_labels,
        )
        .default_speed_index(3)
        .volume_range(100, 150),
    );
    let speed_button = aux_controls.speed_button().clone();
    let speed_box = aux_controls.speed_box().clone();
    let volume_button = aux_controls.volume_button().clone();
    let volume_scale = aux_controls.volume_scale().clone();
    let audio_track_button = aux_controls.audio_track_button().clone();
    let subtitle_track_button = aux_controls.subtitle_track_button().clone();
    let audio_track_box = aux_controls.audio_track_box().clone();
    let subtitle_track_box = aux_controls.subtitle_track_box().clone();

    let fullscreen_button = transport_component.fullscreen_button().clone();
    let controls = transport_component.controls_box();
    controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    controls.append(&speed_button);
    controls.append(&volume_button);
    controls.append(&audio_track_button);
    controls.append(&subtitle_track_button);
    controls.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    controls.append(&fullscreen_button);

    let position_label = transport_component.position_label().clone();
    position_label.set_label("0:00:00.000 / 0:00:00.000");
    let frame_label = transport_component.trailing_label().clone();
    frame_label.set_label("");
    overlay.add_overlay(&transport_overlay_box);

    let bottom_toolbar = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(12)
        .margin_end(12)
        .margin_top(4)
        .margin_bottom(8)
        .css_classes(["toolbar"])
        .build();
    let crop_controls = BigVideoCropControls::new(BigVideoCropLabels {
        left: text.crop_left,
        right: text.crop_right,
        top: text.crop_top,
        bottom: text.crop_bottom,
        reset_crop: text.reset_crop,
        show_grid_overlay: text.show_grid_overlay,
    });
    let crop_left = crop_controls.left_spin().clone();
    let crop_right = crop_controls.right_spin().clone();
    let crop_top = crop_controls.top_spin().clone();
    let crop_bottom = crop_controls.bottom_spin().clone();
    let crop_reset_button = crop_controls.reset_button().clone();
    let grid_button = crop_controls.grid_button().clone();
    bottom_toolbar.append(crop_controls.root());
    bottom_toolbar.append(&gtk::Separator::new(gtk::Orientation::Vertical));

    let media_metadata_grid = BigMediaMetadataGrid::new(BigMediaMetadataLabels {
        resolution: text.resolution,
        codec: text.codec,
        size: text.size,
        duration: text.duration,
    });
    let media_metadata_resolution = media_metadata_grid.resolution_value().clone();
    let media_metadata_codec = media_metadata_grid.codec_value().clone();
    let media_metadata_file_size = media_metadata_grid.size_value().clone();
    let media_metadata_duration = media_metadata_grid.duration_value().clone();
    bottom_toolbar.append(media_metadata_grid.root());
    video_panel.append(&bottom_toolbar);

    BigVideoEditorPanelBuild {
        video_panel,
        overlay,
        video_area,
        crop_overlay_area,
        segment_overlay,
        transport_overlay_box,
        timeline,
        position_label,
        frame_label,
        play_button,
        mark_button,
        speed_button,
        speed_box,
        volume_button,
        volume_scale,
        fullscreen_button,
        audio_track_button,
        subtitle_track_button,
        audio_track_box,
        subtitle_track_box,
        seek_back_button,
        frame_back_button,
        frame_fwd_button,
        seek_fwd_button,
        crop_left,
        crop_right,
        crop_top,
        crop_bottom,
        crop_reset_button,
        grid_button,
        media_metadata_resolution,
        media_metadata_codec,
        media_metadata_file_size,
        media_metadata_duration,
    }
}
