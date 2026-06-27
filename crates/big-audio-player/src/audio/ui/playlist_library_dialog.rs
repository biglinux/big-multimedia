//! Playlist library dialog — a single premium dialog for everything playlist-related.
//!
//! Sidebar + content split. The sidebar holds the actions:
//! 1. **Save** the current queue as a new playlist.
//! 2. **Generate** from listening stats (most played / recently added / never
//!    played) — loads the set into the queue (save it with "Save Current Queue").
//! 3. **Import / Export** — single-file import, batch folder import, export-all.
//!
//! The content pane lists the saved playlists (load / rename / export / delete).
//!
//! Push-model Relm4 `Component`: the shell owns the DB + queue, so every action
//! is an `Output` intent; the shell performs it and pushes the refreshed list
//! back via `PlaylistLibraryDialogInput::SetPlaylists`.

use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::layout::{
    dialog_footer::{BigDialogFooter, BigDialogFooterSpec},
    dialog_page::BigDialogPage,
    preferences::BigPreferencesGroupSpec,
    sidebar_dialog::{BigSidebarDialog, BigSidebarDialogSpec},
    widget_container::clear_box_children,
};
use big_relm4_components::list::{
    bounded_list_box::{BigBoundedListBox, BigBoundedListBoxSpec},
    navigation_row::{BigNavigationRow, BigNavigationRowSpec},
};
use gtk::glib;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::i18n::i18n;

/// `(id, name, track_count, total_secs)` for one saved playlist.
pub type PlaylistEntry = (i64, String, i64, f64);

/// Smart-generator presets, keyed to the shell's `generate_smart_to_queue` kinds.
const GENERATORS: [(&str, &str, &str); 3] = [
    ("most-played", "Most Played", "starred-symbolic"),
    (
        "recently-added",
        "Recently Added",
        "document-open-recent-symbolic",
    ),
    ("never-played", "Never Played", "audio-x-generic-symbolic"),
];

/// Inputs pushed by the shell.
#[derive(Debug)]
pub enum PlaylistLibraryDialogInput {
    /// Replace the displayed list with the shell-resolved playlists.
    SetPlaylists(Vec<PlaylistEntry>),
}

/// Action intents emitted to the shell.
#[derive(Debug)]
pub enum PlaylistLibraryDialogOutput {
    /// Save the current queue as a new playlist.
    SaveCurrent { name: String },
    /// Load this playlist into the queue and play it.
    Load(i64),
    /// Rename this playlist.
    Rename { id: i64, name: String },
    /// Delete this playlist.
    Delete(i64),
    /// Export this playlist to a file (shell opens the save picker).
    Export(i64),
    /// Import a single playlist file as a new saved playlist.
    Import,
    /// Import every playlist file in a chosen folder.
    ImportFolder,
    /// Export all saved playlists to a chosen folder.
    ExportAll,
    /// Load a smart-generator preset (by kind) into the queue and play.
    GenerateLoad(String),
}

/// The playlist library dialog component.
pub struct PlaylistLibraryDialog {
    /// Container holding the (rebuilt) saved-playlists list / empty state.
    list_slot: gtk::Box,
    /// The reusable sidebar-dialog scaffold (for the live content title).
    layout: BigSidebarDialog,
}

impl Component for PlaylistLibraryDialog {
    type Init = ();
    type Input = PlaylistLibraryDialogInput;
    type Output = PlaylistLibraryDialogOutput;
    type Root = adw::Window;
    type Widgets = ();
    type CommandOutput = ();

