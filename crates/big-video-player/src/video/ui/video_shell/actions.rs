use big_relm4_components::input::button_content::{BigButtonContentSpec, build_button};

use super::panel::PanelPage;
use super::*;

pub(super) fn install_window_actions(
    // The `win.*` actions are inserted on the content Root so they
    // travel with the content into whatever window hosts it.
    content: &gtk::Box,
    sender: &ComponentSender<VideoShell>,
) -> gio::SimpleActionGroup {
    let actions = gio::SimpleActionGroup::new();
    let pairs: &[(&str, fn() -> VideoShellInput)] = &[
        ("play-pause", || VideoShellInput::TogglePlayback),
        ("fullscreen", || VideoShellInput::ToggleFullscreen),
        ("open-file", || VideoShellInput::ShowOpenFile),
        ("open-url", || VideoShellInput::ShowOpenUrl),
        ("open-recent", || VideoShellInput::ShowRecent),
        ("open-disc", || VideoShellInput::ShowOpenDisc),
        ("toggle-files-sidebar", || {
            VideoShellInput::ToggleFilesSidebar
        }),
        ("toggle-playlist-sidebar", || {
            VideoShellInput::TogglePlaylistSidebar
        }),
        ("preferences", || VideoShellInput::ShowPreferences),
        ("appearance", || VideoShellInput::ShowAppearance),
        ("restore-defaults", || VideoShellInput::ShowRestoreDefaults),
        ("settings", || VideoShellInput::TogglePanel),
        ("filters", || VideoShellInput::ShowPanel(PanelPage::Effects)),
        ("show-image-adjust", || {
            VideoShellInput::ShowPanel(PanelPage::Picture)
        }),
        ("show-hdr", || VideoShellInput::ShowPanel(PanelPage::Hdr)),
        ("cast", || VideoShellInput::ShowPanel(PanelPage::Tv)),
        ("show-framing", || {
            VideoShellInput::ShowPanel(PanelPage::Framing)
        }),
        ("show-sound", || {
            VideoShellInput::ShowPanel(PanelPage::Sound)
        }),
        ("ytdl-quality", || VideoShellInput::ShowYtdlQuality),
        ("media-info", || VideoShellInput::ShowMediaInfo),
        ("iptv", || VideoShellInput::ShowIptvBrowser),
        ("keyboard-shortcuts", || {
            VideoShellInput::ShowKeyboardShortcuts
        }),
        ("goto-time", || VideoShellInput::ShowGotoTime),
        ("screenshot", || VideoShellInput::Screenshot),
        ("chapters", || {
            VideoShellInput::ShowPanel(PanelPage::Chapters)
        }),
        ("sleep-timer", || {
            VideoShellInput::ShowPanel(PanelPage::SleepTimer)
        }),
        ("trim", || VideoShellInput::EnterTrimMode),
        ("trim-mark", || VideoShellInput::TrimMark),
        ("trim-exit", || VideoShellInput::ExitTrimMode),
        ("extract-audio", || VideoShellInput::ExtractAudio),
        ("convert", || VideoShellInput::Convert),
    ];
    for (name, message) in pairs {
        let sender = sender.clone();
        let message = *message;
        big_app_kit::desktop::install_action(&actions, name, move || sender.input(message()));
    }
    // Stream Quality (yt-dlp format picker) only applies to online/streamed
    // media — start disabled; the shell enables it on a remote `FileLoaded` and
    // disables it again for local files (see playback.rs). Grays the menu entry.
    set_action_enabled(&actions, "ytdl-quality", false);
    // Cutting and converting happen in the converter GUIs; grey out what they
    // cannot receive because the GUI is not installed.
    let video_converter = converter_handoff::is_installed(converter_handoff::VIDEO_CONVERTER);
    set_action_enabled(&actions, "trim", video_converter);
    set_action_enabled(&actions, "convert", video_converter);
    set_action_enabled(
        &actions,
        "extract-audio",
        converter_handoff::is_installed(converter_handoff::AUDIO_CONVERTER),
    );

    content.insert_action_group("win", Some(&actions));
    actions
}

/// Enable/disable a `win.*` action by name (no-op if absent or not a
/// `SimpleAction`). Used to gate context-dependent entries (e.g. Stream Quality
/// for online media only).
pub(super) fn set_action_enabled(actions: &gio::SimpleActionGroup, name: &str, enabled: bool) {
    use gtk::prelude::ActionMapExt;
    if let Some(action) = actions
        .lookup_action(name)
        .and_downcast::<gio::SimpleAction>()
    {
        action.set_enabled(enabled);
    }
}

