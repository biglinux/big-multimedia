use super::actions::{
    build_welcome_screen, install_window_actions, toggle_keep_above, wire_sidebar_toggle,
};
#[cfg(feature = "chromecast")]
use super::cast_snapshot::build_cast_filter_snapshot;
use super::dialogs::reapply_theme;
use super::input::{clear_shell_input, set_shell_input};
use super::input_gestures::{
    wire_background_video_input, wire_osd_autohide, wire_osd_focus_out, wire_video_input,
};
use super::render::{FastRenderContextSlot, VideoRealizeOptions, wire_close, wire_realize};
use super::*;
use big_media_components::surface::{
    BigMediaChrome, BigMediaChromePlacement, BigMediaMenuSpec, BigMediaOverlayStack,
    BigMediaSidebarShell, BigMediaSidebarSpec, create_media_toast_overlay_root,
    mount_media_toast_content, toggle_surface_fullscreened,
};
#[cfg(all(test, not(miri)))]
use big_relm4_components::layout::hamburger_menu::build_flat_menu_model;
use big_relm4_components::layout::hamburger_menu::{
    BigHamburgerMenuSpec, BigMenuActionItem, build_flat_action_popover_button,
};

impl Component for VideoShell {
    type Init = VideoShellMode;
    type Input = VideoShellInput;
    type Output = ();
    type CommandOutput = ();
    // Root is the mountable CONTENT (the toast overlay), not a window: the window
    // chrome + window-scoped setup are supplied / resolved by the mount point
    // (standalone boot, host module, or pane) — RESTRUCTURE ADR D14.
    type Root = adw::ToastOverlay;
    type Widgets = ();

    fn init_root() -> Self::Root {
        create_media_toast_overlay_root()
    }

    fn init(
        mode: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = Settings::for_video();
        let is_background = mode.is_background();

        // App stylesheet + icon path (the god-object loaded these; the shell
        // owns them now). Scoped CSS class for black bg + rounded corners.
        if let Some(display) = gtk::gdk::Display::default() {
            let provider = gtk::CssProvider::new();
            provider.load_from_resource(&format!(
                "{}/style.css",
                crate::config::video::RESOURCE_PREFIX
            ));
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::IconTheme::for_display(&display)
                .add_resource_path(&format!("{}/icons", crate::config::video::RESOURCE_PREFIX));
        }
        root.add_css_class("big-video-player");
        if is_background {
            root.add_css_class("big-video-background");
        }

        // Shared mpv handle: created here so the controls-bar popovers can read
        // it at call time, and so the realize handler can fill it.
        let mpv: Rc<RefCell<Option<Arc<MpvBackend>>>> = Rc::new(RefCell::new(None));
        let video_transform = media_adjust::VideoTransformState::default();

        // Interactive trim bar — built first so its widget can be mounted INSIDE
        // the controls bar (shown in place of the transport row in trim mode, not
        // a separate bar). Its typed outputs route back into the shell.
        let trim = trim::VideoTrimBar::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::TrimBar);

        // OSD controls bar (app-local; recovers the old `controls_box`). The trim
        // bar rides inside it (transport ↔ trim stack).
        let controls = VideoControlsBar::builder()
            .launch(VideoControlsBarInit {
                settings: settings.clone(),
                mpv: mpv.clone(),
                trim_content: trim.widget().clone().upcast::<gtk::Widget>(),
            })
            .forward(sender.input_sender(), VideoShellInput::Controls);
        controls.emit(VideoControlsBarInput::SetControlsSensitive(false));

        // Folder-browser playlist sidebar (folder scan = the playlist).
        let playlist = VideoBrowserSidebar::builder()
            .launch(settings.clone())
            .forward(sender.input_sender(), VideoShellInput::Playlist);

        // IPTV browser (video-local). Held detached as a dialog child; play
        // intents route back to the shell → mpv.
        let iptv = VideoIptvBrowser::builder()
            .launch(())
            .forward(sender.input_sender(), VideoShellInput::Iptv);

        // Cast device picker (video-local UI over the shared CastSessionController
        // engine). Held detached as a dialog child; start/stop reported back.
        #[cfg(feature = "chromecast")]
        let cast = VideoCastDialog::builder()
            .launch(VideoCastDialogInit {
                cast_engine: Arc::new(CastSessionController::new()),
                mpv: mpv.clone(),
                filter_snapshot: {
                    let mpv = mpv.clone();
                    let settings = settings.clone();
                    let video_transform = video_transform.clone();
                    Rc::new(move || build_cast_filter_snapshot(&mpv, &settings, &video_transform))
                },
            })
            .forward(sender.input_sender(), VideoShellInput::Cast);

