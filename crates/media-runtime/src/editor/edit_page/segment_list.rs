//! Trim-segment list builder: per-row goto / edit / remove buttons backed
//! by `SegListActions`. The edit dialog uses h/m/s/cs SpinButtons because
//! frame-accurate trim requires sub-second precision.

use adw::prelude::*;
use big_relm4_components::list::{
    bounded_list_box::clear_list_box_rows,
    button_strip_row::{BigButtonStripRow, BigButtonStripRowSpec},
};

use gtk::gio;

use crate::i18n::i18n;

use super::TrimSegment;
use super::fire;
use super::segments::SegListActions;
use super::time_format::format_time_ms;

pub(super) fn update_segments_listbox(
    listbox: &gtk::ListBox,
    segments: &[TrimSegment],
    placeholder: &adw::ActionRow,
    list_row: &adw::ActionRow,
    actions: Option<&SegListActions>,
) {
    clear_list_box_rows(listbox);

    if segments.is_empty() {
        placeholder.set_visible(true);
        list_row.set_visible(false);
        return;
    }

    placeholder.set_visible(false);
    list_row.set_visible(true);

    for (idx, segment) in segments.iter().enumerate() {
        let duration = segment.end - segment.start;
        let row_builder = BigButtonStripRow::new(
            BigButtonStripRowSpec::new(format!(
                "{} → {}",
                format_time_ms(segment.start),
                format_time_ms(segment.end)
            ))
            .subtitle(format_time_ms(duration)),
        );

        if let Some(context) = actions {
            let goto_button = gtk::Button::builder()
                .icon_name("media-playback-start-symbolic")
                .css_classes(["flat", "circular"])
                .valign(gtk::Align::Center)
                .build();
            crate::shared::tooltip::set(&goto_button, &i18n("Go to segment"));
            goto_button
                .update_property(&[gtk::accessible::Property::Label(&i18n("Go to segment"))]);
            {
                let mpv = context.mpv.clone();
                let context = context.clone();
                let start = segment.start;
                goto_button.connect_clicked(move |_| {
                    if let Some(widgets) = context.widgets() {
                        widgets.timeline.set_value(start);
                    }
                    if let Some(ref mpv) = *mpv.borrow() {
                        mpv.seek_absolute(start);
                    }
                });
            }
            row_builder.append(&goto_button);

            let edit_button = gtk::Button::builder()
                .icon_name("document-edit-symbolic")
                .css_classes(["flat", "circular"])
                .valign(gtk::Align::Center)
                .build();
            crate::shared::tooltip::set(&edit_button, &i18n("Edit segment times"));
            edit_button.update_property(&[gtk::accessible::Property::Label(&i18n(
                "Edit segment times",
            ))]);
            {
                let context = context.clone();
                edit_button.connect_clicked(move |button| {
                    show_segment_edit_dialog(button, idx, &context);
                });
            }
            row_builder.append(&edit_button);

            let remove_button = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .css_classes(["flat", "circular", "error"])
                .valign(gtk::Align::Center)
                .build();
            crate::shared::tooltip::set(&remove_button, &i18n("Remove segment"));
            remove_button
                .update_property(&[gtk::accessible::Property::Label(&i18n("Remove segment"))]);
            {
                let context = context.clone();
                remove_button.connect_clicked(move |_| {
                    {
                        let mut segments = context.segments.borrow_mut();
                        if idx < segments.len() {
                            segments.remove(idx);
                        }
                    }
                    if let Some(widgets) = context.widgets() {
                        update_segments_listbox(
                            &widgets.listbox,
                            &context.segments.borrow(),
                            &widgets.placeholder,
                            &widgets.list_row,
                            Some(&context),
                        );
                        widgets.overlay.queue_draw();
                    }
                    fire(&context.on_changed, context.segments.borrow().clone());
                });
            }
            row_builder.append(&remove_button);
        }

        let row = row_builder.into_root();
        listbox.append(&row);
    }
}

