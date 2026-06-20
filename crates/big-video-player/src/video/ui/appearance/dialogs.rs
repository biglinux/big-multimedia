//! Video appearance dialog.

use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

use crate::i18n::i18n;
use crate::settings::{keys, Settings};

use big_player_ui::appearance::create_video_colors_page;

#[allow(deprecated)]
pub fn show_appearance_dialog(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_changed: impl Fn() + 'static,
    on_closed: impl Fn() + 'static,
) {
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);

    let saved_w = settings.get_int(keys::APPEARANCE_WINDOW_WIDTH);
    let saved_h = settings.get_int(keys::APPEARANCE_WINDOW_HEIGHT);
    let width = if saved_w > 0 { saved_w } else { 550 };
    let height = if saved_h > 0 { saved_h } else { 600 };

    let win = adw::PreferencesWindow::builder()
        .title(i18n("Appearance"))
        .search_enabled(false)
        .default_width(width)
        .default_height(height)
        .transient_for(parent.as_ref())
        .modal(true)
        .build();

    win.add(&create_video_colors_page(settings, &on_changed));
    win.add(&super::layout::create_layout_page(settings, &on_changed));
    win.add(&super::layout::create_buttons_page(settings, &on_changed));

    let s = settings.clone();
    win.connect_close_request(move |w| {
        let _ = s.put_int(keys::APPEARANCE_WINDOW_WIDTH, w.width());
        let _ = s.put_int(keys::APPEARANCE_WINDOW_HEIGHT, w.height());
        on_closed();
        glib::Propagation::Proceed
    });

    win.present();
}
