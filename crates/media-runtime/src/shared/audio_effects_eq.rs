//! bigagents: app-local-adapter; binds equalizer dialog to app settings/mpv callbacks.

use adw::prelude::*;
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::keys;
use big_relm4_components::input::debounce::BigDebouncedAction as DebouncedAction;

use super::audio_effects::{EQ_LABELS, rebuild_filters};

struct EqPreset {
    name: &'static str,
    gains: [f64; 10],
}

fn presets() -> Vec<EqPreset> {
    vec![
        EqPreset {
            name: "Flat",
            gains: [0.0; 10],
        },
        EqPreset {
            name: "Bass Boost",
            gains: [6.0, 5.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        },
        EqPreset {
            name: "Treble Boost",
            gains: [0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 5.0, 6.0, 6.0],
        },
        EqPreset {
            name: "Pop",
            gains: [1.0, 3.0, 5.0, 4.0, 2.0, -1.0, -2.0, -1.0, 2.0, 4.0],
        },
        EqPreset {
            name: "Rock",
            gains: [5.0, 4.0, 2.0, 0.0, -2.0, -1.0, 0.0, 3.0, 4.0, 5.0],
        },
        EqPreset {
            name: "Jazz",
            gains: [3.0, 2.0, 1.0, 2.0, -2.0, -2.0, 0.0, 2.0, 3.0, 4.0],
        },
        EqPreset {
            name: "Classical",
            gains: [5.0, 4.0, 3.0, 2.0, -1.0, -1.0, 0.0, 3.0, 4.0, 5.0],
        },
        EqPreset {
            name: "Vocal",
            gains: [-2.0, -1.0, 0.0, 3.0, 5.0, 5.0, 3.0, 1.0, 0.0, -1.0],
        },
        EqPreset {
            name: "Electronic",
            gains: [5.0, 4.0, 2.0, 0.0, -2.0, 0.0, 2.0, 4.0, 5.0, 5.0],
        },
    ]
}

pub fn create_equalizer_dialog(
    settings: &crate::settings::Settings,
    mpv: Option<Arc<MpvBackend>>,
    on_change: Option<Rc<dyn Fn()>>,
) -> adw::Window {
    let main_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_top(8)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .build();

    let eq_gains: Rc<RefCell<[f64; 10]>> = Rc::new(RefCell::new([0.0; 10]));
    let updating_from_preset: Rc<Cell<bool>> = Rc::new(Cell::new(false));

    // Load stored EQ bands
    let stored_eq = settings.get_string(keys::EQ_BANDS);
    if !stored_eq.is_empty() {
        let parts: Vec<f64> = stored_eq
            .split(';')
            .filter_map(|s| s.parse().ok())
            .collect();
        let mut gains = eq_gains.borrow_mut();
        for (i, &g) in parts.iter().enumerate() {
            if i < 10 {
                gains[i] = g;
            }
        }
    }

    // ── Description ──
    let desc = gtk::Label::builder()
        .label(i18n(
            "Boost or cut frequency bands to shape the sound. Use presets as a starting point.",
        ))
        .wrap(true)
        .xalign(0.5)
        .build();
    desc.add_css_class("dim-label");
    main_box.append(&desc);

    // ── Preset selector (top, prominent) ──
    let preset_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .halign(gtk::Align::Center)
        .margin_top(4)
        .margin_bottom(4)
        .build();

    let preset_label = gtk::Label::new(Some(&i18n("Preset:")));

    let preset_names: Vec<String> = presets().iter().map(|p| i18n(p.name)).collect();
    let preset_refs: Vec<&str> = preset_names.iter().map(String::as_str).collect();
    let preset_list = gtk::StringList::new(&preset_refs);
    let preset_combo = gtk::DropDown::builder().model(&preset_list).build();

    let current_preset = settings.get_string(keys::EQ_PRESET);
    let preset_idx = presets()
        .iter()
        .position(|p| p.name.to_lowercase() == current_preset.as_str().to_lowercase())
        .unwrap_or(0);
    preset_combo.set_selected(preset_idx as u32);

    preset_box.append(&preset_label);
    preset_box.append(&preset_combo);

    // Reset button
    let reset_button = gtk::Button::builder()
        .icon_name("edit-clear-symbolic")
        .build();
    reset_button.add_css_class("flat");
    crate::shared::tooltip::set(&reset_button, &i18n("Reset to Flat"));
    preset_box.append(&reset_button);

    main_box.append(&preset_box);

    // ── EQ sliders area: dB scale on left + 10 band sliders ──
    let eq_area = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .halign(gtk::Align::Center)
        .build();

    // dB scale labels (left side)
    let db_scale = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .margin_end(4)
        .build();
    // Spacer for freq_label height alignment
    let db_spacer_top = gtk::Label::new(Some(""));
    db_spacer_top.set_height_request(20);
    db_scale.append(&db_spacer_top);
    // dB markers
    for db_val in &["+12", "+6", "0", "-6", "-12"] {
        let decibel_marker_label = gtk::Label::new(Some(db_val));
        decibel_marker_label.add_css_class("dim-label");
        decibel_marker_label.add_css_class("caption");
        decibel_marker_label.set_vexpand(true);
        decibel_marker_label.set_valign(gtk::Align::Center);
        db_scale.append(&decibel_marker_label);
    }
    // Spacer for val_label height alignment
    let db_spacer_bot = gtk::Label::new(Some(""));
    db_spacer_bot.set_height_request(20);
    db_scale.append(&db_spacer_bot);
    eq_area.append(&db_scale);

    let sliders_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(4)
        .halign(gtk::Align::Center)
        .build();

    let initial_gains = *eq_gains.borrow();
    let eq_adjs: Rc<Vec<gtk::Adjustment>> = {
        let mut adjs = Vec::with_capacity(10);
        for i in 0..10 {
            let band_box = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(2)
                .build();

            let freq_label = gtk::Label::new(Some(EQ_LABELS[i]));
            freq_label.add_css_class("dim-label");
            freq_label.add_css_class("caption");
            freq_label.set_width_chars(3);
            band_box.append(&freq_label);

            let adj = gtk::Adjustment::new(initial_gains[i], -12.0, 12.0, 0.5, 1.0, 0.0);
            let scale = gtk::Scale::builder()
                .orientation(gtk::Orientation::Vertical)
                .adjustment(&adj)
                .inverted(true)
                .draw_value(false)
                .height_request(160)
                .build();
            scale.add_mark(0.0, gtk::PositionType::Right, None);
            let band_name = match EQ_LABELS[i].strip_suffix('k') {
                Some(khz) => format!("{} {khz} kHz", i18n("Equalizer band")),
                None => format!("{} {} Hz", i18n("Equalizer band"), EQ_LABELS[i]),
            };
            scale.update_property(&[gtk::accessible::Property::Label(&band_name)]);
            band_box.append(&scale);

            let val_label = gtk::Label::builder()
                .label(format!("{:.0}", initial_gains[i]))
                .width_chars(3)
                .build();
            val_label.add_css_class("dim-label");
            val_label.add_css_class("caption");
            band_box.append(&val_label);

            {
                let gains = eq_gains.clone();
                let s = settings.clone();
                let m = mpv.clone();
                let gain_value_label = val_label;
                let updating = updating_from_preset.clone();
                let debounce = DebouncedAction::new(Duration::from_millis(150));
                let on_chg = on_change.clone();
                adj.connect_value_changed(move |adj| {
                    let value = adj.value();
                    gains.borrow_mut()[i] = value;
                    gain_value_label.set_label(&format!("{value:.0}"));

                    if updating.get() {
                        return;
                    }

                    let gains = gains.clone();
                    let s = s.clone();
                    let m = m.clone();
                    let on_chg2 = on_chg.clone();
                    debounce.run(move || {
                        let eq_str: String = gains
                            .borrow()
                            .iter()
                            .map(|g| format!("{g:.1}"))
                            .collect::<Vec<_>>()
                            .join(";");
                        let _ = s.put_string(keys::EQ_BANDS, &eq_str);
                        if let Some(ref mpv) = m {
                            rebuild_filters(&s, &gains.borrow(), mpv);
                        }
                        if let Some(ref cb) = on_chg2 {
                            cb();
                        }
                    });
                });
            }

            adjs.push(adj);
            sliders_box.append(&band_box);
        }
        Rc::new(adjs)
    };

    eq_area.append(&sliders_box);
    main_box.append(&eq_area);

    // ── Frequency range labels: Bass / Mids / Highs ──
    let range_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .margin_start(28) // offset for dB scale column
        .build();
    for (label, bands) in &[("Bass", 3), ("Mids", 4), ("Highs", 3)] {
        let band_range_label = gtk::Label::new(Some(&i18n(label)));
        band_range_label.add_css_class("dim-label");
        band_range_label.add_css_class("caption");
        band_range_label.set_hexpand(false);
        // ~width proportional to number of bands
        band_range_label.set_width_chars(*bands * 5);
        range_box.append(&band_range_label);
    }
    main_box.append(&range_box);

    // ── Preset combo handler ──
    {
        let adjs = eq_adjs.clone();
        let gains = eq_gains.clone();
        let s = settings.clone();
        let m = mpv.clone();
        let updating = updating_from_preset.clone();
        let on_chg = on_change.clone();
        preset_combo.connect_selected_notify(move |combo| {
            let idx = combo.selected() as usize;
            let all_presets = presets();
            if let Some(preset) = all_presets.get(idx) {
                let _ = s.put_string(keys::EQ_PRESET, &preset.name.to_lowercase());

                updating.set(true);

                {
                    let mut g = gains.borrow_mut();
                    for j in 0..10 {
                        g[j] = preset.gains[j];
                    }
                }
                for (j, adj) in adjs.iter().enumerate() {
                    adj.set_value(preset.gains[j]);
                }

                updating.set(false);

                let eq_str: String = gains
                    .borrow()
                    .iter()
                    .map(|g| format!("{g:.1}"))
                    .collect::<Vec<_>>()
                    .join(";");
                let _ = s.put_string(keys::EQ_BANDS, &eq_str);
                if let Some(ref mpv) = m {
                    rebuild_filters(&s, &gains.borrow(), mpv);
                }
                if let Some(ref cb) = on_chg {
                    cb();
                }
            }
        });
    }

    // ── Reset button handler ──
    {
        let adjs = eq_adjs;
        let gains = eq_gains;
        let s = settings.clone();
        let m = mpv;
        let updating = updating_from_preset;
        let on_chg = on_change;
        let pc = preset_combo;
        reset_button.connect_clicked(move |_| {
            pc.set_selected(0); // "Flat"
            updating.set(true);
            {
                let mut g = gains.borrow_mut();
                for j in 0..10 {
                    g[j] = 0.0;
                }
            }
            for adj in adjs.iter() {
                adj.set_value(0.0);
            }
            updating.set(false);
            let _ = s.put_string(keys::EQ_BANDS, "0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0;0.0");
            let _ = s.put_string(keys::EQ_PRESET, "flat");
            if let Some(ref mpv) = m {
                rebuild_filters(&s, &[0.0; 10], mpv);
            }
            if let Some(ref cb) = on_chg {
                cb();
            }
        });
    }

    let dialog = adw::Window::builder()
        .title(i18n("Equalizer"))
        .default_width(520)
        .modal(false)
        .build();

    let toolbar_pane = BigToolbarPane::new(BigToolbarPaneSpec::new());
    let header = BigToolbarHeader::new(BigToolbarHeaderSpec::new());
    toolbar_pane.add_top_bar(header.root());
    toolbar_pane.set_content(&main_box);
    let toolbar_view = toolbar_pane.into_root();
    dialog.set_content(Some(&toolbar_view));

    dialog
}