        // Shared re-encode options (container/codec/quality) — same vocabulary as
        // the full converter, shown in the gear settings dialog (built lazily).
        let trim_encode = big_quick_export::encode_options::EncodeOptions::new();

        // Video surface: a black bg box is the overlay's main child; the GLArea
        // is an overlay child (the offload path hides it and stacks a
        // GraphicsOffload widget). The transport rides as a bottom overlay.
        let video_bg = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .vexpand(true)
            .build();
        video_bg.add_css_class("background");

        let gl_area = gtk::GLArea::new();

        let media_stack = BigMediaOverlayStack::new(&video_bg);
        let main_overlay = media_stack.overlay().clone();
        media_stack.add_primary_media_surface(&gl_area);

        // Software (no-GL) video surface: hidden until the GL render context
        // fails to come up (headless software compositor, GL-less VM). The SW
        // render path paints mpv frames here as memory textures (cairo path).
        let sw_picture = gtk::Picture::builder()
            .content_fit(gtk::ContentFit::Contain)
            .hexpand(true)
            .vexpand(true)
            .visible(false)
            .build();
        media_stack.add_fallback_media_surface(&sw_picture);

        let chrome = BigMediaChrome::new(
            BigMediaMenuSpec {
                icon_name: "open-menu-symbolic".to_owned(),
                label: i18n("Main Menu"),
                items: main_menu_items(),
            },
            !is_background,
        );
        // The OSD header floats over edge-to-edge video with the compositor's
        // native title buttons disabled, so the standalone window has to draw
        // its own minimize/maximize/close (the idle background pane has no chrome).
        if !is_background {
            chrome.show_end_window_controls(&i18n("Minimize"), &i18n("Maximize"), &i18n("Close"));
        }

        // Slim, non-blocking export-progress strip: a shared `BigProgressRow`
        // (title · bar · percent/speed/ETA · Cancel) on a flat status bar docked
        // DIRECTLY ABOVE the transport — so the user keeps watching the video and
        // follows the cut progress in place (NOT the converter's full-cover page).
        let trim_progress_row = BigProgressRow::new(
            &big_relm4_components::list::progress_row::BigProgressRowSpec::indeterminate(
                i18n("Generating cut…"),
                String::new(),
            )
            .with_action(
                big_relm4_components::list::progress_row::BigProgressRowActionSpec::new(
                    "process-stop-symbolic",
                    i18n("Cancel"),
                ),
            ),
        );
        if let Some(cancel) = trim_progress_row.action_button() {
            cancel.connect_clicked(glib::clone!(
                #[strong]
                sender,
                move |_| sender.input(VideoShellInput::TrimCancelExport)
            ));
        }
        let trim_progress_strip = gtk::Box::new(gtk::Orientation::Vertical, 0);
        trim_progress_strip.add_css_class("trim-progress");
        trim_progress_strip.append(trim_progress_row.root());
        let trim_progress_revealer = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .reveal_child(false)
            .child(&trim_progress_strip)
            .build();
        // One bottom stack: the progress strip sits above the transport so it
        // reads as an extension of the bottom controls, never covering the video.
        let bottom_stack = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bottom_stack.set_valign(gtk::Align::End);
        bottom_stack.append(&trim_progress_revealer);
        bottom_stack.append(controls.widget());
        // Re-encode options (container/codec/quality) live in the gear dialog,
        // not an over-screen panel; `trim_encode.group()` is parented there on
        // first open.
        chrome.set_bottom_child(&bottom_stack);
        if !is_background {
            media_stack.install_chrome(&chrome, BigMediaChromePlacement::Bottom);
        }

        // Welcome screen: shown until the first file loads (hidden on
        // `FileLoaded`). Mirrors the old `setup_welcome_screen` overlay.
        let welcome = if is_background {
            None
        } else {
            let welcome = build_welcome_screen(&sender);
            media_stack.add_welcome_overlay(&welcome);
            Some(welcome)
        };

