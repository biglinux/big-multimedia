//! Shared resizable preferences-window chrome for the audio and video players.
//! Both the "Appearance" and "Preferences" dialogs in each player hand-rolled an
//! identical `adw::PreferencesWindow` (size memory + close-persist). This
//! consolidates that chrome; pages and the settings-key wiring stay in the app
//! (passed as `pages` + closures), so the helper is free of any concrete
//! `Settings` type.
//!
//! `adw::PreferencesWindow` (libadwaita 1.6) is used over
//! `PreferencesDialog`: the dialog variant is not user-resizable, and the size
//! memory below is the whole point.

use adw::prelude::*;

/// Build a resizable preferences window: restore the saved size (falling back to
/// `default_size` per dimension when unset/`<= 0`), add `pages` in order, and on
/// close persist the final size via `persist_size` then run `on_closed`. The
/// window is returned **not yet presented** so the caller can post-process it
/// (e.g. inject a header-bar widget) before calling `present()`.
#[allow(deprecated)]
pub fn build_preferences_window(
    parent: &impl IsA<gtk::Window>,
    title: &str,
    search_enabled: bool,
    saved_size: (i32, i32),
    default_size: (i32, i32),
    pages: &[adw::PreferencesPage],
    persist_size: impl Fn(i32, i32) + 'static,
    on_closed: impl Fn() + 'static,
) -> adw::PreferencesWindow {
    let width = if saved_size.0 > 0 {
        saved_size.0
    } else {
        default_size.0
    };
    let height = if saved_size.1 > 0 {
        saved_size.1
    } else {
        default_size.1
    };

    let win = adw::PreferencesWindow::builder()
        .title(title)
        .search_enabled(search_enabled)
        .default_width(width)
        .default_height(height)
        .transient_for(parent.as_ref())
        .modal(true)
        .build();

    for page in pages {
        win.add(page);
    }

    win.connect_close_request(move |w| {
        persist_size(w.width(), w.height());
        on_closed();
        gtk::glib::Propagation::Proceed
    });

    win
}

/// Present the shared "Appearance" window (search disabled). Thin wrapper over
/// [`build_preferences_window`] for the common case with no post-build header
/// injection.
pub fn show_appearance_window(
    parent: &impl IsA<gtk::Window>,
    saved_size: (i32, i32),
    default_size: (i32, i32),
    pages: &[adw::PreferencesPage],
    persist_size: impl Fn(i32, i32) + 'static,
    on_closed: impl Fn() + 'static,
) {
    let win = build_preferences_window(
        parent,
        &big_media_data::i18n::i18n("Appearance"),
        false,
        saved_size,
        default_size,
        pages,
        persist_size,
        on_closed,
    );
    win.present();
}