    fn init_root() -> Self::Root {
        // A resizable `adw::Window` (like the Preferences window), not the fixed
        // `adw::Dialog`, so users can size the playlist list to taste.
        adw::Window::builder()
            .title(i18n("Manage Playlists"))
            .default_width(820)
            .default_height(640)
            .modal(true)
            .destroy_with_parent(true)
            .build()
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Close the window instead of destroying it (it's a reused controller).
        // Signal arg, no capture: a strong `root` clone here is a self-cycle
        // that pins the window past controller drop.
        root.connect_close_request(|win| {
            win.set_visible(false);
            glib::Propagation::Stop
        });

        // Canonical Adwaita sidebar layout, from the shared scaffold (no direct
        // NavigationSplitView assembly here).
        let layout = BigSidebarDialog::install(
            &root,
            &BigSidebarDialogSpec::new(i18n("Manage Playlists"))
                .content_title(i18n("Your Playlists")),
        );

        // ── Sidebar: save action sits in the (flat, title-less) header; the
        // generate + import/export groups fill the body. ──
        layout.set_sidebar_header(&build_save_button(&sender, &root));

        let sidebar_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        sidebar_box.append(&build_generate_group(&sender));
        sidebar_box.append(&build_io_group(&sender));
        layout.set_sidebar(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&sidebar_box)
                .build(),
        );

        // ── Content: the saved playlists (rebuilt on SetPlaylists) ──
        let list_slot = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .vexpand(true)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        layout.set_content(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .vexpand(true)
                .child(&list_slot)
                .build(),
        );

        ComponentParts {
            model: Self { list_slot, layout },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            PlaylistLibraryDialogInput::SetPlaylists(entries) => {
                self.rebuild(&entries, &sender, root);
            }
        }
    }
}

impl PlaylistLibraryDialog {
    /// Rebuild the saved-playlists section: a titled boxed-list, or empty state.
    fn rebuild(
        &self,
        entries: &[PlaylistEntry],
        sender: &ComponentSender<Self>,
        root: &adw::Window,
    ) {
        clear_box_children(&self.list_slot);

        if entries.is_empty() {
            self.layout.set_content_title(&i18n("Your Playlists"));
            let empty = adw::StatusPage::builder()
                .icon_name("view-list-symbolic")
                .title(i18n("No Saved Playlists"))
                .description(i18n(
                    "Save the current queue, generate one from your stats, or import a file",
                ))
                .vexpand(true)
                .build();
            empty.add_css_class("compact");
            self.list_slot.append(&empty);
            return;
        }

        // The page header carries the count; the list itself is untitled.
        self.layout
            .set_content_title(&format!("{} ({})", i18n("Your Playlists"), entries.len()));
        let saved_playlist_list =
            BigBoundedListBox::new(BigBoundedListBoxSpec::new(64).css_class("boxed-list"));
        let list = saved_playlist_list.root().clone();
        list.update_property(&[gtk::accessible::Property::Label(&i18n("Saved playlists"))]);
        for (id, name, count, secs) in entries {
            list.append(&build_playlist_row(*id, name, *count, *secs, sender, root));
        }
        self.list_slot.append(&list);
    }
}

/// "Import & Export" group, shown directly in the dialog (no overflow menu):
/// single-file import, batch folder import, and export-all.
fn build_io_group(sender: &ComponentSender<PlaylistLibraryDialog>) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Import / Export")).build();

    let rows: [(&str, &str, fn() -> PlaylistLibraryDialogOutput); 3] = [
        ("document-import-symbolic", "Import File…", || {
            PlaylistLibraryDialogOutput::Import
        }),
        ("folder-open-symbolic", "Import Folder…", || {
            PlaylistLibraryDialogOutput::ImportFolder
        }),
        ("document-save-symbolic", "Export All…", || {
            PlaylistLibraryDialogOutput::ExportAll
        }),
    ];
    for (icon, label, make) in rows {
        let title = i18n(label);
        let row = BigNavigationRow::new(
            BigNavigationRowSpec::new(title.as_str())
                .prefix_icon_name(icon)
                .action_label(title.as_str()),
        )
        .into_root();
        let out = sender.clone();
        row.connect_activated(move |_| {
            let _ = out.output(make());
        });
        group.add(&row);
    }
    group
}