        // Buffering spinner: centered overlay, hidden until `Buffering(true)`.
        // Mirrors the old `status_overlay`/`buffering_spinner`.
        let buffering_overlay = gtk::Box::builder()
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        buffering_overlay.append(
            &adw::Spinner::builder()
                .width_request(48)
                .height_request(48)
                .build(),
        );
        media_stack.add_status_overlay(&buffering_overlay);

        // Drag-and-drop a file onto the window → open it (old `setup_drag_drop`).
        {
            let sender = sender.clone();
            big_app_kit::widgets::add_file_drop_target(&main_overlay, move |paths| {
                let Some(path) = paths.first().and_then(|p| p.to_str()) else {
                    return false;
                };
                sender.input(VideoShellInput::OpenFiles(vec![path.to_string()]));
                true
            });
        }

        // Sidebar reveal: collapsed overlay (playlist) over the video content.
        // Hidden by default; a header toggle/`OpenFiles` reveals it. The shared
        // media shell owns the split-view sizing and leak-safe drag handle so
        // camera/image shells can reuse the same chrome without cloning this
        // widget plumbing.
        let sidebar_shell = BigMediaSidebarShell::new(
            playlist.widget(),
            &main_overlay,
            BigMediaSidebarSpec::new()
                .width_range(260.0, 480.0)
                .resize_limit(900.0),
        );
        let split_view = sidebar_shell.split_view().clone();

        // Keep-Above toggle. No portable Wayland keep-above
        // exists; mirror the god-object by toggling via the compositor (KWin
        // global shortcut, falling back to wmctrl). Both via argv arrays — no
        // shell, no untrusted input.
        let keep_above_toggle_button = gtk::ToggleButton::builder()
            .icon_name("view-pin-symbolic")
            .css_classes(["osd-btn"])
            .build();
        keep_above_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Keep Above"))]);
        keep_above_toggle_button.connect_toggled(|_button| toggle_keep_above());

        // Open popover button: a real GtkButton exposes AT-SPI DoAction while
        // the popover rows keep named, activatable file-opening actions.
        let open_label = i18n("Open");
        let open_popover_button = build_flat_action_popover_button(
            &BigHamburgerMenuSpec::new(open_label)
                .icon_name("document-open-symbolic")
                .css_classes(["osd-btn"])
                .items(open_menu_items()),
        );
        let open_menu_root = open_popover_button.root().clone();
        let open_menu_popover = open_popover_button.popover().clone();

        // Two separate sidebar entry points: Files (folder browser) and Playlist
        // (the play queue). Each reveals the sidebar in its mode; clicking the
        // active one hides it. Not a radio pair — both off = sidebar hidden.
        let files_sidebar_toggle_button = gtk::ToggleButton::builder()
            .icon_name("folder-symbolic")
            .css_classes(["osd-btn"])
            .build();
        files_sidebar_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Files"))]);
        let playlist_sidebar_toggle_button = gtk::ToggleButton::builder()
            .icon_name("view-list-symbolic")
            .css_classes(["osd-btn"])
            .build();
        playlist_sidebar_toggle_button
            .update_property(&[gtk::accessible::Property::Label(&i18n("Playlist"))]);

        // Guard so programmatic `set_active` (deactivating the sibling) doesn't
        // re-enter the toggled handlers.
        let toggles_updating = Rc::new(Cell::new(false));
        let active_sidebar_mode = Rc::new(Cell::new(SidebarMode::Files));
        let pl_sender = playlist.sender().clone();
        wire_sidebar_toggle(
            &files_sidebar_toggle_button,
            &playlist_sidebar_toggle_button,
            SidebarMode::Files,
            &split_view,
            &toggles_updating,
            &active_sidebar_mode,
            &pl_sender,
        );
        wire_sidebar_toggle(
            &playlist_sidebar_toggle_button,
            &files_sidebar_toggle_button,
            SidebarMode::Playlist,
            &split_view,
            &toggles_updating,
            &active_sidebar_mode,
            &pl_sender,
        );
        // Keep the toggles in sync when the overlay is dismissed externally
        // (click-outside / Esc collapses an AdwOverlaySplitView) → release both.
        split_view.connect_show_sidebar_notify({
            let files_sidebar_toggle_button = files_sidebar_toggle_button.downgrade();
            let playlist_sidebar_toggle_button = playlist_sidebar_toggle_button.downgrade();
            let updating = toggles_updating.clone();
            move |sv| {
                if updating.get() || sv.shows_sidebar() {
                    return;
                }
                updating.set(true);
                if let Some(button) = files_sidebar_toggle_button.upgrade() {
                    button.set_active(false);
                }
                if let Some(button) = playlist_sidebar_toggle_button.upgrade() {
                    button.set_active(false);
                }
                updating.set(false);
            }
        });

