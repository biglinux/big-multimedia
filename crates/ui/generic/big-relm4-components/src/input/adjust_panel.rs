// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! A group of debounced illustrated sliders for live media adjustment
//! (brightness / contrast / saturation / gamma / hue …).
//!
//! One panel, any backend: each axis carries a [`BigIllustratedSliderRowSpec`]
//! plus an `apply` sink invoked (debounced) with the new value as the user
//! drags. The image viewer drives an image pipeline, the video player drives
//! the mpv video equalizer, the camera drives v4l2 controls — all through the
//! same panel. Labels/subtitles stay caller-supplied so each app keeps its own
//! gettext domain; this module only owns the build loop and the canonical color
//! axis range so those stay consistent across apps.

use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use relm4::gtk;

use crate::input::debounce::connect_debounced_adjustment;
use crate::input::illustrated_slider_row::{BigIllustratedSliderRow, BigIllustratedSliderRowSpec};

/// Default vertical spacing between sliders in a [`BigAdjustPanel`]. Override
/// with `panel.root().set_spacing(n)` after building.
pub const DEFAULT_SLIDER_SPACING: i32 = 12;

/// One adjustment axis: the slider spec plus the sink that applies its value.
pub struct BigAdjustAxis {
    /// Slider appearance + range.
    pub spec: BigIllustratedSliderRowSpec,
    /// Debounced sink invoked with the new value while the user drags.
    pub apply: Rc<dyn Fn(f64)>,
}

impl BigAdjustAxis {
    /// Pair a slider spec with an apply sink.
    #[must_use]
    pub fn new(spec: BigIllustratedSliderRowSpec, apply: impl Fn(f64) + 'static) -> Self {
        Self {
            spec,
            apply: Rc::new(apply),
        }
    }
}

/// A vertical group of debounced illustrated adjustment sliders.
///
/// Backend-agnostic: the caller supplies the axes (spec + sink). Each slider's
/// adjustment is wired through [`connect_debounced_adjustment`], so the apply
/// sink only fires after the user pauses — cheap to drive a live pipeline.
pub struct BigAdjustPanel {
    root: gtk::Box,
}

impl BigAdjustPanel {
    /// Build a panel from `axes`, debouncing each slider with `debounce`.
    #[must_use]
    pub fn new(axes: Vec<BigAdjustAxis>, debounce: Duration) -> Self {
        let root = gtk::Box::new(gtk::Orientation::Vertical, DEFAULT_SLIDER_SPACING);
        for axis in axes {
            let row = BigIllustratedSliderRow::new(axis.spec);
            let apply = axis.apply;
            connect_debounced_adjustment(row.adjustment(), debounce, move |value| apply(value));
            root.append(row.root());
        }
        Self { root }
    }

    /// The panel's root container, ready to embed in a dialog/page.
    #[must_use]
    pub fn root(&self) -> &gtk::Box {
        &self.root
    }
}
