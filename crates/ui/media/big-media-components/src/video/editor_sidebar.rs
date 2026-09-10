//! Reusable video editor sidebar: adjustments, transform, audio and trim controls.

#![allow(clippy::needless_borrows_for_generic_args)]

use adw::prelude::*;
use big_relm4_components::feedback::tooltip::icon_button;
use big_relm4_components::input::dropdown_row::{BigDropdownRow, BigDropdownRowSpec};
use big_relm4_components::input::slider_row::{BigSliderRow, BigSliderRowSpec};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::list::button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec};
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
use relm4::gtk;

/// Translated strings used by [`build_video_editor_sidebar`].
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigVideoEditorSidebarText {
    pub reset: String,
    pub reset_brightness: String,
    pub reset_contrast: String,
    pub reset_saturation: String,
    pub reset_hue: String,
    pub video_adjustments: String,
    pub brightness: String,
    pub contrast: String,
    pub saturation: String,
    pub hue: String,
    pub transform: String,
    pub rotate_left: String,
    pub rotate_right: String,
    pub flip_horizontal: String,
    pub flip_vertical: String,
    pub reset_transform: String,
    pub audio_settings: String,
    pub audio_settings_subtitle: String,
    pub trim_segments: String,
    pub segments_placeholder: String,
    pub add_segment: String,
    pub clear_segments: String,
    pub output_mode: String,
    pub output_join: String,
    pub output_separate: String,
}

impl BigVideoEditorSidebarText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(mut text: impl FnMut(&str) -> String) -> Self {
        Self {
            reset: text("Reset"),
            reset_brightness: text("Reset brightness"),
            reset_contrast: text("Reset contrast"),
            reset_saturation: text("Reset saturation"),
            reset_hue: text("Reset hue"),
            video_adjustments: text("Video Adjustments"),
            brightness: text("Brightness"),
            contrast: text("Contrast"),
            saturation: text("Saturation"),
            hue: text("Hue"),
            transform: text("Transform"),
            rotate_left: text("Rotate 90 left"),
            rotate_right: text("Rotate 90 right"),
            flip_horizontal: text("Flip horizontal"),
            flip_vertical: text("Flip vertical"),
            reset_transform: text("Reset transform"),
            audio_settings: text("Audio Settings"),
            audio_settings_subtitle: text("Noise reduction, gate, normalize"),
            trim_segments: text("Trim Segments"),
            segments_placeholder: text("Press M or use the mark button to add segments"),
            add_segment: text("Add segment manually"),
            clear_segments: text("Clear all segments"),
            output_mode: text("Output Mode"),
            output_join: text("Join segments into one file"),
            output_separate: text("Save each segment as separate file"),
        }
    }
}

/// Widgets returned by [`build_video_editor_sidebar`].
///
/// Apps own every widget; signal handlers drive adjustment, transform,
/// segment, and output-mode state.
#[allow(missing_docs)]
pub struct BigVideoEditorSidebarBuild {
    pub sidebar_scroll: gtk::ScrolledWindow,
    pub sidebar_box: gtk::Box,
    pub brightness_scale: gtk::Scale,
    pub brightness_reset: gtk::Button,
    pub contrast_scale: gtk::Scale,
    pub contrast_reset: gtk::Button,
    pub saturation_scale: gtk::Scale,
    pub saturation_reset: gtk::Button,
    pub hue_scale: gtk::Scale,
    pub hue_reset: gtk::Button,
    pub rot_left_button: gtk::Button,
    pub rot_right_button: gtk::Button,
    pub flip_h_button: gtk::Button,
    pub flip_v_button: gtk::Button,
    pub reset_transform_button: gtk::Button,
    pub audio_group: adw::PreferencesGroup,
    pub audio_row: adw::ActionRow,
    pub segments_placeholder: adw::ActionRow,
    pub segments_listbox: gtk::ListBox,
    pub segments_list_row: adw::ActionRow,
    pub add_segment_button: gtk::Button,
    pub clear_segs_button: gtk::Button,
    pub output_dropdown: gtk::DropDown,
}