/// Prominent "save current queue" pill.
fn build_save_button(
    sender: &ComponentSender<PlaylistLibraryDialog>,
    root: &adw::Window,
) -> gtk::Button {
    let save_button = gtk::Button::builder()
        .halign(gtk::Align::Center)
        .css_classes(["suggested-action", "pill"])
        .child(
            &adw::ButtonContent::builder()
                .icon_name("list-add-symbolic")
                .label(i18n("Save Current Queue"))
                .build(),
        )
        .build();
    let out = sender.clone();
    let root_weak = root.downgrade();
    save_button.connect_clicked(move |_| {
        prompt_name(
            root_weak.upgrade().as_ref(),
            &i18n("Save Current Queue"),
            &i18n("Playlist name"),
            &i18n("Name for the new playlist"),
            "",
            {
                let out = out.clone();
                move |name| {
                    let _ = out.output(PlaylistLibraryDialogOutput::SaveCurrent { name });
                }
            },
        );
    });
    save_button
}

/// "Generate" group: smart playlists from listening stats. Row activate loads
/// the set into the queue (save it afterwards with "Save Current Queue").
fn build_generate_group(sender: &ComponentSender<PlaylistLibraryDialog>) -> adw::PreferencesGroup {
    let group = BigPreferencesGroupSpec::new(i18n("Generate"))
        .description(i18n("Build a playlist from your listening stats"))
        .build();

    for (kind, title, icon) in GENERATORS {
        let title = i18n(title);
        let row = BigNavigationRow::new(
            BigNavigationRowSpec::new(title.as_str())
                .prefix_icon_name(icon)
                .action_label(title.as_str()),
        )
        .into_root();
        let (out, kind) = (sender.clone(), kind.to_string());
        row.connect_activated(move |_| {
            let _ = out.output(PlaylistLibraryDialogOutput::GenerateLoad(kind.clone()));
        });
        group.add(&row);
    }
    group
}

/// One saved-playlist row: title + stats, activatable to load, with rename /
/// export / delete suffix buttons.
fn build_playlist_row(
    id: i64,
    name: &str,
    count: i64,
    secs: f64,
    sender: &ComponentSender<PlaylistLibraryDialog>,
    root: &adw::Window,
) -> adw::ActionRow {
    let subtitle = format!(
        "{count} {} · {} {}",
        if count == 1 {
            i18n("track")
        } else {
            i18n("tracks")
        },
        (secs as i64) / 60,
        i18n("min"),
    );
    let row = BigNavigationRow::new(
        BigNavigationRowSpec::new(name)
            .subtitle(&subtitle)
            .action_label(i18n("Load")),
    )
    .into_root();
    {
        let out = sender.clone();
        row.connect_activated(move |_| {
            let _ = out.output(PlaylistLibraryDialogOutput::Load(id));
        });
    }

    let rename_button = suffix_button(
        "document-edit-symbolic",
        &format!("{}: {name}", i18n("Rename playlist")),
    );
    {
        let out = sender.clone();
        let root_weak = root.downgrade();
        let current = name.to_string();
        rename_button.connect_clicked(move |_| {
            prompt_name(
                root_weak.upgrade().as_ref(),
                &i18n("Rename Playlist"),
                &i18n("Playlist name"),
                &i18n("New name"),
                &current,
                {
                    let out = out.clone();
                    move |name| {
                        let _ = out.output(PlaylistLibraryDialogOutput::Rename { id, name });
                    }
                },
            );
        });
    }
    row.add_suffix(&rename_button);

    let export_button = suffix_button(
        "document-save-symbolic",
        &format!("{}: {name}", i18n("Export playlist")),
    );
    {
        let out = sender.clone();
        export_button.connect_clicked(move |_| {
            let _ = out.output(PlaylistLibraryDialogOutput::Export(id));
        });
    }
    row.add_suffix(&export_button);

    let delete_button = suffix_button(
        "user-trash-symbolic",
        &format!("{}: {name}", i18n("Delete playlist")),
    );
    delete_button.add_css_class("destructive-action");
    {
        let out = sender.clone();
        let root_weak = root.downgrade();
        let label = name.to_string();
        delete_button.connect_clicked(move |_| {
            confirm_delete(root_weak.upgrade().as_ref(), &label, {
                let out = out.clone();
                move || {
                    let _ = out.output(PlaylistLibraryDialogOutput::Delete(id));
                }
            });
        });
    }
    row.add_suffix(&delete_button);

    row
}

