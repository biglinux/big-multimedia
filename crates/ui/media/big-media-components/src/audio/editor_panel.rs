//! Shared audio editor panel.

use adw::prelude::*;

use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use big_relm4_components::list::detail_row::{BigDetailRow, BigDetailRowSpec};

const PANEL_VERTICAL_SPACING: i32 = 12;
const PANEL_VERTICAL_MARGIN: i32 = 12;
const WAVEFORM_CONTENT_HEIGHT: i32 = 220;
const TIMELINE_MINIMUM_VALUE: f64 = 0.0;
const TIMELINE_MAXIMUM_VALUE: f64 = 1.0;
const CONTROLS_VERTICAL_SPACING: i32 = 0;
const TIME_ROW_HORIZONTAL_MARGIN: i32 = 8;
const TIME_ROW_MARGIN_TOP: i32 = 2;
const LEFT_BUTTONS_SPACING: i32 = 4;
const CENTER_BUTTONS_SPACING: i32 = 8;
const RIGHT_BUTTONS_SPACING: i32 = 4;
const BUTTONS_ROW_HORIZONTAL_MARGIN: i32 = 12;
const BUTTONS_ROW_VERTICAL_MARGIN: i32 = 4;
const SEGMENTS_GROUP_HORIZONTAL_MARGIN: i32 = 12;
const SEGMENTS_SCROLL_MAX_CONTENT_HEIGHT: i32 = 260;
const CUT_CONTENT_VERTICAL_SPACING: i32 = 12;
const CUT_REVEALER_TRANSITION_DURATION_MS: u32 = 200;
const HOVER_BOX_SPACING: i32 = 2;
const VALUE_LABEL_XALIGN: f32 = 0.0;
const SUGGESTED_ICON_BUTTON_CLASSES: &[&str] = &["suggested-action", "circular"];
const FLAT_ICON_BUTTON_CLASSES: &[&str] = &["flat", "circular"];
const VALUE_LABEL_CLASSES: &[&str] = &["osd-value-label"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EditorIconButtonSpec {
    icon_name: String,
    label: String,
    css_classes: &'static [&'static str],
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EditorIconToggleButtonSpec {
    icon_name: String,
    label: String,
    is_active: bool,
    css_classes: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq)]
struct EditorValueLabelSpec {
    label: String,
    width_chars: i32,
    xalign: f32,
    css_classes: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EditorHoverBoxSpec {
    orientation: gtk::Orientation,
    spacing: i32,
}

impl Default for EditorValueLabelSpec {
    fn default() -> Self {
        Self {
            label: String::new(),
            width_chars: 0,
            xalign: 0.0,
            css_classes: &[],
        }
    }
}

impl Default for EditorHoverBoxSpec {
    fn default() -> Self {
        Self {
            orientation: gtk::Orientation::Vertical,
            spacing: 0,
        }
    }
}

/// Translatable labels used by [`build_audio_editor_panel`].
///
/// Build with [`BigAudioEditorPanelText::translated`] to source the
/// strings through any translation callback.
#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigAudioEditorPanelText {
    pub apply: String,
    pub convert_this_file: String,
    pub play_pause: String,
    pub previous_track: String,
    pub next_track: String,
    pub play_only_selected_area: String,
    pub auto_advance_to_next_track: String,
    pub equalizer: String,
    pub clear_all_segments: String,
    pub volume: String,
    pub playback_speed: String,
    pub waveform_zoom: String,
    pub audio_settings: String,
    pub audio_timeline: String,
    pub trim_segments: String,
    pub mark_segments_hint: String,
}

impl BigAudioEditorPanelText {
    /// Translate every label through `text` and return a populated set.
    #[must_use]
    pub fn translated(text: impl Fn(&str) -> String) -> Self {
        Self {
            apply: text("Apply"),
            convert_this_file: text("Convert this file"),
            play_pause: text("Play/Pause"),
            previous_track: text("Previous track"),
            next_track: text("Next track"),
            play_only_selected_area: text("Play only selected area"),
            auto_advance_to_next_track: text("Auto-advance to next track"),
            equalizer: text("Equalizer"),
            clear_all_segments: text("Clear all segments"),
            volume: text("Volume"),
            playback_speed: text("Playback speed"),
            waveform_zoom: text("Waveform zoom"),
            audio_settings: text("Audio Settings"),
            audio_timeline: text("Audio timeline"),
            trim_segments: text("Trim Segments"),
            mark_segments_hint: text("Press the mark button to create segments"),
        }
    }
}

