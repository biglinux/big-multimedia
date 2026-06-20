//! Appearance → Layout preferences page: controls bar (radius/margin/padding),
//! buttons (custom size + radius gated by switch), and window corner radius +
//! window-button sizing. Each switch toggles visibility of its dependent rows.

use adw::prelude::*;
use std::rc::Rc;

use crate::i18n::i18n;
use crate::settings::{keys, Settings};
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
    let bar_group = adw::PreferencesGroup::builder()
        .title(i18n("Controls Bar"))
        .description(i18n(
            "Adjust the shape and spacing of the player controls at the bottom",
        ))
        .build();

    let spin_items: &[(&str, &str, f64, f64, &str)] = &[
        (
            "Corner Radius",
            "ui-controls-radius",
            0.0,
            48.0,
            "corner_radius.svg",
        ),
        (
            "Bottom Margin",
            "ui-controls-margin-bottom",
            0.0,
            100.0,
            "margin.svg",
        ),
        (
            "Side Margin",
            "ui-controls-margin-side",
            0.0,
            100.0,
            "margin.svg",
        ),
        (
            "Padding Top",
            "ui-controls-padding-top",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            "Padding Bottom",
            "ui-controls-padding-bottom",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            "Padding Left",
            "ui-controls-padding-left",
            0.0,
            48.0,
            "padding.svg",
        ),
        (
            "Padding Right",
            "ui-controls-padding-right",
            0.0,
            48.0,
            "padding.svg",
        ),
    ];
    for (label, key, min, max, svg) in spin_items {
        let cb = on_changed.clone();
        let row = add_spin_int_row(
            &bar_group,
            &i18n(label),
            "",
            key,
            *min,
            *max,
            1.0,
            settings,
            move |_| cb(),
        );
        row.add_prefix(&illustration(svg));
    }
    bar_group
}

fn build_buttons_group(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let btn_group = adw::PreferencesGroup::builder()
        .title(i18n("Buttons"))
        .description(i18n("Customize the size and shape of player buttons"))
        .build();

    let btn_custom_row = adw::SwitchRow::builder()
        .title(i18n("Custom Button Size"))
        .subtitle(i18n(
            "Set your own button size instead of the system default",
        ))
        .active(settings.get_bool(keys::UI_BUTTON_CUSTOM))
        .build();
    btn_custom_row.add_prefix(&illustration("button_size.svg"));
    btn_group.add(&btn_custom_row);

    let btn_size_row = adw::SpinRow::builder()
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
    btn_size_row.set_visible(settings.get_bool(keys::UI_BUTTON_CUSTOM));
    btn_size_row.add_prefix(&illustration("button_size.svg"));
    btn_group.add(&btn_size_row);

    let btn_radius_row = adw::SpinRow::builder()
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
    btn_radius_row.set_visible(settings.get_bool(keys::UI_BUTTON_CUSTOM));
    btn_radius_row.add_prefix(&illustration("corner_radius.svg"));
    btn_group.add(&btn_radius_row);

    {
        let s = settings.clone();
        let cb = on_changed.clone();
        let bs = btn_size_row.clone();
        let br = btn_radius_row.clone();
        btn_custom_row.connect_active_notify(move |row| {
            let active = row.is_active();
            let _ = s.put_bool(keys::UI_BUTTON_CUSTOM, active);
            bs.set_visible(active);
            br.set_visible(active);
            cb();
        });
    }
    {
        let s = settings.clone();
        let cb = on_changed.clone();
        btn_size_row.adjustment().connect_value_changed(move |adj| {
            let _ = s.put_int(keys::UI_BUTTON_SIZE, adj.value() as i32);
            cb();
        });
    }
    {
        let s = settings.clone();
        let cb = on_changed.clone();
        btn_radius_row
            .adjustment()
            .connect_value_changed(move |adj| {
                let _ = s.put_int(keys::UI_BUTTON_RADIUS, adj.value() as i32);
                cb();
            });
    }
    btn_group
}

fn build_window_group(settings: &Settings, on_changed: &Rc<dyn Fn()>) -> adw::PreferencesGroup {
    let win_group = adw::PreferencesGroup::builder()
        .title(i18n("Window"))
        .description(i18n("Adjust the player window appearance"))
        .build();

    let wr_custom_row = adw::SwitchRow::builder()
        .title(i18n("Custom Window Corner Radius"))
        .subtitle(i18n(
            "Set your own corner rounding instead of the system default",
        ))
        .active(settings.get_bool(keys::UI_WINDOW_RADIUS_CUSTOM))
        .build();
    wr_custom_row.add_prefix(&illustration("window_radius.svg"));
    win_group.add(&wr_custom_row);

    let wr_spin_row = adw::SpinRow::builder()
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
    wr_spin_row.set_visible(settings.get_bool(keys::UI_WINDOW_RADIUS_CUSTOM));
    wr_spin_row.add_prefix(&illustration("window_radius.svg"));
    win_group.add(&wr_spin_row);

    {
        let s = settings.clone();
        let cb = on_changed.clone();
        let ws = wr_spin_row.clone();
        wr_custom_row.connect_active_notify(move |row| {
            let active = row.is_active();
            let _ = s.put_bool(keys::UI_WINDOW_RADIUS_CUSTOM, active);
            ws.set_visible(active);
            cb();
        });
    }
    {
        let s = settings.clone();
        let cb = on_changed.clone();
        wr_spin_row.adjustment().connect_value_changed(move |adj| {
            let _ = s.put_int(keys::UI_WINDOW_RADIUS, adj.value() as i32);
            cb();
        });
    }

    let wbtn_row = adw::SwitchRow::builder()
        .title(i18n("Window Buttons Follow Custom Size"))
        .subtitle(i18n("Also resize the minimize, maximize and close buttons"))
        .active(settings.get_bool(keys::UI_WINDOW_BTN_CUSTOM_SIZE))
        .build();
    {
        let s = settings.clone();
        let cb = on_changed.clone();
        wbtn_row.connect_active_notify(move |row| {
            let _ = s.put_bool(keys::UI_WINDOW_BTN_CUSTOM_SIZE, row.is_active());
            cb();
        });
    }
    wbtn_row.add_prefix(&illustration("window_buttons.svg"));
    win_group.add(&wbtn_row);

    win_group
}