        // Shared media-viewer chrome (header + accessible primary menu + the
        // OSD reveal) — see big_media_components::surface. The video
        // player keeps mpv/offload/welcome/buffering state app-local; shared
        // chrome owns the repeatable header, bottom revealer, and sidebar split.
        chrome.add_header_start(&keep_above_toggle_button);
        chrome.add_header_start(&open_menu_root);
        chrome.add_header_start(&files_sidebar_toggle_button);
        chrome.add_header_start(&playlist_sidebar_toggle_button);

        let win_actions = install_window_actions(&root, &sender);

        // Header is an OSD overlay over edge-to-edge video (matches the old
        // design), NOT an AdwToolbarView top bar: wrapping the video surface in a
        // ToolbarView breaks GraphicsOffload subsurface presentation (the video
        // rendered but presented at ~1 fps). With the video filling the split's
        // content directly, the offload subsurface presents at full rate.
        //
        // Wrap the headerbar in a box carrying the custom `.osd-header` class
        // (NOT the libadwaita built-in `.osd` class). `.osd` forces light-on-dark
        // regardless of theme — in the light theme that left the header icons
        // (incl. the hamburger menu button) light-on-light / unreadable. The
        // app-owned `apply_appearance` CSS styles `.osd-header headerbar` with
        // theme-aware colors (the `.osd-btn` header buttons get
        // `@player_icon_color`), so both light and dark themes stay readable —
        // matching the old god-object's `.osd-header` wrapper box.
        if !is_background {
            media_stack.install_chrome(&chrome, BigMediaChromePlacement::Header);
        }

        // OSD auto-hide: reveal on pointer motion over the video, hide after
        // `OSD_HIDE_DELAY_MS` of inactivity or on pointer-leave. Mirrors the old
        // `controls/visibility.rs`. The welcome screen is up at init, so the OSD
        // never auto-hides until the first file loads.
        let trim_active = Rc::new(Cell::new(false));
        let osd = OsdState::new(
            &main_overlay,
            chrome.header_revealer(),
            chrome.bottom_revealer(),
            vec![chrome.menu_popover().clone(), open_menu_popover],
            controls.model().popovers(),
            !is_background,
            trim_active.clone(),
        );
        if is_background {
            wire_background_video_input(&main_overlay);
        } else {
            wire_osd_autohide(&main_overlay, &osd);
            // Collapse the OSD when the window loses focus (alt-tab to another
            // app), like other video players — not just on the idle timer.
            wire_osd_focus_out(&main_overlay, &osd);
            // Pointer gestures over the video (mirrors the old `input/mod.rs`):
            // double-click → fullscreen, right-click → context menu, drag → move
            // the window (when not fullscreen).
            wire_video_input(&root, &main_overlay, &sender);
            // Window-level keyboard shortcuts (space/seek/volume/fullscreen/…).
            // The Relm4 port had lost this dispatch — the shortcut table was
            // display-only until this controller restored it.
            super::keyboard::wire_keyboard_shortcuts(&root, &mpv, &settings, &sender);
            // Trim-mode keys (I / O / M / Esc): a capture-phase controller that
            // only acts while trim mode is active, so it never shadows the normal
            // bindings on those keys outside trim mode.
            super::keyboard::wire_trim_keys(&root, trim_active.clone(), &sender);
        }

        // Static idle background behind the welcome screen (gl surface hidden so
        // it shows through). Shown below once the display is known; restored on
        // the first `FileLoaded`. The blurred desktop wallpaper is fetched off
        // the main loop (idle_wallpaper::spawn_apply) and swapped in when ready.
        let idle_bg = Rc::new(IdleBackground {
            video_bg: video_bg.clone(),
            gl_area: gl_area.clone(),
            provider: RefCell::new(None),
            blurred_path: RefCell::new(None),
            is_shown: Cell::new(false),
        });