/// Widgets returned by [`build_audio_editor_panel`].
///
/// Apps own every field after the call and wire signal handlers to drive
/// playback, segment marking, and the audio settings dialog.
#[allow(missing_docs)]
pub struct BigAudioEditorPanelBuild {
    pub toolbar: adw::ToolbarView,
    pub header: adw::HeaderBar,
    pub root: gtk::Box,
    pub controls: gtk::Box,
    pub trim_group: adw::PreferencesGroup,
    pub play_button: gtk::Button,
    pub prev_button: gtk::Button,
    pub next_button: gtk::Button,
    pub play_selection_button: gtk::ToggleButton,
    pub auto_advance_button: gtk::ToggleButton,
    pub eq_toggle_button: gtk::Button,
    pub clear_button: gtk::Button,
    pub settings_button: gtk::Button,
    pub volume_button: gtk::Button,
    pub volume_label: gtk::Label,
    pub speed_button: gtk::Button,
    pub speed_label: gtk::Label,
    pub zoom_button: gtk::Button,
    pub zoom_label: gtk::Label,
    pub waveform: gtk::DrawingArea,
    pub timeline: gtk::Scale,
    pub position_label: gtk::Label,
    pub duration_label: gtk::Label,
    pub segments_list: gtk::ListBox,
    pub segments_placeholder: adw::ActionRow,
    pub segments_row: adw::ActionRow,
    pub cut_revealer: gtk::Revealer,
    pub apply_button: gtk::Button,
    pub convert_button: gtk::Button,
}

