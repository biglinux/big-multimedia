//! Video appearance dialog.

use std::rc::Rc;

use adw::prelude::IsA;

use crate::settings::{Settings, keys};

use big_player_ui::appearance::{create_video_colors_page, show_appearance_window};

pub fn show_appearance_dialog(
    parent: &impl IsA<gtk::Window>,
    settings: &Settings,
    on_changed: impl Fn() + 'static,
) {
    let on_changed: Rc<dyn Fn()> = Rc::new(on_changed);

    let saved = (
        settings.get_int(keys::APPEARANCE_WINDOW_WIDTH),
        settings.get_int(keys::APPEARANCE_WINDOW_HEIGHT),
    );

    let pages = [
        create_video_colors_page(settings, &on_changed),
        super::layout::create_layout_page(settings, &on_changed),
        super::layout::create_buttons_page(settings, &on_changed),
    ];

    let s = settings.clone();
    show_appearance_window(
        parent,
        saved,
        (550, 600),
        &pages,
        move |w, h| {
            let _ = s.put_int(keys::APPEARANCE_WINDOW_WIDTH, w);
            let _ = s.put_int(keys::APPEARANCE_WINDOW_HEIGHT, h);
        },
        || {},
    );
}