/// Flat, vertically-centred icon button for an `ActionRow` suffix.
fn suffix_button(icon: &str, tooltip: &str) -> gtk::Button {
    let suffix_button = gtk::Button::builder()
        .icon_name(icon)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&suffix_button, tooltip);
    suffix_button.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    suffix_button
}

/// Present a single-entry name dialog; `on_accept` fires with the trimmed,
/// non-empty name. `parent` anchors the dialog to the playlist library window.
fn prompt_name(
    parent: Option<&adw::Window>,
    heading: &str,
    entry_label: &str,
    placeholder: &str,
    prefill: &str,
    on_accept: impl Fn(String) + 'static,
) {
    let dialog = adw::Dialog::builder()
        .title(heading)
        .content_width(420)
        .content_height(240)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(placeholder);

    let group = BigPreferencesGroupSpec::untitled().build();
    let entry = adw::EntryRow::builder()
        .title(entry_label)
        .text(prefill)
        .show_apply_button(true)
        .build();
    group.add(&entry);
    page.content().append(&group);

    let footer = BigDialogFooter::new(&BigDialogFooterSpec::new(i18n("Cancel"), i18n("Save")));
    footer
        .accept_button()
        .set_sensitive(!prefill.trim().is_empty());
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let accept_button = footer.accept_button().clone();
    entry.connect_changed(move |row| {
        accept_button.set_sensitive(!row.text().trim().is_empty());
    });

    let accept_button = footer.accept_button().clone();
    entry.connect_apply(move |_| {
        if accept_button.is_sensitive() {
            accept_button.emit_clicked();
        }
    });

    let on_accept = Rc::new(on_accept);
    let entry_for_accept = entry.clone();
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        let name = entry_for_accept.text().trim().to_string();
        if name.is_empty() {
            return;
        }
        on_accept(name);
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let entry_for_focus = entry.clone();
    dialog.connect_map(move |_| {
        let entry_for_idle = entry_for_focus.clone();
        glib::idle_add_local_once(move || {
            entry_for_idle.grab_focus();
            entry_for_idle.select_region(0, -1);
        });
    });
    dialog.present(parent);
}

/// Present a destructive confirmation; `on_confirm` fires on accept.
fn confirm_delete(parent: Option<&adw::Window>, name: &str, on_confirm: impl Fn() + 'static) {
    let dialog = adw::Dialog::builder()
        .title(i18n("Delete Playlist?"))
        .content_width(420)
        .content_height(220)
        .build();
    let page = BigDialogPage::plain();
    page.content().set_spacing(12);
    page.append_description(&format!(
        "{} \u{201C}{name}\u{201D}?",
        i18n("This will permanently delete")
    ));

    let footer = BigDialogFooter::new(
        &BigDialogFooterSpec::new(i18n("Cancel"), i18n("Delete"))
            .with_accept_css_classes(&["destructive-action"]),
    );
    page.root().add_bottom_bar(footer.root());
    dialog.set_child(Some(page.root()));

    let on_confirm = Rc::new(on_confirm);
    let dialog_weak = dialog.downgrade();
    footer.accept_button().connect_clicked(move |_| {
        on_confirm();
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });

    let dialog_weak = dialog.downgrade();
    footer.cancel_button().connect_clicked(move |_| {
        if let Some(dialog) = dialog_weak.upgrade() {
            dialog.close();
        }
    });
    dialog.present(parent);
}