        // Toast host wrapping the content (screenshot / chapter / sleep-timer
        // feedback). The split view is its child; toasts float above the video.
        // The Root IS the toast overlay (toast host wrapping the content): the
        // split view is its child; toasts float above the video.
        let toast_overlay = root.clone();
        mount_media_toast_content(&toast_overlay, &split_view);

        // Show the idle background now (welcome is up). Hidden on first
        // `FileLoaded` so the video surface becomes visible again. Kick off the
        // wallpaper detection + blur so the static gradient is replaced by the
        // blurred desktop wallpaper once it is cached.
        if !is_background {
            if let Some(display) = gtk::gdk::Display::default() {
                idle_bg.show(&display);
            }
            super::idle_wallpaper::spawn_apply(idle_bg.clone());
        }

        // mpv option strings, read once.
        let mut custom_mpv_options = settings.get_string(keys::CUSTOM_MPV_OPTIONS).clone();
        if settings.get_bool(keys::GPU_DUMB_MODE) {
            if !custom_mpv_options.is_empty() {
                custom_mpv_options.push(',');
            }
            custom_mpv_options.push_str("gpu-dumb-mode=yes");
        }
        let audio_output = settings.get_string(keys::AUDIO_OUTPUT).clone();
        let initial_volume = settings.get_double(keys::VOLUME) * 100.0;
        let muted = settings.get_bool(keys::MUTE);

        let fast_render_context_slot: FastRenderContextSlot = Rc::new(RefCell::new(None));
        let video_backend: Rc<RefCell<Option<VideoBackend>>> = Rc::new(RefCell::new(None));

        // mpv/GL comes up when the CONTENT realizes (it needs only the display +
        // overlay/gl_area, not a window — ADR D14), so it works in any mount point.
        wire_realize(
            &root,
            &main_overlay,
            &gl_area,
            &sw_picture,
            &mpv,
            &fast_render_context_slot,
            &video_backend,
            &sender,
            VideoRealizeOptions {
                custom_mpv_options,
                audio_output,
                initial_volume,
                muted,
                render_mode: settings.get_string(keys::RENDER_MODE).clone(),
            },
        );

        // Publish the shell input so the application `open` handler can deliver
        // files opened from the desktop / CLI.
        let shell_input_token = set_shell_input(sender.input_sender().clone());
        root.connect_unrealize(move |_| clear_shell_input(shell_input_token));

        // Apply the saved OSD palette + layout CSS now (the god-object did this
        // on boot) into the shared display-wide provider slot; the Appearance
        // dialog re-applies live through the same slot.
        reapply_theme(&settings);

        // Window aspect-ratio snap: resize the window to the video's aspect on
        // load and on manual resize (debounced), so no letterbox bars remain.
        // Resolves the window lazily from the content (ADR D14).
        let aspect_snap = Rc::new(AspectSnap {
            content: root.downgrade(),
            settings: settings.clone(),
            mpv: mpv.clone(),
            gl_area: gl_area.clone(),
            split_view: split_view.clone(),
            aspect: Cell::new(0.0),
            resizing: Cell::new(false),
            resize_gen: Cell::new(0),
        });

        // Window-needing wiring (close → mpv teardown, the default-size notify
        // that drives the aspect snap) is deferred to first realize, when the
        // content is actually in a window (it has none at init — the mount point
        // supplies it). Runs once.
        {
            let mpv = mpv.clone();
            let settings = settings.clone();
            let snap = aspect_snap.clone();
            let done = Cell::new(false);
            root.connect_realize(move |root| {
                if done.replace(true) {
                    return;
                }
                let Some(window) = root.root().and_downcast::<adw::ApplicationWindow>() else {
                    return;
                };
                wire_close(&window, &mpv, &settings);
                {
                    let snap = snap.clone();
                    window.connect_default_width_notify(move |_| snap.schedule());
                }
                {
                    let snap = snap.clone();
                    window.connect_default_height_notify(move |_| snap.schedule());
                }
            });
        }