/// Build the audio editor panel widget tree.
///
/// Returns every interactive widget in a [`BigAudioEditorPanelBuild`] so
/// the caller can connect signals to its own model.
#[must_use]
pub fn build_audio_editor_panel(text: BigAudioEditorPanelText) -> BigAudioEditorPanelBuild {
    let header = adw::HeaderBar::new();

    let apply_button = gtk::Button::builder()
        .label(&text.apply)
        .css_classes(["suggested-action"])
        .build();
    let convert_button = gtk::Button::builder()
        .label(&text.convert_this_file)
        .css_classes(["suggested-action"])
        .build();
    header.pack_end(&convert_button);
    header.pack_end(&apply_button);

    let root = gtk::Box::new(gtk::Orientation::Vertical, PANEL_VERTICAL_SPACING);
    root.set_margin_top(PANEL_VERTICAL_MARGIN);
    root.set_margin_bottom(PANEL_VERTICAL_MARGIN);

    let play_button = icon_button("media-playback-start-symbolic", &text.play_pause, true);
    let prev_button = icon_button("media-skip-backward-symbolic", &text.previous_track, false);
    let next_button = icon_button("media-skip-forward-symbolic", &text.next_track, false);
    let play_selection_button = icon_toggle_button(
        "selection-mode-symbolic",
        &text.play_only_selected_area,
        false,
    );
    let auto_advance_button = icon_toggle_button(
        "media-playlist-consecutive-symbolic",
        &text.auto_advance_to_next_track,
        true,
    );
    let eq_toggle_button = icon_button("media-eq-symbolic", &text.equalizer, false);
    let clear_button = icon_button("user-trash-symbolic", &text.clear_all_segments, false);

    let volume_button = icon_button("audio-volume-high-symbolic", &text.volume, false);
    let volume_label = value_label("100", 3);
    let volume_hover_box = labeled_hover_box(&volume_button, &volume_label);

    let speed_button = icon_button("speedometer-symbolic", &text.playback_speed, false);
    let speed_label = value_label("1.00x", 5);
    let speed_hover_box = labeled_hover_box(&speed_button, &speed_label);

    let zoom_button = icon_button("system-search-symbolic", &text.waveform_zoom, false);
    let zoom_label = value_label("1.0x", 4);
    let zoom_hover_box = labeled_hover_box(&zoom_button, &zoom_label);

    let settings_button = icon_button("audio-x-generic-symbolic", &text.audio_settings, false);

    let waveform = gtk::DrawingArea::builder()
        .vexpand(false)
        .hexpand(true)
        .build();
    waveform.set_content_height(WAVEFORM_CONTENT_HEIGHT);
    waveform.set_can_focus(true);

    let timeline = gtk::Scale::builder()
        .orientation(gtk::Orientation::Horizontal)
        .draw_value(false)
        .hexpand(true)
        .css_classes(["seek-scale"])
        .build();
    timeline.update_property(&[gtk::accessible::Property::Label(&text.audio_timeline)]);
    timeline.set_range(TIMELINE_MINIMUM_VALUE, TIMELINE_MAXIMUM_VALUE);

    let controls = gtk::Box::new(gtk::Orientation::Vertical, CONTROLS_VERTICAL_SPACING);
    let time_row = gtk::CenterBox::new();
    time_row.set_margin_start(TIME_ROW_HORIZONTAL_MARGIN);
    time_row.set_margin_end(TIME_ROW_HORIZONTAL_MARGIN);
    time_row.set_margin_top(TIME_ROW_MARGIN_TOP);
    let position_label = gtk::Label::builder()
        .label("0:00")
        .css_classes(["time-label"])
        .build();
    let duration_label = gtk::Label::builder()
        .label("0:00")
        .css_classes(["time-label"])
        .build();
    time_row.set_start_widget(Some(&position_label));
    time_row.set_end_widget(Some(&duration_label));
    controls.append(&time_row);
    controls.append(&timeline);

    let buttons_row = gtk::CenterBox::new();
    let left_buttons = gtk::Box::new(gtk::Orientation::Horizontal, LEFT_BUTTONS_SPACING);
    left_buttons.append(&play_selection_button);
    left_buttons.append(&auto_advance_button);
    left_buttons.append(&eq_toggle_button);
    left_buttons.append(&volume_hover_box);
    left_buttons.append(&speed_hover_box);
    left_buttons.append(&zoom_hover_box);
    let center_buttons = gtk::Box::new(gtk::Orientation::Horizontal, CENTER_BUTTONS_SPACING);
    center_buttons.set_valign(gtk::Align::Center);
    center_buttons.append(&prev_button);
    center_buttons.append(&play_button);
    center_buttons.append(&next_button);
    let right_buttons = gtk::Box::new(gtk::Orientation::Horizontal, RIGHT_BUTTONS_SPACING);
    right_buttons.append(&clear_button);
    right_buttons.append(&settings_button);
    buttons_row.set_start_widget(Some(&left_buttons));
    buttons_row.set_center_widget(Some(&center_buttons));
    buttons_row.set_end_widget(Some(&right_buttons));
    buttons_row.set_margin_start(BUTTONS_ROW_HORIZONTAL_MARGIN);
    buttons_row.set_margin_end(BUTTONS_ROW_HORIZONTAL_MARGIN);
    buttons_row.set_margin_top(BUTTONS_ROW_VERTICAL_MARGIN);
    buttons_row.set_margin_bottom(BUTTONS_ROW_VERTICAL_MARGIN);
    controls.append(&buttons_row);

    let trim_group = BigPreferencesGroupSpec::new(&text.trim_segments).build();
    let segments_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(SEGMENTS_SCROLL_MAX_CONTENT_HEIGHT)
        .propagate_natural_height(true)
        .child(&segments_list)
        .build();
    let segments_row = adw::ActionRow::new();
    segments_row.set_child(Some(&scroll));
    segments_row.set_visible(false);
    let segments_placeholder = BigDetailRow::new(
        BigDetailRowSpec::new(&text.mark_segments_hint).prefix_icon_name("bookmark-new-symbolic"),
    )
    .into_root();
    trim_group.add(&segments_placeholder);
    trim_group.add(&segments_row);
    trim_group.set_margin_start(SEGMENTS_GROUP_HORIZONTAL_MARGIN);
    trim_group.set_margin_end(SEGMENTS_GROUP_HORIZONTAL_MARGIN);

    let cut_content = gtk::Box::new(gtk::Orientation::Vertical, CUT_CONTENT_VERTICAL_SPACING);
    cut_content.append(&waveform);
    cut_content.append(&trim_group);
    let cut_revealer = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideDown)
        .transition_duration(CUT_REVEALER_TRANSITION_DURATION_MS)
        .reveal_child(false)
        .child(&cut_content)
        .build();
    root.append(&cut_revealer);
    root.append(&controls);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&root));

    BigAudioEditorPanelBuild {
        toolbar,
        header,
        root,
        controls,
        trim_group,
        play_button,
        prev_button,
        next_button,
        play_selection_button,
        auto_advance_button,
        eq_toggle_button,
        clear_button,
        settings_button,
        volume_button,
        volume_label,
        speed_button,
        speed_label,
        zoom_button,
        zoom_label,
        waveform,
        timeline,
        position_label,
        duration_label,
        segments_list,
        segments_placeholder,
        segments_row,
        cut_revealer,
        apply_button,
        convert_button,
    }
}

fn icon_button(icon_name: &str, label: &str, suggested: bool) -> gtk::Button {
    let spec = editor_icon_button_spec(icon_name, label, suggested);
    tooltip::icon_button(&spec.icon_name, &spec.label, spec.css_classes)
}