/// Build the video editor sidebar with adjustment, transform, audio, and trim groups.
#[must_use]
pub fn build_video_editor_sidebar(text: BigVideoEditorSidebarText) -> BigVideoEditorSidebarBuild {
    let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 12);
    sidebar.set_margin_start(6);
    sidebar.set_margin_end(6);
    sidebar.set_margin_top(6);
    sidebar.set_margin_bottom(6);
    sidebar.set_size_request(300, -1);

    let adjust_group = BigPreferencesGroupSpec::new(&text.video_adjustments).build();
    let (brightness_row, brightness_scale, brightness_reset) = make_slider_row(
        &text.brightness,
        -1.0,
        1.0,
        0.0,
        0.05,
        &text.reset_brightness,
    );
    let (contrast_row, contrast_scale, contrast_reset) =
        make_slider_row(&text.contrast, 0.0, 2.0, 1.0, 0.05, &text.reset_contrast);
    let (saturation_row, saturation_scale, saturation_reset) = make_slider_row(
        &text.saturation,
        0.0,
        2.0,
        1.0,
        0.05,
        &text.reset_saturation,
    );
    let (hue_row, hue_scale, hue_reset) =
        make_slider_row(&text.hue, -1.0, 1.0, 0.0, 0.05, &text.reset_hue);
    adjust_group.add(&brightness_row);
    adjust_group.add(&contrast_row);
    adjust_group.add(&saturation_row);
    adjust_group.add(&hue_row);
    sidebar.append(&adjust_group);

    let transform_group = BigPreferencesGroupSpec::new(&text.transform).build();
    let transform_row = BigButtonStripRow::new(BigButtonStripRowSpec::new(&text.transform));
    let rot_left_button = icon_button(
        "object-rotate-left-symbolic",
        &text.rotate_left,
        &["flat", "circular"],
    );
    let rot_right_button = icon_button(
        "object-rotate-right-symbolic",
        &text.rotate_right,
        &["flat", "circular"],
    );
    let flip_h_button = icon_button(
        "object-flip-horizontal-symbolic",
        &text.flip_horizontal,
        &["flat", "circular"],
    );
    let flip_v_button = icon_button(
        "object-flip-vertical-symbolic",
        &text.flip_vertical,
        &["flat", "circular"],
    );
    let reset_transform_button = icon_button(
        "edit-undo-symbolic",
        &text.reset_transform,
        &["flat", "circular"],
    );
    transform_row.append(&rot_left_button);
    transform_row.append(&rot_right_button);
    transform_row.append(&flip_h_button);
    transform_row.append(&flip_v_button);
    transform_row.append(&reset_transform_button);
    transform_group.add(transform_row.root());
    sidebar.append(&transform_group);

    let audio_group = BigPreferencesGroupSpec::new(&text.audio_settings).build();
    let audio_row = BigNavigationRow::new(
        BigNavigationRowSpec::new(&text.audio_settings)
            .subtitle(&text.audio_settings_subtitle)
            .prefix_icon_name("audio-volume-high-symbolic"),
    )
    .into_root();
    audio_group.add(&audio_row);
    sidebar.append(&audio_group);

    let trim_group = BigPreferencesGroupSpec::new(&text.trim_segments).build();
    trim_group.set_vexpand(true);
    let segments_placeholder = BigDetailRow::new(
        BigDetailRowSpec::new(&text.segments_placeholder).prefix_icon_name("bookmark-new-symbolic"),
    )
    .into_root();
    trim_group.add(&segments_placeholder);

    let segments_listbox = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let segments_scroll = gtk::ScrolledWindow::builder()
        .propagate_natural_height(true)
        .vexpand(true)
        .child(&segments_listbox)
        .build();
    let segments_list_row = adw::ActionRow::new();
    segments_list_row.set_child(Some(&segments_scroll));
    segments_list_row.set_visible(false);
    trim_group.add(&segments_list_row);

    let seg_actions_row = adw::ActionRow::new();
    let seg_actions_box = gtk::Box::builder()
        .spacing(8)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let add_segment_button = icon_button(
        "list-add-symbolic",
        &text.add_segment,
        &["circular", "suggested-action"],
    );
    let clear_segs_button = icon_button(
        "user-trash-symbolic",
        &text.clear_segments,
        &["circular", "destructive-action"],
    );
    seg_actions_box.append(&add_segment_button);
    seg_actions_box.append(&clear_segs_button);
    seg_actions_row.add_suffix(&seg_actions_box);
    trim_group.add(&seg_actions_row);

    let output = BigDropdownRow::new(BigDropdownRowSpec::new(
        &text.output_mode,
        [&text.output_join, &text.output_separate],
    ));
    let output_row = output.root().clone();
    let output_dropdown = output.dropdown().clone();
    trim_group.add(&output_row);
    sidebar.append(&trim_group);

    let sidebar_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&sidebar)
        .build();

    BigVideoEditorSidebarBuild {
        sidebar_scroll,
        sidebar_box: sidebar,
        brightness_scale,
        brightness_reset,
        contrast_scale,
        contrast_reset,
        saturation_scale,
        saturation_reset,
        hue_scale,
        hue_reset,
        rot_left_button,
        rot_right_button,
        flip_h_button,
        flip_v_button,
        reset_transform_button,
        audio_group,
        audio_row,
        segments_placeholder,
        segments_listbox,
        segments_list_row,
        add_segment_button,
        clear_segs_button,
        output_dropdown,
    }
}

fn make_slider_row(
    title: &str,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
    reset_label: &str,
) -> (adw::ActionRow, gtk::Scale, gtk::Button) {
    BigSliderRow::new(BigSliderRowSpec::new(
        title,
        min,
        max,
        default,
        step,
        reset_label,
    ))
    .into_parts()
}

#[cfg(test)]
mod tests {
    use super::BigVideoEditorSidebarText;

    #[test]
    fn adjustment_reset_labels_are_specific() {
        let text = BigVideoEditorSidebarText::translated(str::to_owned);

        assert_eq!(text.reset_brightness, "Reset brightness");
        assert_eq!(text.reset_contrast, "Reset contrast");
        assert_eq!(text.reset_saturation, "Reset saturation");
        assert_eq!(text.reset_hue, "Reset hue");
    }
}
