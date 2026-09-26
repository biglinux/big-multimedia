//! App-local picture and equalizer builders for the settings panel: the
//! "Video colour" choice up front and, under "More options", debounced
//! `BigIllustratedSliderRow`s plus rotate/flip/crop that drive the live
//! `Arc<MpvBackend>` directly.
//!
//! The equalizer reuses the shared
//! `big_media_runtime::shared::audio_effects::create_equalizer_dialog`
//! verbatim (the audio player drives the same dialog); only the
//! transient/size/present wiring is app-local.
//!
//! mpv is the source of truth for these live values (the equalizer persists
//! its bands through `Settings`).
//!
//! The pure slider-spec → range/default mapping is exercised by the unit tests
//! at the bottom (range monotonicity + default-in-range invariants).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::debounce::connect_debounced_adjustment;
use big_relm4_components::input::illustrated_slider_row::{
    BigIllustratedSliderRow, BigIllustratedSliderRowSpec,
};
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::config::video::RESOURCE_PREFIX;
use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use big_media_runtime::i18n::gettext_noop;

/// Debounce window for live slider→mpv updates.
const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CropMargins {
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
}

/// How the picture's colours are read (`MpvBackend::set_source_colours`),
/// in the order the dialog lists them.
const SOURCE_COLOURS: [&str; 4] = ["auto", "pq", "hlg", "sdr"];

#[derive(Clone, Default)]
pub(crate) struct VideoTransformState {
    hflip: Rc<Cell<bool>>,
    vflip: Rc<Cell<bool>>,
    crop_margins: Rc<RefCell<CropMargins>>,
    source_colours: Rc<Cell<usize>>,
}

impl VideoTransformState {
    pub(crate) fn hflip(&self) -> bool {
        self.hflip.get()
    }

    pub(crate) fn vflip(&self) -> bool {
        self.vflip.get()
    }

    fn set_hflip(&self, value: bool) {
        self.hflip.set(value);
    }

    fn set_vflip(&self, value: bool) {
        self.vflip.set(value);
    }

    pub(crate) fn set_flip(&self, mpv: &MpvBackend, horizontal: bool, vertical: bool) {
        self.set_hflip(horizontal);
        self.set_vflip(vertical);
        mpv.set_video_flip(horizontal, vertical);
    }

    pub(crate) fn crop_margins(&self) -> CropMargins {
        *self.crop_margins.borrow()
    }

    pub(crate) fn set_crop_margins(&self, margins: CropMargins) {
        self.crop_margins.replace(margins);
    }

    pub(crate) fn source_colours_index(&self) -> usize {
        self.source_colours.get()
    }

    pub(crate) fn select_source_colours(&self, mpv: &MpvBackend, index: usize) {
        if index < SOURCE_COLOURS.len() {
            self.source_colours.set(index);
            mpv.set_source_colours(SOURCE_COLOURS[index]);
        }
    }

    /// "auto" unless the user said how to read this file's colours.
    pub(crate) fn source_colours(&self) -> &'static str {
        SOURCE_COLOURS[self.source_colours.get()]
    }

    /// Back to trusting the file: a choice made for one recording would
    /// spoil the next file, which may describe its colours correctly.
    pub(crate) fn reset_source_colours(&self, mpv: &MpvBackend) {
        if self.source_colours.replace(0) != 0 {
            mpv.set_source_colours("auto");
        }
    }
}

/// Short names of [`SOURCE_COLOURS`], for the choice and its chip.
pub(crate) fn source_colour_names() -> [String; 4] {
    [
        i18n("Automatic"),
        i18n("HDR"),
        i18n("HDR (TV)"),
        i18n("Normal"),
    ]
}

/// Resource path prefix for the slider illustration SVGs.
fn illustration_prefix() -> String {
    format!("{RESOURCE_PREFIX}/illustrations/")
}

/// Seed value for a slider from a raw mpv property reading: clamp into the
/// slider's `[lower, upper]` so an out-of-range/uninitialised mpv value can
/// never push the adjustment past its bounds (which would silently re-apply a
/// clamped value to mpv on the first `value-changed`).
fn clamp_initial(raw: f64, lower: f64, upper: f64) -> f64 {
    raw.clamp(lower, upper)
}

fn normalize_rotation(degrees: i64) -> i64 {
    match degrees.rem_euclid(360) {
        1..=134 => 90,
        135..=224 => 180,
        225..=314 => 270,
        _ => 0,
    }
}

