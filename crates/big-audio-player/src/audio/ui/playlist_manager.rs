//! Playlist manager — a single premium dialog for everything playlist-related.
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
//! back via `PlaylistManagerInput::SetPlaylists`.

use adw::prelude::*;
use big_relm4_components::layout::sidebar_dialog::{BigSidebarDialog, BigSidebarDialogSpec};
use big_relm4_components::list::navigation_row::{BigNavigationRow, BigNavigationRowSpec};
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
pub enum PlaylistManagerInput {
    /// Replace the displayed list with the shell-resolved playlists.
    SetPlaylists(Vec<PlaylistEntry>),
}

/// Action intents emitted to the shell.
#[derive(Debug)]
pub enum PlaylistManagerOutput {
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

/// The playlist-manager dialog component.
pub struct PlaylistManager {
    /// Container holding the (rebuilt) saved-playlists list / empty state.
    list_slot: gtk::Box,
    /// The reusable sidebar-dialog scaffold (for the live content title).
    layout: BigSidebarDialog,
}

impl Component for PlaylistManager {
    type Init = ();
    type Input = PlaylistManagerInput;
    type Output = PlaylistManagerOutput;
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

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match msg {
            PlaylistManagerInput::SetPlaylists(entries) => {
                self.rebuild(&entries, &sender, root);
            }
        }
    }
}

impl PlaylistManager {
    /// Rebuild the saved-playlists section: a titled boxed-list, or empty state.
    fn rebuild(
        &self,
        entries: &[PlaylistEntry],
        sender: &ComponentSender<Self>,
        root: &adw::Window,
    ) {
        while let Some(child) = self.list_slot.first_child() {
            self.list_slot.remove(&child);
        }

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
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        for (id, name, count, secs) in entries {
            list.append(&build_playlist_row(*id, name, *count, *secs, sender, root));
        }
        self.list_slot.append(&list);
    }
}

/// "Import & Export" group, shown directly in the dialog (no overflow menu):
/// single-file import, batch folder import, and export-all.
fn build_io_group(sender: &ComponentSender<PlaylistManager>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Import / Export"))
        .build();

    let rows: [(&str, &str, fn() -> PlaylistManagerOutput); 3] = [
        ("document-import-symbolic", "Import File…", || {
            PlaylistManagerOutput::Import
        }),
        ("folder-open-symbolic", "Import Folder…", || {
            PlaylistManagerOutput::ImportFolder
        }),
        ("document-save-symbolic", "Export All…", || {
            PlaylistManagerOutput::ExportAll
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
fn build_save_button(sender: &ComponentSender<PlaylistManager>, root: &adw::Window) -> gtk::Button {
    let save_btn = gtk::Button::builder()
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
    save_btn.connect_clicked(move |_| {
        prompt_name(
            root_weak.upgrade().as_ref(),
            &i18n("Save Current Queue"),
            &i18n("Name for the new playlist"),
            "",
            {
                let out = out.clone();
                move |name| {
                    let _ = out.output(PlaylistManagerOutput::SaveCurrent { name });
                }
            },
        );
    });
    save_btn
}

/// "Generate" group: smart playlists from listening stats. Row activate loads
/// the set into the queue (save it afterwards with "Save Current Queue").
fn build_generate_group(sender: &ComponentSender<PlaylistManager>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(i18n("Generate"))
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
            let _ = out.output(PlaylistManagerOutput::GenerateLoad(kind.clone()));
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
    sender: &ComponentSender<PlaylistManager>,
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
            let _ = out.output(PlaylistManagerOutput::Load(id));
        });
    }

    let rename_btn = suffix_button("document-edit-symbolic", &i18n("Rename"));
    {
        let out = sender.clone();
        let root_weak = root.downgrade();
        let current = name.to_string();
        rename_btn.connect_clicked(move |_| {
            prompt_name(
                root_weak.upgrade().as_ref(),
                &i18n("Rename Playlist"),
                &i18n("New name"),
                &current,
                {
                    let out = out.clone();
                    move |name| {
                        let _ = out.output(PlaylistManagerOutput::Rename { id, name });
                    }
                },
            );
        });
    }
    row.add_suffix(&rename_btn);

    let export_btn = suffix_button("document-save-symbolic", &i18n("Export"));
    {
        let out = sender.clone();
        export_btn.connect_clicked(move |_| {
            let _ = out.output(PlaylistManagerOutput::Export(id));
        });
    }
    row.add_suffix(&export_btn);

    let delete_btn = suffix_button("user-trash-symbolic", &i18n("Delete"));
    delete_btn.add_css_class("destructive-action");
    {
        let out = sender.clone();
        let root_weak = root.downgrade();
        let label = name.to_string();
        delete_btn.connect_clicked(move |_| {
            confirm_delete(root_weak.upgrade().as_ref(), &label, {
                let out = out.clone();
                move || {
                    let _ = out.output(PlaylistManagerOutput::Delete(id));
                }
            });
        });
    }
    row.add_suffix(&delete_btn);

    row
}

/// Flat, vertically-centred icon button for an `ActionRow` suffix.
fn suffix_button(icon: &str, tooltip: &str) -> gtk::Button {
    let btn = gtk::Button::builder()
        .icon_name(icon)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    big_relm4_components::feedback::tooltip::set(&btn, tooltip);
    btn.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    btn
}

/// Present a single-entry name dialog; `on_accept` fires with the trimmed,
/// non-empty name. `parent` anchors the dialog to the manager.
fn prompt_name(
    parent: Option<&adw::Window>,
    heading: &str,
    placeholder: &str,
    prefill: &str,
    on_accept: impl Fn(String) + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(heading), None);
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .text(prefill)
        .activates_default(true)
        .build();
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("accept", &i18n("Save"));
    dialog.set_response_appearance("accept", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("accept"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, resp| {
        if resp != "accept" {
            return;
        }
        let name = entry.text().trim().to_string();
        if !name.is_empty() {
            on_accept(name);
        }
    });
    dialog.present(parent);
}

/// Present a destructive confirmation; `on_confirm` fires on accept.
fn confirm_delete(parent: Option<&adw::Window>, name: &str, on_confirm: impl Fn() + 'static) {
    let dialog = adw::AlertDialog::new(
        Some(&i18n("Delete Playlist?")),
        Some(&format!(
            "{} \u{201C}{name}\u{201D}?",
            i18n("This will permanently delete")
        )),
    );
    dialog.add_response("cancel", &i18n("Cancel"));
    dialog.add_response("delete", &i18n("Delete"));
    dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, move |_, resp| {
        if resp == "delete" {
            on_confirm();
        }
    });
    dialog.present(parent);
}
