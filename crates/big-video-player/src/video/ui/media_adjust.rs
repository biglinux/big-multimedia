//! App-local media-adjustment dialog builders ported from the old god-object's
//! `delay_adjust.rs` / `image_adjust.rs` (V-fidelity pass). Each builds a plain
//! widget tree of debounced `BigIllustratedSliderRow`s that drive the live
//! `Arc<MpvBackend>` directly — no callback indirection and no `Window`
//! god-object (the legacy routed every change through an `OptionCallback`).
//!
//! The equalizer reuses the shared
//! `big_media_runtime::shared::audio_effects::create_equalizer_dialog`
//! verbatim (the audio player drives the same dialog); only the
//! transient/size/present wiring is app-local.
//!
//! mpv is the source of truth for these live values: the delay and image
//! sliders apply straight to mpv properties (the equalizer persists its bands
//! through `Settings`). Dialog *size* persists via
//! `bind_dialog_size_persistence`, matching the old behaviour.
//!
//! The pure slider-spec → range/default mapping is exercised by the unit tests
//! at the bottom (range monotonicity + default-in-range invariants).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::adjust_panel::{BigAdjustAxis, BigAdjustPanel};
use big_relm4_components::input::debounce::connect_debounced_adjustment;
use big_relm4_components::input::illustrated_slider_row::{
    BigIllustratedSliderRow, BigIllustratedSliderRowSpec,
};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;

use crate::config::video::RESOURCE_PREFIX;
use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use big_media_runtime::i18n::gettext_noop;

/// Debounce window for live slider→mpv updates (matches the old 200 ms).
const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Default)]
pub(crate) struct CropMargins {
    left: i64,
    right: i64,
    top: i64,
    bottom: i64,
}

#[derive(Clone, Default)]
pub(crate) struct VideoTransformState {
    hflip: Rc<Cell<bool>>,
    vflip: Rc<Cell<bool>>,
    crop_margins: Rc<RefCell<CropMargins>>,
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

    fn crop_margins(&self) -> CropMargins {
        *self.crop_margins.borrow()
    }