/// Build the centered welcome screen (icon + title + subtitle + Open File /
/// Open Recent / Open URL); buttons
/// fire the `win.*` actions installed by [`install_window_actions`].
pub(super) fn build_welcome_screen(sender: &ComponentSender<VideoShell>) -> gtk::Box {
    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(4)
        .css_classes(["welcome-container"])
        .build();

    let icon_circle = gtk::Box::builder()
        .halign(gtk::Align::Center)
        .css_classes(["welcome-icon-circle"])
        .build();
    let icon = gtk::Image::builder()
        .icon_name("media-playback-start-symbolic")
        .pixel_size(48)
        .halign(gtk::Align::Center)
        .css_classes(["welcome-icon"])
        .build();
    icon_circle.append(&icon);
    container.append(&icon_circle);

    let title = gtk::Label::builder()
        .label(i18n("Big Video Player"))
        .halign(gtk::Align::Center)
        .css_classes(["welcome-title"])
        .build();
    container.append(&title);

    let subtitle = gtk::Label::builder()
        .label(i18n("Drop a file or open one to start"))
        .halign(gtk::Align::Center)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["welcome-subtitle"])
        .build();
    container.append(&subtitle);

    let welcome_button_row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .spacing(10)
        .margin_top(24)
        .build();
    welcome_button_row.append(&welcome_button(
        "folder-open-symbolic",
        &i18n("Open File"),
        sender,
        || VideoShellInput::ShowOpenFile,
    ));
    welcome_button_row.append(&welcome_button(
        "document-open-recent-symbolic",
        &i18n("Open Recent"),
        sender,
        || VideoShellInput::ShowRecent,
    ));
    welcome_button_row.append(&welcome_button(
        "web-browser-symbolic",
        &i18n("Open URL"),
        sender,
        || VideoShellInput::ShowOpenUrl,
    ));
    container.append(&welcome_button_row);

    container
}

/// A welcome-screen action button (icon + label) that fires `message()`.
fn welcome_button(
    icon_name: &str,
    label: &str,
    sender: &ComponentSender<VideoShell>,
    message: fn() -> VideoShellInput,
) -> gtk::Button {
    let button = build_button(
        &BigButtonContentSpec::new(icon_name, label)
            .css_classes(["welcome-btn", "welcome-btn-primary"]),
    );
    button.set_halign(gtk::Align::Center);
    let sender = sender.clone();
    button.connect_clicked(move |_| sender.input(message()));
    button
}

/// Toggle the active window's keep-above state via the compositor, off the GTK
/// main thread. Tries the KWin global shortcut first, falls back to wmctrl.
/// Argv arrays only (no shell).
pub(super) fn toggle_keep_above() {
    use big_app_kit::subprocess::BigSubprocessSpec;
    big_media_components::player_chrome::spawn_blocking_task(move || {
        let kwin = BigSubprocessSpec::builder()
            .program("qdbus6")
            .args([
                "org.kde.kglobalaccel",
                "/component/kwin",
                "org.kde.kglobalaccel.Component.invokeShortcut",
                "Window Above Other Windows",
            ])
            .build()
            .run();
        if kwin.is_ok_and(|out| out.status.success()) {
            return;
        }
        let _ = BigSubprocessSpec::builder()
            .program("wmctrl")
            .args(["-r", ":ACTIVE:", "-b", "toggle,above"])
            .build()
            .run();
    });
}

/// Wire one of the two sidebar entry-point toggles: pressing it reveals the
/// sidebar in `mode` and releases the sibling; un-pressing it hides the sidebar.
/// The `updating` guard suppresses the re-entrant toggle from releasing the
/// sibling; the sibling is held weakly to avoid a button↔button cycle.
pub(super) fn wire_sidebar_toggle(
    btn: &gtk::ToggleButton,
    sibling: &gtk::ToggleButton,
    mode: SidebarMode,
    split_view: &adw::OverlaySplitView,
    updating: &Rc<Cell<bool>>,
    active_mode: &Rc<Cell<SidebarMode>>,
    pl_sender: &relm4::Sender<VideoBrowserSidebarInput>,
) {
    let sibling = sibling.downgrade();
    // Weak: the toggle lives inside the split view's content — a strong
    // split capture is a content⇄split cycle.
    let split_view = split_view.downgrade();
    let updating = updating.clone();
    let active_mode = active_mode.clone();
    let pl_sender = pl_sender.clone();
    btn.connect_toggled(move |b| {
        if updating.get() {
            return;
        }
        let Some(split_view) = split_view.upgrade() else {
            return;
        };
        if b.is_active() {
            active_mode.set(mode);
            let _ = pl_sender.send(VideoBrowserSidebarInput::SetMode(mode));
            split_view.set_show_sidebar(true);
            if let Some(sibling) = sibling.upgrade() {
                updating.set(true);
                sibling.set_active(false);
                updating.set(false);
            }
        } else {
            split_view.set_show_sidebar(false);
        }
    });
}
