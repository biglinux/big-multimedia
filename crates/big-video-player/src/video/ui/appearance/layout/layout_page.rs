//! Appearance → Layout preferences page: controls bar (radius/margin/padding),
//! buttons (custom size + radius gated by switch), and window corner radius +
//! window-button sizing. Each switch toggles visibility of its dependent rows.

use adw::prelude::*;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use std::rc::Rc;

use crate::i18n::{gettext_noop, i18n};
use crate::settings::{Settings, keys};
use crate::shared::preference_rows::add_spin_int_row;

use super::super::illustration;

pub(in crate::video) fn create_layout_page(
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("Layout"))
        .icon_name("view-grid-symbolic")
        .build();

    page.add(&build_controls_bar_group(settings, on_changed));
    page.add(&build_buttons_group(settings, on_changed));
    page.add(&build_window_group(settings, on_changed));

    page
}

fn build_controls_bar_group(
    settings: &Settings,
    on_changed: &Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let bar_group = BigPreferencesGroupSpec::new(i18n("Controls Bar"))
        .description(i18n(
            "Adjust the shape and spacing of the player controls at the bottom",
        ))
        .build();

    let spin_items: &[(&str, &str, f64, f64, &str)] = &[
        (
            gettext_noop("Corner Radius"),
            "ui-controls-radius",
            0.0,
            48.0,
            "corner_radius.svg",
        ),
        (
            gettext_noop("Bottom Margin"),
            "ui-controls-margin-bottom",
            0.0,
            100.0,
            "margin.svg",
        ),
        (
            gettext_noop("Side Margin"),
            "ui-controls-margin-side",
            0.0,
            100.0,
            "margin.svg",
        ),
        (
            gettext_noop("Padding Top"),
            "ui-controls-padding-top",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            gettext_noop("Padding Bottom"),
            "ui-controls-padding-bottom",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            gettext_noop("Padding Left"),
            "ui-controls-padding-left",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            gettext_noop("Padding Right"),
            "ui-controls-padding-right",
            0.0,
            48.0,
            "padding.svg",
        ),
    ];
    for (label, key, min, max, svg) in spin_items {
        let on_changed = on_changed.clone();
        let row = add_spin_int_row(
            &bar_group,
            &i18n(label),
            "",
            key,
            *min,
            *max,
            1.0,
            settings,
            move |_| on_changed(),
        );
        row.add_prefix(&illustration(svg));
    }
    bar_group
}

fn build_buttons_group(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let button_group = BigPreferencesGroupSpec::new(i18n("Buttons"))
        .description(i18n("Customize the size and shape of player buttons"))
        .build();

    let custom_button_size_row = adw::SwitchRow::builder()
        .title(i18n("Custom Button Size"))
        .subtitle(i18n(
            "Set your own button size instead of the system default",
        ))
        .active(settings.get_bool(keys::UI_BUTTON_CUSTOM))
        .build();
    custom_button_size_row.add_prefix(&illustration("button_size.svg"));
    button_group.add(&custom_button_size_row);

    let button_size_row = adw::SpinRow::builder()
        .title(i18n("Button Size"))
        .adjustment(&gtk::Adjustment::new(
            f64::from(settings.get_int(keys::UI_BUTTON_SIZE)),
            24.0,
            64.0,
            2.0,
            8.0,
            0.0,
        ))
        .digits(0)
        .build();
    button_size_row.set_visible(settings.get_bool(keys::UI_BUTTON_CUSTOM));
    button_size_row.add_prefix(&illustration("button_size.svg"));
    button_group.add(&button_size_row);

    let button_radius_row = adw::SpinRow::builder()
        .title(i18n("Button Border Radius"))
        .adjustment(&gtk::Adjustment::new(
            f64::from(settings.get_int(keys::UI_BUTTON_RADIUS)),
            0.0,
            32.0,
            1.0,
            4.0,
            0.0,
        ))
        .digits(0)
        .build();
    button_radius_row.set_visible(settings.get_bool(keys::UI_BUTTON_CUSTOM));
    button_radius_row.add_prefix(&illustration("corner_radius.svg"));
    button_group.add(&button_radius_row);

    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        let button_size_row = button_size_row.clone();
        let button_radius_row = button_radius_row.clone();
        custom_button_size_row.connect_active_notify(move |row| {
            let active = row.is_active();
            let _ = settings.put_bool(keys::UI_BUTTON_CUSTOM, active);
            button_size_row.set_visible(active);
            button_radius_row.set_visible(active);
            on_changed();
        });
    }
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        button_size_row
            .adjustment()
            .connect_value_changed(move |adjustment| {
                let _ = settings.put_int(keys::UI_BUTTON_SIZE, adjustment.value() as i32);
                on_changed();
            });
    }
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        button_radius_row
            .adjustment()
            .connect_value_changed(move |adjustment| {
                let _ = settings.put_int(keys::UI_BUTTON_RADIUS, adjustment.value() as i32);
                on_changed();
            });
    }
    button_group
}

fn build_window_group(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let window_group = BigPreferencesGroupSpec::new(i18n("Window"))
        .description(i18n("Adjust the player window appearance"))
        .build();

    let custom_window_radius_row = adw::SwitchRow::builder()
        .title(i18n("Custom Window Corner Radius"))
        .subtitle(i18n(
            "Set your own corner rounding instead of the system default",
        ))
        .active(settings.get_bool(keys::UI_WINDOW_RADIUS_CUSTOM))
        .build();
    custom_window_radius_row.add_prefix(&illustration("window_radius.svg"));
    window_group.add(&custom_window_radius_row);

    let window_radius_row = adw::SpinRow::builder()
        .title(i18n("Window Corner Radius"))
        .adjustment(&gtk::Adjustment::new(
            f64::from(settings.get_int(keys::UI_WINDOW_RADIUS)),
            0.0,
            48.0,
            1.0,
            4.0,
            0.0,
        ))
        .digits(0)
        .build();
    window_radius_row.set_visible(settings.get_bool(keys::UI_WINDOW_RADIUS_CUSTOM));
    window_radius_row.add_prefix(&illustration("window_radius.svg"));
    window_group.add(&window_radius_row);

    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        let window_radius_row = window_radius_row.clone();
        custom_window_radius_row.connect_active_notify(move |row| {
            let active = row.is_active();
            let _ = settings.put_bool(keys::UI_WINDOW_RADIUS_CUSTOM, active);
            window_radius_row.set_visible(active);
            on_changed();
        });
    }
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        window_radius_row
            .adjustment()
            .connect_value_changed(move |adjustment| {
                let _ = settings.put_int(keys::UI_WINDOW_RADIUS, adjustment.value() as i32);
                on_changed();
            });
    }

    let custom_window_button_size_row = adw::SwitchRow::builder()
        .title(i18n("Window Buttons Follow Custom Size"))
        .subtitle(i18n("Also resize the minimize, maximize and close buttons"))
        .active(settings.get_bool(keys::UI_WINDOW_BUTTON_CUSTOM_SIZE))
        .build();
    {
        let settings = settings.clone();
        let on_changed = on_changed.clone();
        custom_window_button_size_row.connect_active_notify(move |row| {
            let _ = settings.put_bool(keys::UI_WINDOW_BUTTON_CUSTOM_SIZE, row.is_active());
            on_changed();
        });
    }
    custom_window_button_size_row.add_prefix(&illustration("window_buttons.svg"));
    window_group.add(&custom_window_button_size_row);

    window_group
}