    fn set_crop_margins(&self, margins: CropMargins) {
        self.crop_margins.replace(margins);
    }
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

/// One audio/subtitle delay slider: illustration, range, and the mpv property
/// it drives. `apply` receives the live seconds value.
struct DelaySliderSpec {
    illustration: &'static str,
    title: &'static str,
    subtitle: &'static str,
    apply: fn(&MpvBackend, f64),
}

/// One image slider: illustration, the integer-percent range mpv uses for the
/// built-in video equalizer (-100..=100), the current value, and the setter.
struct ImageSliderSpec {
    illustration: &'static str,
    title: &'static str,
    subtitle: &'static str,
    lower: f64,
    upper: f64,
    default: f64,
    initial: f64,
    step: f64,
    digits: i32,
    apply: fn(&MpvBackend, f64),
}

/// Build the "Audio and Subtitle Delay" dialog content: two debounced sliders
/// (subtitle + audio delay, ±10 s) bound to the live mpv backend. Returns the
/// scrolling content widget to mount in an `adw::Dialog`.
fn build_delay_content(mpv: &Arc<MpvBackend>) -> gtk::Widget {
    let prefix = illustration_prefix();
    let main_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(16)
        .margin_top(16)
        .margin_bottom(16)
        .margin_start(20)
        .margin_end(20)
        .build();

    let desc = gtk::Label::builder()
        .label(i18n(
            "Adjust timing when audio or subtitles are out of sync with the video. \
             Positive values delay, negative values advance.",
        ))
        .wrap(true)
        .xalign(0.0)
        .build();
    desc.add_css_class("dim-label");
    main_box.append(&desc);

    let rows = [
        DelaySliderSpec {
            illustration: "sub_delay.svg",
            title: gettext_noop("Subtitle Delay"),
            subtitle: gettext_noop("Shift subtitles forward or backward in time"),
            apply: |mpv, value| mpv.set_sub_delay(value),
        },
        DelaySliderSpec {
            illustration: "audio_delay.svg",
            title: gettext_noop("Audio Delay"),
            subtitle: gettext_noop("Shift audio forward or backward in time"),
            apply: |mpv, value| mpv.set_audio_delay(value),
        },
    ];

    for spec in rows {
        let row = BigIllustratedSliderRow::new(
            BigIllustratedSliderRowSpec::new(
                &prefix,
                i18n(spec.title),
                -10.0,
                10.0,
                0.0,
                0.0,
                0.1,
                i18n("Reset to zero"),
            )
            .illustration_name(spec.illustration)
            .subtitle(i18n(spec.subtitle))
            .digits(1)
            .value_suffix(" s")
            .value_width_chars(7)
            .scale_width_request(240)
            .title_heading(true)
            .mark_label(Some("0")),
        );
        let (outer_box, adj, _, _, _) = row.into_parts();

        let mpv = Arc::clone(mpv);
        let apply = spec.apply;
        connect_debounced_adjustment(&adj, SLIDER_DEBOUNCE, move |value| apply(&mpv, value));

        main_box.append(&outer_box);
    }

    main_box.upcast()
}

/// Append a `PreferencesGroup` of image sliders to `content`, each debounced
/// onto its mpv setter.
fn add_image_group(
    content: &gtk::Box,
    title: &str,
    description: &str,
    rows: &[ImageSliderSpec],
    mpv: &Arc<MpvBackend>,
) {
    let prefix = illustration_prefix();
    let group = BigPreferencesGroupSpec::new(title)
        .description(description)
        .build();
    let axes: Vec<BigAdjustAxis> = rows
        .iter()
        .map(|spec| {
            let row_spec = BigIllustratedSliderRowSpec::new(
                &prefix,
                i18n(spec.title),
                spec.lower,
                spec.upper,
                spec.default,
                clamp_initial(spec.initial, spec.lower, spec.upper),
                spec.step,
                i18n("Reset"),
            )
            .illustration_name(spec.illustration)
            .subtitle(i18n(spec.subtitle))
            .digits(spec.digits)
            .scale_width_request(200);
            let mpv = Arc::clone(mpv);
            let apply = spec.apply;
            BigAdjustAxis::new(row_spec, move |value| apply(&mpv, value))
        })
        .collect();

    let panel = BigAdjustPanel::new(axes, SLIDER_DEBOUNCE);
    group.add(panel.root());
    content.append(&group);
}

fn build_transform_group(
    content: &gtk::Box,
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) {
    let group = BigPreferencesGroupSpec::new(i18n("Transform"))
        .description(i18n("Rotate or mirror the video picture"))
        .build();

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
    let group = BigPreferencesGroupSpec::new(i18n("Crop"))
        .description(i18n("Crop video edges in pixels"))
        .build();
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
fn build_image_content(
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
) -> gtk::Widget {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    let image_rows = [
        ImageSliderSpec {
            illustration: "contrast.svg",
            title: gettext_noop("Contrast"),
            subtitle: gettext_noop("Difference between dark and light areas"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_contrast() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_contrast(value as i64),
        },
        ImageSliderSpec {
            illustration: "brightness.svg",
            title: gettext_noop("Brightness"),
            subtitle: gettext_noop("Overall lightness of the image"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_brightness() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_brightness(value as i64),
        },
        ImageSliderSpec {
            illustration: "gamma.svg",
            title: gettext_noop("Gamma"),
            subtitle: gettext_noop(
                "Midtone brightness — adjusts without clipping highlights or shadows",
            ),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_gamma() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_gamma(value as i64),
        },
        ImageSliderSpec {
            illustration: "saturation.svg",
            title: gettext_noop("Saturation"),
            subtitle: gettext_noop("Color intensity — 0 = grayscale"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_saturation() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_saturation(value as i64),
        },
        ImageSliderSpec {
            illustration: "hue.svg",
            title: gettext_noop("Hue"),
            subtitle: gettext_noop("Shift all colors around the color wheel"),
            lower: -100.0,
            upper: 100.0,
            default: 0.0,
            initial: mpv.get_hue() as f64,
            step: 1.0,
            digits: 0,
            apply: |mpv, value| mpv.set_hue(value as i64),
        },
    ];
    add_image_group(
        &content,
        &i18n("Image"),
        &i18n("Fine-tune colors and brightness of the video picture"),
        &image_rows,
        mpv,
    );

    let zoom_rows = [
        ImageSliderSpec {
            illustration: "zoom.svg",
            title: gettext_noop("Zoom"),
            subtitle: gettext_noop("Magnify the picture — 0 = fit window, 2 = double size"),
            lower: 0.0,
            upper: 2.0,
            default: 0.0,
            initial: mpv.get_video_zoom(),
            step: 0.1,
            digits: 2,
            apply: |mpv, value| mpv.set_video_zoom(value),
        },
        ImageSliderSpec {
            illustration: "panscan.svg",
            title: gettext_noop("Pan & Scan"),
            subtitle: gettext_noop(
                "Crop edges to fill the window — useful for removing black bars",
            ),
            lower: 0.0,
            upper: 1.0,
            default: 0.0,
            initial: mpv.get_panscan(),
            step: 0.1,
            digits: 2,
            apply: |mpv, value| mpv.set_panscan(value),
        },
    ];
    add_image_group(
        &content,
        &i18n("Zoom and Pan"),
        &i18n("Enlarge or reframe the video within the window"),
        &zoom_rows,
        mpv,
    );
    build_transform_group(&content, mpv, transform_state);
    build_crop_group(&content, mpv, transform_state);

    let clamp = adw::Clamp::builder()
        .maximum_size(520)
        .child(&content)
        .build();
    let scroll = gtk::ScrolledWindow::builder()
        .vexpand(true)
        .child(&clamp)
        .build();
    scroll.upcast()
}

/// Wrap dialog `content` in a titled `adw::Dialog` with a header bar, persist
/// its size, and present it over `root`. Esc-to-close is built into
/// `adw::Dialog`.
fn present_adjust_dialog(
    content: &gtk::Widget,
    title: &str,
    width: i32,
    height: i32,
    root: &adw::ApplicationWindow,
) {
    let page = BigDialogPage::scrolled_clamped(width, width);
    page.content().append(content);
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(width)
        .content_height(height)
        .child(&page.into_root())
        .build();
    dialog.present(Some(root));
}

/// Present the "Audio and Subtitle Delay" dialog bound to `mpv`.
pub(super) fn present_delay_dialog(mpv: &Arc<MpvBackend>, root: &adw::ApplicationWindow) {
    let content = build_delay_content(mpv);
    present_adjust_dialog(&content, &i18n("Audio and Subtitle Delay"), 460, 320, root);
}

/// Present the "Image and Video" dialog bound to `mpv`.
pub(super) fn present_image_adjust_dialog(
    mpv: &Arc<MpvBackend>,
    transform_state: &VideoTransformState,
    root: &adw::ApplicationWindow,
) {
    let content = build_image_content(mpv, transform_state);
    present_adjust_dialog(&content, &i18n("Image and Video"), 500, 600, root);
}

/// Present the audio equalizer dialog — the shared dialog the audio player
/// uses, persisting its bands through `Settings` and applying to mpv.
pub(super) fn present_equalizer_dialog(
    settings: &crate::settings::Settings,
    mpv: Option<Arc<MpvBackend>>,
    root: &adw::ApplicationWindow,
) {
    let dialog = crate::shared::audio_effects::create_equalizer_dialog(settings, mpv, None);
    dialog.set_transient_for(Some(root));
    crate::settings::bind_dialog_size_persistence(&dialog, settings, "equalizer", 500, 400);
    dialog.present();
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
