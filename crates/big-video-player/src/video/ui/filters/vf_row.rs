//! Shared widget helpers for the filters dialog: the GResource-bound SVG
//! illustration loader and `build_vf_expander_row` — an ExpanderRow + SpinRow
//! that owns the enable/disable + debounced-apply lifecycle of a single
//! single-parameter `vf` filter, keyed by `label`. App-local port of the
//! legacy `dialogs/filters/common.rs`, retargeted from the `Window` god-object
//! to a plain `Arc<MpvBackend>` handle.

use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;

use crate::config::video::RESOURCE_PREFIX;
use crate::player::mpv_backend::MpvBackend;
use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;

/// Load a filters-dialog SVG illustration from the video GResource bundle.
pub(super) fn illustration(filename: &str) -> gtk::Image {
    crate::shared::illustration::illustration(
        &format!("{RESOURCE_PREFIX}/illustrations/"),
        filename,
    )
}

/// Spec for a single-parameter `vf` filter row.
pub(super) struct VfFilterSpec<F: Fn(f64) -> String + Clone + 'static> {
    /// mpv vf label (identity for append/remove and active detection).
    pub(super) label: &'static str,
    /// Illustration SVG basename (empty = none).
    pub(super) svg: &'static str,
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) spin_title: String,
    /// `(default, min, max, step, page)` for the spin adjustment.
    pub(super) adjustment: (f64, f64, f64, f64, f64),
    pub(super) digits: u32,
    pub(super) debounce_ms: u64,
    /// Maps the spin value to the mpv vf filter string.
    pub(super) format_vf: F,
}

/// Build an ExpanderRow whose enable-switch appends/removes the vf filter and
/// whose spin value re-applies (debounced) while enabled.
pub(super) fn build_vf_expander_row(
    mpv: &Arc<MpvBackend>,
    current_vf: &str,
    spec: VfFilterSpec<impl Fn(f64) -> String + Clone + 'static>,
) -> adw::ExpanderRow {
    let is_active = current_vf.contains(spec.label);
    let expander = adw::ExpanderRow::builder()
        .title(&spec.title)
        .subtitle(&spec.subtitle)
        .show_enable_switch(true)
        .enable_expansion(is_active)
        .build();
    if !spec.svg.is_empty() {
        expander.add_prefix(&illustration(spec.svg));
    }

    let (default, min, max, step, page) = spec.adjustment;
    let adjustment = gtk::Adjustment::new(default, min, max, step, page, 0.0);
    let mut spin_builder = adw::SpinRow::builder()
        .title(&spec.spin_title)
        .adjustment(&adjustment);
    if spec.digits > 0 {
        spin_builder = spin_builder.digits(spec.digits);
    }
    let spin = spin_builder.build();
    expander.add_row(&spin);

    let label = spec.label;
    let apply = {
        let mpv = Arc::clone(mpv);
        let adjustment = adjustment.clone();
        let format_vf = spec.format_vf.clone();
        move || {
            let value = adjustment.value();
            mpv.vf_append(label, &format_vf(value));
        }
    };

    let apply_on_toggle = apply.clone();
    let mpv_toggle = Arc::clone(mpv);
    expander.connect_enable_expansion_notify(move |row| {
        if row.enables_expansion() {
            apply_on_toggle();
        } else {
            mpv_toggle.vf_remove(label);
        }
    });

    let debounce = DebouncedAction::new(Duration::from_millis(spec.debounce_ms));
    // Weak back-ref: the expander owns the SpinRow (→ adjustment); a strong
    // capture here would be a mutual reference cycle (leaks on dialog close).
    let expander_weak = expander.downgrade();
    adjustment.connect_value_changed(move |_| {
        let Some(expander_ref) = expander_weak.upgrade() else {
            return;
        };
        if !expander_ref.enables_expansion() {
            return;
        }
        debounce.run(apply.clone());
    });

    expander
}
