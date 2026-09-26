//! General page: what happens during and after playback, the window, screen
//! lock, screenshots, and the reset-everything action.

use adw::prelude::*;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};

use super::{add_combo_row, add_switch_row, live, live_str};

pub(super) fn page(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    on_restore_defaults: impl Fn() + 'static,
) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(i18n("General"))
        .icon_name("emblem-system-symbolic")
        .build();

    let playback = adw::PreferencesGroup::builder()
        .title(i18n("Playback"))
        .build();
    add_switch_row(
        &playback,
        &i18n("Resume Where You Left Off"),
        &i18n("Reopen files at the point where you stopped watching"),
        "resume-playback",
        settings,
        live(mpv, MpvBackend::set_save_position),
    );
    add_combo_row(
        &playback,
        &i18n("Repeat"),
        "",
        &[
            &i18n("Off"),
            &i18n("Current video"),
            &i18n("Whole playlist"),
        ],
        &["no", "file", "playlist"],
        keys::LOOP_MODE.0,
        settings,
        live_str(mpv, MpvBackend::set_loop_mode),
    );
    page.add(&playback);

    let ending = adw::PreferencesGroup::builder()
        .title(i18n("When Playback Ends"))
        .description(i18n(
            "Playlist videos play one after another unless they stop after each",
        ))
        .build();
    let end_row = add_combo_row(
        &ending,
        &i18n("When a Video Ends"),
        "",
        &[
            &i18n("Keep the last frame"),
            &i18n("Stop after each"),
            &i18n("Close the video"),
        ],
        &["yes", "always", "no"],
        "keep-open",
        settings,
        live_str(mpv, MpvBackend::set_keep_open),
    );
    let idle_mpv = mpv.cloned();
    let idle_row = add_combo_row(
        &ending,
        &i18n("When the Playlist Finishes"),
        &i18n("Used only when the video closes"),
        &[
            &i18n("Stay open"),
            &i18n("Close the player"),
            &i18n("Close after one playlist"),
        ],
        &["yes", "no", "once"],
        keys::IDLE_MODE.0,
        settings,
        move |value| {
            // Applied now only with a file loaded; otherwise at the next load
            // (an empty playlist plus `idle=no` would make mpv quit).
            if let Some(mpv) = &idle_mpv
                && mpv.get_current_file().is_some()
            {
                mpv.set_idle(value);
            }
        },
    );
    // Holding the last frame means mpv never reaches its idle step.
    end_row
        .bind_property("selected", &idle_row, "sensitive")
        .transform_to(|_, selected: u32| Some(selected == 2))
        .sync_create()
        .build();
    page.add(&ending);

    let window = adw::PreferencesGroup::builder()
        .title(i18n("Window and Screen"))
        .build();
    add_combo_row(
        &window,
        &i18n("Keep the Screen On"),
        &i18n("Stops the screen from dimming or locking"),
        &[&i18n("During playback"), &i18n("Always"), &i18n("Never")],
        &["yes", "always", "no"],
        "stop-screensaver",
        settings,
        // The video shell holds the inhibitor; it re-reads this on every
        // play, pause and end of file, and when the preferences close.
        |_| {},
    );
    add_switch_row(
        &window,
        &i18n("Fit Window to Video"),
        &i18n("Keeps the window in the video's proportions, without black bars"),
        keys::AUTO_WINDOW_RESIZE.0,
        settings,
        // Read by the video shell's aspect snap, not by mpv.
        |_| {},
    );
    add_switch_row(
        &window,
        &i18n("Preview While Seeking"),
        &i18n("Show a thumbnail of the video when pointing at the progress bar"),
        keys::SEEKBAR_THUMBNAIL.0,
        settings,
        |_| {},
    );
    page.add(&window);

    page.add(&screenshot_group(settings));

    let reset = adw::PreferencesGroup::new();
    let restore = adw::ButtonRow::builder()
        .title(i18n("Restore Default Settings…"))
        .build();
    // `css_classes` in a builder would replace the row's own style classes.
    restore.add_css_class("destructive-action");
    restore.connect_activated(move |_| on_restore_defaults());
    reset.add(&restore);
    page.add(&reset);

    page
}

fn screenshot_group(settings: &Settings) -> adw::PreferencesGroup {
    // Screenshots are always PNG: the player validates the file mpv writes as
    // PNG before publishing it, so there is no format to choose.
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Screenshots"))
        .build();
    group.add(&screenshot_folder_row(settings));
    group
}

const SCREENSHOT_DIRECTORY: &str = "screenshot-directory";

/// Folder row: shows where screenshots go and opens the folder chooser.
fn screenshot_folder_row(settings: &Settings) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(i18n("Save To"))
        .subtitle_selectable(true)
        .build();
    let choose = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(i18n("Choose Folder"))
        .valign(gtk::Align::Center)
        .build();
    let reset = gtk::Button::builder()
        .icon_name("edit-undo-symbolic")
        .tooltip_text(i18n("Use the Pictures Folder"))
        .valign(gtk::Align::Center)
        .build();
    choose.add_css_class("flat");
    reset.add_css_class("flat");
    row.add_suffix(&reset);
    row.add_suffix(&choose);
    row.set_activatable_widget(Some(&choose));

    let show = {
        let row = row.downgrade();
        let reset = reset.downgrade();
        move |folder: &str| {
            if let Some(row) = row.upgrade() {
                row.set_subtitle(&if folder.is_empty() {
                    i18n("Pictures folder")
                } else {
                    glib::markup_escape_text(folder).to_string()
                });
            }
            if let Some(reset) = reset.upgrade() {
                reset.set_visible(!folder.is_empty());
            }
        }
    };
    show(&settings.string(SCREENSHOT_DIRECTORY));

    // Read by `take_screenshot` for each capture, so nothing to apply live.
    let save = {
        let settings = settings.clone();
        move |folder: &str| {
            let _ = settings.set_string(SCREENSHOT_DIRECTORY, folder);
            show(folder);
        }
    };
    let save = std::rc::Rc::new(save);
    {
        let save = save.clone();
        reset.connect_clicked(move |_| save(""));
    }
    choose.connect_clicked(move |button| {
        let dialog = gtk::FileDialog::builder()
            .title(i18n("Screenshot Folder"))
            .modal(true)
            .build();
        let parent = button.root().and_downcast::<gtk::Window>();
        let save = save.clone();
        dialog.select_folder(
            parent.as_ref(),
            gtk::gio::Cancellable::NONE,
            move |result| {
                if let Some(path) = result.ok().and_then(|folder| folder.path())
                    && let Some(folder) = path.to_str()
                {
                    save(folder);
                }
            },
        );
    });
    row
}