fn crop_filter_from_margins(width: i64, height: i64, margins: CropMargins) -> Option<String> {
    if width <= 2 || height <= 2 {
        return None;
    }
    let left = margins.left.clamp(0, width - 2);
    let right = margins.right.clamp(0, width - left - 2);
    let top = margins.top.clamp(0, height - 2);
    let bottom = margins.bottom.clamp(0, height - top - 2);
    if left == 0 && right == 0 && top == 0 && bottom == 0 {
        return None;
    }
    Some(format!(
        "{}x{}+{}+{}",
        width - left - right,
        height - top - bottom,
        left,
        top
    ))
}

/// One image slider: its name, the integer-percent range mpv uses for the
/// built-in video equalizer (-100..=100), the current value, and the setter.
struct ImageSliderSpec {
    title: &'static str,
    lower: f64,
    upper: f64,
    default: f64,
    initial: f64,
    step: f64,
    digits: i32,
    apply: fn(&MpvBackend, f64),
}

/// A ±10 s timing slider for the sound (`subtitles == false`) or the
/// subtitles, named for the symptom it fixes.
pub(super) fn build_delay_row(mpv: &Arc<MpvBackend>, subtitles: bool) -> gtk::Widget {
    let (title, subtitle, initial, apply): (_, _, _, fn(&MpvBackend, f64)) = if subtitles {
        (
            i18n("Subtitles early or late"),
            i18n("When the text shows before or after the speech. More = later"),
            mpv.get_property_f64("sub-delay"),
            |mpv, value| mpv.set_sub_delay(value),
        )
    } else {
        (
            i18n("Sound early or late"),
            i18n("When voices do not match the lips. More = later"),
            mpv.get_property_f64("audio-delay"),
            |mpv, value| mpv.set_audio_delay(value),
        )
    };
    let row = BigIllustratedSliderRow::new(
        BigIllustratedSliderRowSpec::new(
            illustration_prefix(),
            title,
            -10.0,
            10.0,
            0.0,
            clamp_initial(initial, -10.0, 10.0),
            0.1,
            i18n("Reset to zero"),
        )
        .subtitle(subtitle)
        .digits(1)
        .value_suffix(" s")
        .value_width_chars(7)
        .scale_width_request(80)
        .title_heading(true)
        .mark_label(Some("0")),
    );
    let (outer_box, adj, _, _, _) = row.into_parts();
    let mpv = Arc::clone(mpv);
    connect_debounced_adjustment(&adj, SLIDER_DEBOUNCE, move |value| apply(&mpv, value));
    outer_box.upcast()
}

/// Append image sliders to `content`, each debounced onto its mpv setter.
/// Named only: the page's title says what they are about.
fn add_image_group(
    content: &gtk::Box,
    rows: &[ImageSliderSpec],
    mpv: &Arc<MpvBackend>,
    wide: bool,
) {
    let prefix = illustration_prefix();
    let group = adw::PreferencesGroup::new();
    // Side by side when the panel is wide, stacked when it is narrow.
    let flow = reflowing_box(if wide { 2 } else { 1 });
    flow.set_min_children_per_line(if wide { 2 } else { 1 });
    for spec in rows {
        let row = BigIllustratedSliderRow::new(
            BigIllustratedSliderRowSpec::new(
                &prefix,
                i18n(spec.title),
                spec.lower,
                spec.upper,
                spec.default,
                clamp_initial(spec.initial, spec.lower, spec.upper),
                spec.step,
                i18n("Reset"),
            )
            .digits(spec.digits)
            .scale_width_request(80),
        );
        let mpv = Arc::clone(mpv);
        let apply = spec.apply;
        connect_debounced_adjustment(row.adjustment(), SLIDER_DEBOUNCE, move |value| {
            apply(&mpv, value);
        });
        flow.append(row.root());
    }
    unfocusable_children(&flow);
    group.add(&flow);
    content.append(&group);
}

/// A box that puts up to `per_line` children side by side as width allows.
fn reflowing_box(per_line: u32) -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .max_children_per_line(per_line)
        .column_spacing(12)
        .row_spacing(12)
        .build()
}

/// Tab moves between the controls, not the flow box's own cells.
fn unfocusable_children(flow: &gtk::FlowBox) {
    let mut child = flow.first_child();
    while let Some(cell) = child {
        cell.set_focusable(false);
        child = cell.next_sibling();
    }
}