fn show_segment_edit_dialog(widget: &gtk::Button, idx: usize, context: &SegListActions) {
    let segments = context.segments.borrow();
    let Some(segment) = segments.get(idx) else {
        return;
    };
    let start = segment.start;
    let end = segment.end;
    let duration = context.duration.get();
    drop(segments);

    let (dialog, content) = big_app_kit::dialogs::content_action_dialog(
        &format!("{} {}", i18n("Edit Segment"), idx + 1),
        &i18n("Cancel"),
        "save",
        &i18n("Save"),
        false,
    );

    let grid = gtk::Grid::builder()
        .row_spacing(8)
        .column_spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(8)
        .build();

    grid.attach(&gtk::Label::new(Some(&i18n("Start:"))), 0, 0, 1, 1);
    let (start_h, start_m, start_s, start_cs) = time_spinbuttons(start, duration);
    // Stage 8: time-unit abbreviations are universal (SI / common locale
    // form for hours/minutes/seconds/centiseconds). Route through `i18n()`
    // so a locale that prefers a different letter (e.g. `с` Cyrillic for
    // seconds) can override them.
    grid.attach(&start_h, 1, 0, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("h"))), 2, 0, 1, 1);
    grid.attach(&start_m, 3, 0, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("m"))), 4, 0, 1, 1);
    grid.attach(&start_s, 5, 0, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("s"))), 6, 0, 1, 1);
    grid.attach(&start_cs, 7, 0, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("cs"))), 8, 0, 1, 1);

    grid.attach(&gtk::Label::new(Some(&i18n("End:"))), 0, 1, 1, 1);
    let (end_h, end_m, end_s, end_cs) = time_spinbuttons(end, duration);
    grid.attach(&end_h, 1, 1, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("h"))), 2, 1, 1, 1);
    grid.attach(&end_m, 3, 1, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("m"))), 4, 1, 1, 1);
    grid.attach(&end_s, 5, 1, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("s"))), 6, 1, 1, 1);
    grid.attach(&end_cs, 7, 1, 1, 1);
    grid.attach(&gtk::Label::new(Some(&i18n("cs"))), 8, 1, 1, 1);

    content.append(&grid);

    let context = context.clone();
    let root = widget.root();
    dialog.choose(
        root.and_then(|root| root.downcast::<gtk::Window>().ok())
            .as_ref(),
        None::<&gio::Cancellable>,
        move |response| {
            if response != "save" {
                return;
            }
            let new_start = spinbuttons_to_secs(&start_h, &start_m, &start_s, &start_cs);
            let new_end = spinbuttons_to_secs(&end_h, &end_m, &end_s, &end_cs);
            if new_start >= new_end {
                return;
            }
            {
                let mut segments = context.segments.borrow_mut();
                if let Some(segment) = segments.get_mut(idx) {
                    segment.start = new_start;
                    segment.end = new_end;
                }
            }
            if let Some(widgets) = context.widgets() {
                update_segments_listbox(
                    &widgets.listbox,
                    &context.segments.borrow(),
                    &widgets.placeholder,
                    &widgets.list_row,
                    Some(&context),
                );
                widgets.overlay.queue_draw();
            }
            fire(&context.on_changed, context.segments.borrow().clone());
        },
    );
}

fn time_spinbuttons(
    secs: f64,
    max_duration: f64,
) -> (
    gtk::SpinButton,
    gtk::SpinButton,
    gtk::SpinButton,
    gtk::SpinButton,
) {
    let total_centiseconds = (secs * 100.0).round() as u64;
    let cs = (total_centiseconds % 100) as f64;
    let total_seconds = total_centiseconds / 100;
    let s = (total_seconds % 60) as f64;
    let total_minutes = total_seconds / 60;
    let m = (total_minutes % 60) as f64;
    let h = (total_minutes / 60) as f64;
    let max_h = (max_duration / 3600.0).ceil();

    let spin = |value, max, width| {
        let adjustment = gtk::Adjustment::new(value, 0.0, max, 1.0, 10.0, 0.0);
        gtk::SpinButton::builder()
            .adjustment(&adjustment)
            .numeric(true)
            .width_chars(width)
            .build()
    };

    (
        spin(h, max_h, 2),
        spin(m, 59.0, 2),
        spin(s, 59.0, 2),
        spin(cs, 99.0, 2),
    )
}

fn spinbuttons_to_secs(
    h: &gtk::SpinButton,
    m: &gtk::SpinButton,
    s: &gtk::SpinButton,
    cs: &gtk::SpinButton,
) -> f64 {
    h.value() * 3600.0 + m.value() * 60.0 + s.value() + cs.value() / 100.0
}