fn icon_toggle_button(icon_name: &str, label: &str, active: bool) -> gtk::ToggleButton {
    let spec = editor_icon_toggle_button_spec(icon_name, label, active);
    tooltip::icon_toggle_button(
        &spec.icon_name,
        &spec.label,
        spec.is_active,
        spec.css_classes,
    )
}

fn value_label(label: &str, width_chars: i32) -> gtk::Label {
    let spec = editor_value_label_spec(label, width_chars);
    gtk::Label::builder()
        .label(&spec.label)
        .width_chars(spec.width_chars)
        .xalign(spec.xalign)
        .css_classes(spec.css_classes)
        .build()
}

fn labeled_hover_box(button: &impl IsA<gtk::Widget>, label: &gtk::Label) -> gtk::Box {
    let spec = editor_hover_box_spec();
    let hover_box = gtk::Box::new(spec.orientation, spec.spacing);
    hover_box.append(button);
    hover_box.append(label);
    hover_box
}

fn editor_icon_button_spec(icon_name: &str, label: &str, suggested: bool) -> EditorIconButtonSpec {
    EditorIconButtonSpec {
        icon_name: icon_name.to_string(),
        label: label.to_string(),
        css_classes: if suggested {
            SUGGESTED_ICON_BUTTON_CLASSES
        } else {
            FLAT_ICON_BUTTON_CLASSES
        },
    }
}

fn editor_icon_toggle_button_spec(
    icon_name: &str,
    label: &str,
    is_active: bool,
) -> EditorIconToggleButtonSpec {
    EditorIconToggleButtonSpec {
        icon_name: icon_name.to_string(),
        label: label.to_string(),
        is_active,
        css_classes: FLAT_ICON_BUTTON_CLASSES,
    }
}

fn editor_value_label_spec(label: &str, width_chars: i32) -> EditorValueLabelSpec {
    EditorValueLabelSpec {
        label: label.to_string(),
        width_chars,
        xalign: VALUE_LABEL_XALIGN,
        css_classes: VALUE_LABEL_CLASSES,
    }
}