/// "Video colour": how to read the picture's colours, as four plain
/// choices. A grey, washed-out picture is fixed here and by no slider.
fn build_source_colours_group(
    content: &gtk::Box,
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) {
    let group = adw::PreferencesGroup::new();
    let hints = [
        i18n("Uses what the file says"),
        i18n("For a picture that looks grey and washed out"),
        i18n("For some TV broadcasts (HLG)"),
        i18n("Plain colours (SDR)"),
    ];
    // Side-by-side choices: the picked one is highlighted.
    let flow = reflowing_box(4);
    flow.set_min_children_per_line(2);
    let mut first: Option<gtk::ToggleButton> = None;
    for (index, (name, hint)) in source_colour_names().into_iter().zip(hints).enumerate() {
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        texts.append(
            &gtk::Label::builder()
                .label(&name)
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
        texts.append(
            &gtk::Label::builder()
                .label(&hint)
                .xalign(0.0)
                .wrap(true)
                .lines(3)
                .css_classes(["caption"])
                .build(),
        );
        let choice = gtk::ToggleButton::builder()
            .child(&texts)
            .active(transform_state.source_colours_index() == index)
            .width_request(130)
            .css_classes(["colour-choice"])
            .build();
        choice.update_property(&[gtk::accessible::Property::Label(&format!("{name}: {hint}"))]);
        choice.set_group(first.as_ref());
        first.get_or_insert_with(|| choice.clone());
        choice.connect_toggled({
            let mpv = Arc::clone(mpv);
            let transform_state = transform_state.clone();
            move |choice| {
                if choice.is_active() {
                    transform_state.select_source_colours(&mpv, index);
                }
            }
        });
        flow.append(&choice);
    }
    unfocusable_children(&flow);
    group.add(&flow);
    content.append(&group);
}

fn build_transform_group(
    content: &gtk::Box,
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) {
    let group = BigPreferencesGroupSpec::new(i18n("Transform")).build();

    let rotation = Rc::new(Cell::new(normalize_rotation(mpv.get_video_rotate())));
    let rotation_row = adw::ActionRow::new();
    rotation_row.set_title(&i18n("Rotation"));
    let rotation_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    let rotation_label = gtk::Label::builder()
        .label(format!("{}°", rotation.get()))
        .width_chars(4)
        .xalign(1.0)
        .build();
    let rotate_left = gtk::Button::builder()
        .icon_name("object-rotate-left-symbolic")
        .css_classes(["flat"])
        .build();
    rotate_left.update_property(&[gtk::accessible::Property::Label(&i18n("Rotate Left"))]);
    let rotate_reset = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .css_classes(["flat"])
        .build();
    rotate_reset.update_property(&[gtk::accessible::Property::Label(&i18n("Reset Rotation"))]);
    let rotate_right = gtk::Button::builder()
        .icon_name("object-rotate-right-symbolic")
        .css_classes(["flat"])
        .build();
    rotate_right.update_property(&[gtk::accessible::Property::Label(&i18n("Rotate Right"))]);
    rotation_box.append(&rotation_label);
    rotation_box.append(&rotate_left);
    rotation_box.append(&rotate_reset);
    rotation_box.append(&rotate_right);
    rotation_row.add_suffix(&rotation_box);
    group.add(&rotation_row);

    let apply_rotation: Rc<dyn Fn(i64)> = {
        let mpv = Arc::clone(mpv);
        let rotation = rotation.clone();
        let rotation_label = rotation_label.downgrade();
        Rc::new(move |degrees| {
            let normalized = normalize_rotation(degrees);
            rotation.set(normalized);
            if let Some(label) = rotation_label.upgrade() {
                label.set_label(&format!("{normalized}°"));
            }
            mpv.set_video_rotate(normalized);
        })
    };
    rotate_left.connect_clicked({
        let rotation = rotation.clone();
        let apply_rotation = apply_rotation.clone();
        move |_| apply_rotation(rotation.get() - 90)
    });
    rotate_reset.connect_clicked({
        let apply_rotation = apply_rotation.clone();
        move |_| apply_rotation(0)
    });
    rotate_right.connect_clicked({
        let rotation = rotation.clone();
        let apply_rotation = apply_rotation.clone();
        move |_| apply_rotation(rotation.get() + 90)
    });

    let flip_row = adw::ActionRow::new();
    flip_row.set_title(&i18n("Flip"));
    let flip_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    let horizontal_flip_button = gtk::ToggleButton::builder()
        .label(i18n("Horizontal"))
        .active(transform_state.hflip())
        .css_classes(["flat"])
        .build();
    let vertical_flip_button = gtk::ToggleButton::builder()
        .label(i18n("Vertical"))
        .active(transform_state.vflip())
        .css_classes(["flat"])
        .build();
    flip_box.append(&horizontal_flip_button);
    flip_box.append(&vertical_flip_button);
    flip_row.add_suffix(&flip_box);
    group.add(&flip_row);

    horizontal_flip_button.connect_toggled({
        let mpv = Arc::clone(mpv);
        let transform_state = transform_state.clone();
        move |button| {
            transform_state.set_hflip(button.is_active());
            mpv.set_video_flip(transform_state.hflip(), transform_state.vflip());
        }
    });
    vertical_flip_button.connect_toggled({
        let mpv = Arc::clone(mpv);
        let transform_state = transform_state.clone();
        move |button| {
            transform_state.set_vflip(button.is_active());
            mpv.set_video_flip(transform_state.hflip(), transform_state.vflip());
        }
    });

    content.append(&group);
}

fn build_crop_group(
    content: &gtk::Box,
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) {
    let group = BigPreferencesGroupSpec::new(i18n("Crop")).build();
    let (width, height) = mpv.get_video_dimensions().unwrap_or((0, 0));
    group.set_sensitive(width > 2 && height > 2);

    let margins = transform_state.crop_margins();
    let left = crop_spin(&i18n("Left"), (width - 2).max(0), margins.left);
    let right = crop_spin(&i18n("Right"), (width - 2).max(0), margins.right);
    let top = crop_spin(&i18n("Top"), (height - 2).max(0), margins.top);
    let bottom = crop_spin(&i18n("Bottom"), (height - 2).max(0), margins.bottom);

    for (title, spin) in [
        (i18n("Left"), &left),
        (i18n("Right"), &right),
        (i18n("Top"), &top),
        (i18n("Bottom"), &bottom),
    ] {
        let row = adw::ActionRow::new();
        row.set_title(&title);
        row.add_suffix(spin);
        row.set_activatable_widget(Some(spin));
        group.add(&row);
    }

    let reset = gtk::Button::builder()
        .label(i18n("Reset crop"))
        .halign(gtk::Align::End)
        .css_classes(["flat"])
        .build();
    group.add(&reset);

    let updating = Rc::new(Cell::new(false));
    let apply_crop: Rc<dyn Fn()> = {
        let mpv = Arc::clone(mpv);
        let transform_state = transform_state.clone();
        let updating = updating.clone();
        let left = left.downgrade();
        let right = right.downgrade();
        let top = top.downgrade();
        let bottom = bottom.downgrade();
        Rc::new(move || {
            if updating.get() {
                return;
            }
            let (Some(left), Some(right), Some(top), Some(bottom)) = (
                left.upgrade(),
                right.upgrade(),
                top.upgrade(),
                bottom.upgrade(),
            ) else {
                return;
            };
            let margins = CropMargins {
                left: left.value() as i64,
                right: right.value() as i64,
                top: top.value() as i64,
                bottom: bottom.value() as i64,
            };
            transform_state.set_crop_margins(margins);
            if let Some(crop) = crop_filter_from_margins(width, height, margins) {
                mpv.set_video_crop(&crop);
            } else {
                mpv.set_video_crop("");
            }
        })
    };
    for spin in [&left, &right, &top, &bottom] {
        let apply_crop = apply_crop.clone();
        spin.connect_value_changed(move |_| apply_crop());
    }
    reset.connect_clicked({
        let mpv = Arc::clone(mpv);
        let transform_state = transform_state.clone();
        move |_| {
            updating.set(true);
            for spin in [&left, &right, &top, &bottom] {
                spin.set_value(0.0);
            }
            updating.set(false);
            transform_state.set_crop_margins(CropMargins::default());
            mpv.set_video_crop("");
        }
    });

    content.append(&group);
}

fn crop_spin(label: &str, upper: i64, initial: i64) -> gtk::SpinButton {
    let adjustment = gtk::Adjustment::new(
        initial.clamp(0, upper) as f64,
        0.0,
        upper.max(0) as f64,
        1.0,
        10.0,
        0.0,
    );
    let spin = gtk::SpinButton::builder()
        .adjustment(&adjustment)
        .digits(0)
        .numeric(true)
        .width_chars(6)
        .build();
    spin.update_property(&[gtk::accessible::Property::Label(label)]);
    spin
}

/// Build the "Image and Video" dialog content: brightness/contrast/gamma/
/// saturation/hue (mpv built-in video equalizer, -100..=100) plus zoom & pan.
/// Each slider is seeded from the current mpv value and applies live. Returns
/// the scrolling content widget to mount in an `adw::Dialog`.
/// The settings panel's "HDR" section: how to read the video's colours.
pub(super) fn build_hdr_section(
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) -> gtk::Widget {
    let section = gtk::Box::new(gtk::Orientation::Vertical, 0);
    build_source_colours_group(&section, mpv, transform_state);
    section.upcast()
}

/// The settings panel's "Picture" section: brightness and colour sliders,
/// two side by side on a `wide` panel.
pub(super) fn build_picture_section(mpv: &Arc<MpvBackend>, wide: bool) -> gtk::Widget {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();

    let image_rows = [
        ImageSliderSpec {
            title: gettext_noop("Contrast"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_contrast() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_contrast(value as i64),
        },
        ImageSliderSpec {
            title: gettext_noop("Brightness"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_brightness() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_brightness(value as i64),
        },
        ImageSliderSpec {
            title: gettext_noop("Gamma"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_gamma() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_gamma(value as i64),
        },
        ImageSliderSpec {
            title: gettext_noop("Saturation"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_saturation() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_saturation(value as i64),
        },
        ImageSliderSpec {
            title: gettext_noop("Hue"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_hue() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_hue(value as i64),
        },
    ];
    add_image_group(&section, &image_rows, mpv, wide);
    section.upcast()
}

/// The settings panel's "Zoom and position" section: zoom, fill, rotate,
/// mirror and crop.
pub(super) fn build_framing_section(
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    let zoom_rows = [
        ImageSliderSpec {
            title: gettext_noop("Zoom"),
            lower: 0.0,
            upper: 2.0,
            default: 0.0,
            initial: mpv.get_video_zoom(),
            step: 0.1,
            digits: 2,
            apply: |mpv, value| mpv.set_video_zoom(value),
        },
        ImageSliderSpec {
            title: gettext_noop("Fill the window"),
            lower: 0.0,
            upper: 1.0,
            default: 0.0,
            initial: mpv.get_panscan(),
            step: 0.1,
            digits: 2,
            apply: |mpv, value| mpv.set_panscan(value),
        },
    ];
    add_image_group(&content, &zoom_rows, mpv, false);
    build_transform_group(&content, mpv, transform_state);
    build_crop_group(&content, mpv, transform_state);

    content.upcast()
}

#[cfg(test)]
mod tests {
    use super::{CropMargins, clamp_initial, crop_filter_from_margins, normalize_rotation};

    #[test]
    fn in_range_value_passes_through() {
        assert_eq!(clamp_initial(0.0, -100.0, 100.0), 0.0);
        assert_eq!(clamp_initial(42.0, -100.0, 100.0), 42.0);
        assert_eq!(clamp_initial(-100.0, -100.0, 100.0), -100.0);
        assert_eq!(clamp_initial(100.0, -100.0, 100.0), 100.0);
    }

    #[test]
    fn out_of_range_value_is_clamped_to_bounds() {
        // mpv image equalizer props are -100..=100; a stray reading must clamp.
        assert_eq!(clamp_initial(250.0, -100.0, 100.0), 100.0);
        assert_eq!(clamp_initial(-250.0, -100.0, 100.0), -100.0);
        // zoom slider is 0..=2.
        assert_eq!(clamp_initial(5.0, 0.0, 2.0), 2.0);
        assert_eq!(clamp_initial(-1.0, 0.0, 2.0), 0.0);
    }

    #[test]
    fn rotation_normalizes_to_quarter_turns() {
        assert_eq!(normalize_rotation(0), 0);
        assert_eq!(normalize_rotation(90), 90);
        assert_eq!(normalize_rotation(450), 90);
        assert_eq!(normalize_rotation(-90), 270);
        assert_eq!(normalize_rotation(181), 180);
    }

    #[test]
    fn crop_filter_omits_zero_crop() {
        assert_eq!(
            crop_filter_from_margins(1920, 1080, CropMargins::default()),
            None
        );
    }

    #[test]
    fn crop_filter_clamps_to_minimum_area() {
        assert_eq!(
            crop_filter_from_margins(
                100,
                80,
                CropMargins {
                    left: 90,
                    right: 90,
                    top: 70,
                    bottom: 70,
                },
            )
            .as_deref(),
            Some("2x2+90+70")
        );
    }

    #[test]
    fn crop_filter_builds_mpv_crop_string() {
        assert_eq!(
            crop_filter_from_margins(
                1920,
                1080,
                CropMargins {
                    left: 10,
                    right: 20,
                    top: 30,
                    bottom: 40,
                },
            )
            .as_deref(),
            Some("1890x1010+10+30")
        );
    }
}