        ComponentParts {
            model: Self {
                mode,
                controls,
                playlist,
                split_view: split_view.clone(),
                sidebar_mode: active_sidebar_mode,
                files_sidebar_button: files_sidebar_toggle_button,
                playlist_sidebar_button: playlist_sidebar_toggle_button,
                sidebar_toggle_updating: toggles_updating,
                iptv,
                #[cfg(feature = "chromecast")]
                cast,
                main_overlay,
                welcome_overlay: RefCell::new(welcome),
                playlist_paths: Vec::new(),
                mpv,
                duration_secs: 0.0,
                settings,
                video_transform,
                osd,
                idle_bg,
                buffering_overlay,
                toast_overlay,
                win_actions,
                sleep_timer_source: None,
                activity_paused_playback: Cell::new(false),
                sleep_timer_remaining: 0,
                sleep_timer_action: String::new(),
                sleep_timer_volume: 100.0,
                aspect_snap,
                shell_input_token,
                trim,
                trim_progress_row,
                trim_encode,
                trim_filters: Rc::new(RefCell::new(
                    big_media_runtime::converter::profiles::VideoFilters::default(),
                )),
                trim_split: Rc::new(Cell::new(false)),
                trim_play_selection: Rc::new(Cell::new(false)),
                trim_ranges: Rc::new(RefCell::new(Vec::new())),
                trim_encode_dialog: Rc::new(RefCell::new(None)),
                trim_progress_revealer,
                trim_active,
                trim_queue: Rc::new(RefCell::new(None)),
            },
            widgets: (),
        }
    }

    fn update(
        &mut self,
        shell_input: Self::Input,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        // Panic boundary (module contract): a panic in message handling reaps
        // this module's window cleanly (host closes it) instead of orphaning a
        // dead window; the host process and sibling modules survive.
        big_app_kit::containment::contain_embedded_update(root, "video-player", || {
            let adw_window = root.root().and_downcast::<adw::ApplicationWindow>();
            let window_ref = adw_window.as_ref();
            let gtk_window = root.root().and_downcast::<gtk::Window>();
            let gtk_window_ref = gtk_window.as_ref();
            match shell_input {
                VideoShellInput::Player(event) => self.on_player_event(event, window_ref),
                VideoShellInput::Controls(out) => self.on_controls(out, window_ref),
                VideoShellInput::Playlist(out) => self.on_playlist(out),
                VideoShellInput::EnterTrimMode => self.enter_trim_mode(),
                VideoShellInput::ExitTrimMode => self.exit_trim_mode(),
                VideoShellInput::TrimBar(out) => self.on_trim_bar(out, window_ref, &sender),
                VideoShellInput::TrimCancelExport => self.cancel_trim_export(),
                VideoShellInput::TrimMark => self.trim_mark(),
                VideoShellInput::TrimExportFinished(outcome) => self.finish_trim_export(outcome),
                VideoShellInput::ExtractAudio(codec) => self.extract_audio(&codec),
                VideoShellInput::QuickConvert(container) => self.quick_convert(&container),
                VideoShellInput::Iptv(out) => VideoShell::on_iptv(out, &sender),
                #[cfg(feature = "chromecast")]
                VideoShellInput::Cast(out) => self.on_cast(out),
                VideoShellInput::ShowIptvBrowser => {
                    if let Some(root) = window_ref {
                        self.show_iptv_browser(root);
                    }
                }
                #[cfg(feature = "chromecast")]
                VideoShellInput::ShowCastDialog => {
                    if let Some(root) = window_ref {
                        self.show_cast_dialog(root);
                    }
                }
                VideoShellInput::ShowFiltersDialog => {
                    if let Some(root) = window_ref {
                        self.show_filters_dialog(root);
                    }
                }
                VideoShellInput::ShowEqualizer => {
                    if let Some(root) = window_ref {
                        self.show_equalizer_dialog(root);
                    }
                }
                VideoShellInput::ShowDelay => {
                    if let Some(root) = window_ref {
                        self.show_delay_dialog(root);
                    }
                }
                VideoShellInput::ShowImageAdjust => {
                    if let Some(root) = window_ref {
                        self.show_image_adjust_dialog(root);
                    }
                }
                VideoShellInput::ShowYtdlQuality => {
                    if let Some(root) = window_ref {
                        self.show_ytdl_quality_dialog(root);
                    }
                }
                VideoShellInput::ShowMediaInfo => {
                    if let Some(root) = window_ref {
                        self.show_media_details(root);
                    }
                }
                VideoShellInput::ShowKeyboardShortcuts => {
                    if let Some(root) = window_ref {
                        show_video_keyboard_shortcuts_dialog(root, &self.settings);
                    }
                }
                VideoShellInput::ShowOpenFile => {
                    if let Some(root) = gtk_window_ref {
                        Self::show_open_file_dialog(root);
                    }
                }
                VideoShellInput::ShowOpenUrl => {
                    if let Some(root) = window_ref {
                        Self::show_open_url_dialog(root);
                    }
                }
                VideoShellInput::ShowRecent => {
                    if let Some(root) = window_ref {
                        Self::show_recent_dialog(root);
                    }
                }
                VideoShellInput::ShowOpenDisc => {
                    if let Some(root) = window_ref {
                        Self::show_open_disc_dialog(root);
                    }
                }
                VideoShellInput::ToggleFilesSidebar => {
                    self.toggle_sidebar_mode(SidebarMode::Files);
                }
                VideoShellInput::TogglePlaylistSidebar => {
                    self.toggle_sidebar_mode(SidebarMode::Playlist);
                }
                VideoShellInput::ShowPreferences => {
                    if let Some(root) = window_ref {
                        self.show_preferences(root);
                    }
                }
                VideoShellInput::ShowAppearance => {
                    if let Some(root) = window_ref {
                        self.show_appearance(root);
                    }
                }
                VideoShellInput::ShowRestoreDefaults => {
                    if let Some(root) = window_ref {
                        Self::show_restore_defaults_dialog(root);
                    }
                }
                VideoShellInput::RestoreDefaultsConfirmed => self.restore_all_defaults(),
                VideoShellInput::OpenFiles(paths) => VideoShell::open_files(&paths, &sender),
                VideoShellInput::PlaylistResolved {
                    entries,
                    start,
                    folder,
                } => self.apply_open(entries, start, folder),
                VideoShellInput::ShowGotoTime => {
                    if let Some(root) = window_ref {
                        self.show_goto_time_dialog(root);
                    }
                }
                VideoShellInput::Screenshot => self.take_screenshot(),
                VideoShellInput::ShowChapters => {
                    if let Some(root) = window_ref {
                        self.show_chapters_dialog(root, &sender);
                    }
                }
                VideoShellInput::SetChapter(idx) => self.set_chapter(idx),
                VideoShellInput::ShowSleepTimer => {
                    if let Some(root) = window_ref {
                        self.show_sleep_timer_dialog(root, &sender);
                    }
                }
                VideoShellInput::SleepTimerStart { minutes, action } => {
                    self.start_sleep_timer(sleep_minutes_to_secs(minutes), action, &sender);
                }
                VideoShellInput::SleepTimerCancel => {
                    self.cancel_sleep_timer();
                    self.notify(&i18n("Sleep timer cancelled"));
                }
                VideoShellInput::SleepTick => self.sleep_tick(),
                VideoShellInput::TogglePlayback => {
                    if let Some(mpv) = self.mpv.borrow().as_ref() {
                        mpv.toggle_pause();
                    }
                }
                VideoShellInput::ToggleFullscreen => {
                    if let Some(root) = window_ref {
                        let _ = toggle_surface_fullscreened(root);
                    }
                }
                VideoShellInput::SetPaneActivity(activity) => self.set_pane_activity(activity),
            }
        });
    }
}