fn editor_hover_box_spec() -> EditorHoverBoxSpec {
    EditorHoverBoxSpec {
        orientation: gtk::Orientation::Horizontal,
        spacing: HOVER_BOX_SPACING,
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{
        BUTTONS_ROW_HORIZONTAL_MARGIN, BUTTONS_ROW_VERTICAL_MARGIN, BigAudioEditorPanelText,
        CENTER_BUTTONS_SPACING, CONTROLS_VERTICAL_SPACING, CUT_CONTENT_VERTICAL_SPACING,
        CUT_REVEALER_TRANSITION_DURATION_MS, FLAT_ICON_BUTTON_CLASSES, HOVER_BOX_SPACING,
        LEFT_BUTTONS_SPACING, PANEL_VERTICAL_MARGIN, PANEL_VERTICAL_SPACING, RIGHT_BUTTONS_SPACING,
        SEGMENTS_GROUP_HORIZONTAL_MARGIN, SEGMENTS_SCROLL_MAX_CONTENT_HEIGHT,
        SUGGESTED_ICON_BUTTON_CLASSES, TIME_ROW_HORIZONTAL_MARGIN, TIME_ROW_MARGIN_TOP,
        TIMELINE_MAXIMUM_VALUE, TIMELINE_MINIMUM_VALUE, VALUE_LABEL_CLASSES, VALUE_LABEL_XALIGN,
        WAVEFORM_CONTENT_HEIGHT, editor_hover_box_spec, editor_icon_button_spec,
        editor_icon_toggle_button_spec, editor_value_label_spec,
    };

    #[test]
    fn translated_text_routes_every_visible_message_through_callback() {
        let translated_messages = RefCell::new(Vec::new());
        let text = BigAudioEditorPanelText::translated(|message| {
            translated_messages.borrow_mut().push(message.to_string());
            format!("translated:{message}")
        });

        assert_eq!(text.apply, "translated:Apply");
        assert_eq!(text.convert_this_file, "translated:Convert this file");
        assert_eq!(text.play_pause, "translated:Play/Pause");
        assert_eq!(text.audio_settings, "translated:Audio Settings");
        assert_eq!(text.audio_timeline, "translated:Audio timeline");
        assert_eq!(
            text.mark_segments_hint,
            "translated:Press the mark button to create segments"
        );

        let translated_messages = translated_messages.into_inner();
        assert_eq!(translated_messages.len(), 16);
        assert!(translated_messages.contains(&"Playback speed".to_string()));
        assert!(translated_messages.contains(&"Waveform zoom".to_string()));
        assert!(translated_messages.contains(&"Trim Segments".to_string()));
    }

    #[test]
    fn icon_button_spec_selects_icon_label_and_css_classes() {
        let play_spec =
            editor_icon_button_spec("media-playback-start-symbolic", "Play/Pause", true);

        assert_eq!(play_spec.icon_name, "media-playback-start-symbolic");
        assert_eq!(play_spec.label, "Play/Pause");
        assert_eq!(play_spec.css_classes, SUGGESTED_ICON_BUTTON_CLASSES);

        let secondary_spec =
            editor_icon_button_spec("media-skip-forward-symbolic", "Next track", false);
        assert_eq!(secondary_spec.icon_name, "media-skip-forward-symbolic");
        assert_eq!(secondary_spec.label, "Next track");
        assert_eq!(secondary_spec.css_classes, FLAT_ICON_BUTTON_CLASSES);
    }

    #[test]
    fn toggle_button_spec_keeps_initial_state_and_a11y_label() {
        let active_spec = editor_icon_toggle_button_spec(
            "media-playlist-consecutive-symbolic",
            "Auto-advance to next track",
            true,
        );

        assert_eq!(active_spec.icon_name, "media-playlist-consecutive-symbolic");
        assert_eq!(active_spec.label, "Auto-advance to next track");
        assert!(active_spec.is_active);
        assert_eq!(active_spec.css_classes, FLAT_ICON_BUTTON_CLASSES);

        let inactive_spec = editor_icon_toggle_button_spec(
            "selection-mode-symbolic",
            "Play only selected area",
            false,
        );
        assert_eq!(inactive_spec.icon_name, "selection-mode-symbolic");
        assert_eq!(inactive_spec.label, "Play only selected area");
        assert!(!inactive_spec.is_active);
    }

    #[test]
    fn value_label_spec_matches_compact_osd_controls() {
        let volume_spec = editor_value_label_spec("100", 3);
        assert_eq!(volume_spec.label, "100");
        assert_eq!(volume_spec.width_chars, 3);
        assert_eq!(volume_spec.xalign, VALUE_LABEL_XALIGN);
        assert_eq!(volume_spec.css_classes, VALUE_LABEL_CLASSES);

        let speed_spec = editor_value_label_spec("1.00x", 5);
        assert_eq!(speed_spec.label, "1.00x");
        assert_eq!(speed_spec.width_chars, 5);

        let zoom_spec = editor_value_label_spec("1.0x", 4);
        assert_eq!(zoom_spec.label, "1.0x");
        assert_eq!(zoom_spec.width_chars, 4);
    }

    #[test]
    fn hover_box_spec_keeps_button_and_value_inline() {
        let spec = editor_hover_box_spec();

        assert_eq!(spec.orientation, gtk::Orientation::Horizontal);
        assert_eq!(spec.spacing, HOVER_BOX_SPACING);
    }

    #[test]
    fn editor_panel_layout_constants_match_audio_editor_contract() {
        assert_eq!(PANEL_VERTICAL_SPACING, 12);
        assert_eq!(PANEL_VERTICAL_MARGIN, 12);
        assert_eq!(WAVEFORM_CONTENT_HEIGHT, 220);
        assert_eq!(TIMELINE_MINIMUM_VALUE, 0.0);
        assert_eq!(TIMELINE_MAXIMUM_VALUE, 1.0);
        assert_eq!(CONTROLS_VERTICAL_SPACING, 0);
        assert_eq!(TIME_ROW_HORIZONTAL_MARGIN, 8);
        assert_eq!(TIME_ROW_MARGIN_TOP, 2);
        assert_eq!(LEFT_BUTTONS_SPACING, 4);
        assert_eq!(CENTER_BUTTONS_SPACING, 8);
        assert_eq!(RIGHT_BUTTONS_SPACING, 4);
        assert_eq!(BUTTONS_ROW_HORIZONTAL_MARGIN, 12);
        assert_eq!(BUTTONS_ROW_VERTICAL_MARGIN, 4);
        assert_eq!(SEGMENTS_GROUP_HORIZONTAL_MARGIN, 12);
        assert_eq!(SEGMENTS_SCROLL_MAX_CONTENT_HEIGHT, 260);
        assert_eq!(CUT_CONTENT_VERTICAL_SPACING, 12);
        assert_eq!(CUT_REVEALER_TRANSITION_DURATION_MS, 200);
        assert_eq!(VALUE_LABEL_XALIGN, 0.0);
    }
}
