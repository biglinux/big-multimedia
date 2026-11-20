//! Shared widget helper for the filters dialog: `build_vf_expander_row`, an
//! ExpanderRow + SpinRow
//! that owns the enable/disable + debounced-apply lifecycle of a single
//! single-parameter `vf` filter, keyed by `label`, driving an
//! `Arc<MpvBackend>` handle.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use adw::prelude::*;
use big_relm4_components::input::spin_row::accessible_spin_row;

use crate::player::mpv_backend::MpvBackend;
use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;

/// Spec for a single-parameter `vf` filter row.
pub(super) struct VfFilterSpec<F: Fn(f64) -> String + Clone + 'static> {
    /// mpv vf label (identity for append/remove and active detection).
    pub(super) label: &'static str,
    /// The graph option holding the spin's value, read back from mpv so an
    /// active filter shows the value it runs with.
    pub(super) param: &'static str,
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
    let active = super::filter_specs::active_vf_graph(current_vf, spec.label);
    let is_active = active.is_some();
    let expander = adw::ExpanderRow::builder()
        .title(&spec.title)
        .subtitle(&spec.subtitle)
        .show_enable_switch(true)
        .enable_expansion(is_active)
        .build();

    let (default, min, max, step, page) = spec.adjustment;
    let value = active
        .and_then(|graph| super::filter_specs::vf_graph_number(&graph, spec.param))
        .map_or(default, |value| value.clamp(min, max));
    let adjustment = gtk::Adjustment::new(value, min, max, step, page, 0.0);
    let mut spin_builder = accessible_spin_row()
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
        let adjustment = adjustment.downgrade();
        let format_vf = spec.format_vf.clone();
        move || {
            let Some(adjustment) = adjustment.upgrade() else {
                return;
            };
            let value = adjustment.value();
            mpv.vf_append(label, &format_vf(value));
        }
    };

    let mpv = Arc::clone(mpv);
    let changed = wire_filter_expander(
        &expander,
        Some(Duration::from_millis(spec.debounce_ms)),
        apply,
        move || mpv.vf_remove(label),
    );
    adjustment.connect_value_changed(move |_| changed());

    expander
}

/// Run `apply` when `expander` is switched on and `remove` when it is switched
/// off. The returned callback reapplies after a value change, after `debounce`
/// when given, and only while the expander is on.
pub(super) fn wire_filter_expander(
    expander: &adw::ExpanderRow,
    debounce: Option<Duration>,
    apply: impl Fn() + Clone + 'static,
    remove: impl Fn() + 'static,
) -> Rc<dyn Fn()> {
    let apply_on_toggle = apply.clone();
    expander.connect_enable_expansion_notify(move |row| {
        if row.enables_expansion() {
            apply_on_toggle();
        } else {
            remove();
        }
    });
    let debounce = debounce.map(DebouncedAction::new);
    // Weak: the expander owns the value widgets whose handlers keep this
    // callback, so a strong capture would be a cycle that leaks on close.
    let expander = expander.downgrade();
    Rc::new(move || {
        if !expander
            .upgrade()
            .is_some_and(|row| row.enables_expansion())
        {
            return;
        }
        match &debounce {
            // Switched off before the delay ends: nothing to reapply.
            Some(debounce) => {
                let (apply, expander) = (apply.clone(), expander.clone());
                debounce.run(move || {
                    if expander
                        .upgrade()
                        .is_some_and(|row| row.enables_expansion())
                    {
                        apply();
                    }
                });
            }
            None => apply(),
        }
    })
}