impl Drop for VideoShell {
    /// Remove the armed sleep-timer countdown source so it does not outlive the
    /// shell (no leaked `glib::timeout`) and clear the global input broker.
    ///
    /// Do not force-drop the live mpv backend here: GTK widgets still own
    /// render-context callbacks during unrealize/finalize, and those callbacks
    /// must release their mpv render context before the shared mpv handle is
    /// destroyed.
    fn drop(&mut self) {
        clear_shell_input(self.shell_input_token);
        // Abort any in-flight clip export so a closing window does not leave an
        // orphaned ffmpeg worker running.
        self.cancel_trim_export();
        if let Some(source) = self.sleep_timer_source.take() {
            source.remove();
        }
    }
}

#[cfg(all(test, not(miri)))]
fn build_open_menu_model() -> gio::Menu {
    build_flat_menu_model(&open_menu_items())
}

fn open_menu_items() -> Vec<BigMenuActionItem> {
    vec![
        BigMenuActionItem::new(i18n("Open File…"), "win.open-file"),
        BigMenuActionItem::new(i18n("Open Recent"), "win.open-recent"),
        BigMenuActionItem::new(i18n("Open URL…"), "win.open-url"),
        BigMenuActionItem::new(i18n("Open Disc…"), "win.open-disc"),
    ]
}

