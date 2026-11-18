//! The Edit Cut dialog both players open from a cut chip.

use std::rc::Rc;

use adw::prelude::*;
use big_media_data::i18n::i18n;
use big_relm4_components::input::preference_cards::BigSpinRange;
use big_relm4_components::input::spin_row::{BigSpinRow, BigSpinRowSpec};
use big_relm4_components::layout::dialog_footer::{BigDialogFooter, BigDialogFooterSpec};
use big_relm4_components::layout::dialog_page::BigDialogPage;
use big_relm4_components::layout::preferences::BigPreferencesGroupSpec;
use gtk::glib;

use crate::cut_list::{Cut, cut_span};

/// Open the Edit Cut dialog for `cut` on `parent`: start and end within
/// `[0, duration]` seconds, each with a button that copies `playhead()`.
/// Apply hands `apply` the cut's id and its times in order; a pair shorter than
/// [`crate::cut_list::MIN_CUT_MILLISECONDS`] keeps the dialog open with an error.
pub fn present_cut_edit_dialog(
    parent: &impl IsA<gtk::Widget>,
    cut: Cut,
    duration: f64,
    playhead: impl Fn() -> f64 + 'static,
    apply: impl Fn(u64, f64, f64) + 'static,
) {
    let playhead: Rc<dyn Fn() -> f64> = Rc::new(playhead);
    let range = BigSpinRange::new(0.0, duration, 0.1, 5.0);
    let start_row = BigSpinRow::new(
        BigSpinRowSpec::new(i18n("Start (seconds)"), cut.start, range)
            .digits(2)
            .width_chars(7),
    );
    let end_row = BigSpinRow::new(
        BigSpinRowSpec::new(i18n("End (seconds)"), cut.end, range)
            .digits(2)
            .width_chars(7),
    );
    start_row
        .row()
        .add_suffix(&playhead_button(&playhead, start_row.spin()));
    end_row
        .row()
        .add_suffix(&playhead_button(&playhead, end_row.spin()));
    let start_spin = start_row.spin().clone();
    let end_spin = end_row.spin().clone();

    let group = BigPreferencesGroupSpec::new(i18n("Cut range"))
        .description(i18n(
            "Use the clock buttons to copy the current playback position",
        ))
        .build();
    group.add(start_row.row());
    group.add(end_row.row());

    let too_short = i18n("Start and end must be at least 0.1 seconds apart");
    let error_label = gtk::Label::builder()
        .label(&too_short)
        .halign(gtk::Align::Start)
        .margin_start(12)
        .margin_end(12)
        .wrap(true)
        .selectable(true)
        .visible(false)
        .css_classes(["error"])
        .build();
    error_label.update_property(&[gtk::accessible::Property::Label(&too_short)]);

    let dialog = big_gtk_kit::dialog::Dialog::builder()
        .title(i18n("Edit Cut"))
        .content_width(440)
        .content_height(360)
        .build();
    let page = BigDialogPage::scrolled_clamped(420, 320);
    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Apply")));
    page.content().set_spacing(12);
    page.content().append(&group);
    page.content().append(&error_label);
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(&page.into_root()));

    let dialog_ref = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_ref.upgrade() {
            dialog.close();
        }
    });

    let dialog_ref = dialog.downgrade();
    footer.accept_button().connect_clicked(glib::clone!(
        #[weak]
        start_spin,
        #[weak]
        end_spin,
        #[weak]
        error_label,
        move |_| {
            let Some((start, end)) = cut_span(start_spin.value(), end_spin.value()) else {
                error_label.set_visible(true);
                end_spin.grab_focus();
                return;
            };
            apply(cut.id, start, end);
            if let Some(dialog) = dialog_ref.upgrade() {
                dialog.close();
            }
        }
    ));

    dialog.present(Some(parent));
}

fn playhead_button(playhead: &Rc<dyn Fn() -> f64>, spin: &gtk::SpinButton) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name("preferences-system-time-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.set_tooltip_text(Some(&i18n("Use current position")));
    button.update_property(&[gtk::accessible::Property::Label(&i18n(
        "Use current playback position",
    ))]);
    let playhead = Rc::clone(playhead);
    let spin = spin.downgrade();
    button.connect_clicked(move |_| {
        if let Some(spin) = spin.upgrade() {
            spin.set_value(playhead());
        }
    });
    button
}