#[cfg(test)]
fn open_menu_action_names() -> Vec<String> {
    open_menu_items()
        .into_iter()
        .map(|menu_item| menu_item.action)
        .collect()
}

#[cfg(all(test, not(miri)))]
fn build_main_menu_model() -> gio::Menu {
    build_flat_menu_model(&main_menu_items())
}

fn main_menu_items() -> Vec<BigMenuActionItem> {
    // "Go to Time" moved to the elapsed-time label click; "Screenshot" is on
    // the video right-click menu — both removed from here.
    let mut items = vec![
        BigMenuActionItem::new(i18n("Chapters"), "win.chapters"),
        BigMenuActionItem::new(i18n("Sleep Timer…"), "win.sleep-timer"),
        BigMenuActionItem::new(i18n("Equalizer"), "win.show-equalizer"),
        BigMenuActionItem::new(i18n("Audio & Subtitle Delay"), "win.show-delay"),
    ];
    // Renamed for clarity: "Image & Video" was really the colour/picture
    // adjustments; "Online Video Quality" is the stream-quality picker.
    items.extend([
        BigMenuActionItem::new(i18n("Picture Adjustments"), "win.show-image-adjust"),
        BigMenuActionItem::new(i18n("Video Filters"), "win.filters"),
        BigMenuActionItem::new(i18n("Stream Quality"), "win.ytdl-quality"),
        BigMenuActionItem::new(i18n("IPTV"), "win.iptv"),
    ]);
    #[cfg(feature = "chromecast")]
    items.push(BigMenuActionItem::new(i18n("Cast to Device"), "win.cast"));

    // Edit & Export (cut / extract audio / convert) moved to a footer button
    // next to the subtitle control — see video_controls_bar::zones.

    // "Restore Defaults" moved into the Preferences window (header button), so
    // it is not loose in the top menu.
    items.extend([
        BigMenuActionItem::new(i18n("Media Info"), "win.media-info"),
        BigMenuActionItem::new(i18n("Preferences"), "win.preferences"),
        BigMenuActionItem::new(i18n("Appearance"), "win.appearance"),
        BigMenuActionItem::new(i18n("Keyboard Shortcuts"), "win.keyboard-shortcuts"),
        BigMenuActionItem::new(i18n("About"), "app.about"),
    ]);
    items
}

#[cfg(test)]
fn main_menu_action_names() -> Vec<String> {
    main_menu_items()
        .into_iter()
        .map(|menu_item| menu_item.action)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_menu_uses_flat_action_items() {
        assert_menu_actions_include(
            &open_menu_action_names(),
            &[
                "win.open-file",
                "win.open-recent",
                "win.open-url",
                "win.open-disc",
            ],
        );
    }

    #[test]
    fn main_menu_uses_native_gmenu_actions() {
        let expected = &[
            "win.chapters",
            "win.sleep-timer",
            "win.show-equalizer",
            "win.show-delay",
            "win.show-image-adjust",
            "win.filters",
            "win.ytdl-quality",
            "win.iptv",
            "win.media-info",
            "win.preferences",
            "win.appearance",
            "win.keyboard-shortcuts",
            "app.about",
        ];
        assert_menu_actions_include(&main_menu_action_names(), expected);

        #[cfg(feature = "chromecast")]
        assert_menu_actions_include(&main_menu_action_names(), &["win.cast"]);
    }

    #[cfg(not(miri))]
    #[test]
    fn main_menu_is_flat_without_sections_or_submenus() {
        let menu = build_main_menu_model();
        for index in 0..menu.n_items() {
            assert!(
                menu.item_link(index, "section").is_none(),
                "hamburger menu must not expose categories"
            );
            assert!(
                menu.item_link(index, "submenu").is_none(),
                "hamburger menu must not expose category submenus"
            );
        }
    }

    #[cfg(not(miri))]
    #[test]
    fn open_menu_is_flat_without_sections_or_submenus() {
        let menu = build_open_menu_model();
        for index in 0..menu.n_items() {
            assert!(menu.item_link(index, "section").is_none());
            assert!(menu.item_link(index, "submenu").is_none());
        }
    }

    fn assert_menu_actions_include(actions: &[String], expected: &[&str]) {
        for action in expected {
            assert!(
                actions.iter().any(|actual| actual == action),
                "missing menu action {action}; actions={actions:?}"
            );
        }
    }
}
