//! Relm4 root component for the converter — the de-god-objected `Window`.
//!
//! `ConverterShell` owns the `adw::ApplicationWindow`, the runtime
//! [`ConversionQueue`]/settings/profiles/GPUs, and the six child Controllers
//! (header, sidebar, queue, progress, video/audio editor). Every child `Output`
//! is forwarded to [`ShellMessage`]; the shell holds no `Rc<RefCell<callback>>` and
//! no command sink. The 200 ms progress tick lives here and is torn down on drop.
//!
//! Built alongside the legacy `audio_converter::ConverterModel` until the F7
//! cutover switches `lib.rs::run` over to this component.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use gtk::glib::clone;
use log::info;
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller, main_application,
};

use big_media_components::converter::header_bar::{
    BigConverterHeader, BigConverterHeaderOutput, BigConverterHeaderText,
};
use big_media_components::converter::progress::{
    BigConverterProgress, BigConverterProgressInput, BigConverterProgressOutput,
};
use big_media_components::converter::queue::BigConversionQueueMode;
use big_media_components::converter::queue_component::{
    BigConverterQueue, BigConverterQueueInput, BigConverterQueueOutput,
};
use big_os_kit::microdaemon::DurableJobSnapshot;
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};
use big_relm4_components::layout::widget_container::clear_box_children;

use crate::cast::gpu::{GpuDevice, detect_all_gpus};
use crate::converter::app::ConverterMode;
use crate::converter::daemon::{DaemonJobAccepted, DaemonSubmitError};
use crate::converter::profiles::{self, Profile};
use crate::converter::queue::ConversionQueue;
use crate::converter::settings::{self as conv_settings, keys};
use crate::converter::ui::sidebar::{
    ConversionParams, ConverterSidebar, ConverterSidebarInit, ConverterSidebarInput,
    ConverterSidebarOutput,
};
use crate::i18n::i18n;
use crate::settings::Settings;
use big_media_runtime::editor::audio_edit_page::{
    AudioEditor, AudioEditorInit, AudioEditorInput, AudioEditorOutput,
};
use big_media_runtime::editor::edit_page::{
    VideoEditResult, VideoEditor, VideoEditorInput, VideoEditorOutput,
};

mod dialogs;
mod handlers;
mod setup;

thread_local! {
    /// Main-thread handles to the running shells' inputs, keyed by binding
    /// and set at init so the
    /// [`Application`](crate::converter::app::Application) `open` handler can
    /// deliver opened files. `ShellMessage` carries GTK objects (not `Send`), so
    /// thread-local `Sender`s are used instead of a `MessageBroker` static.
    /// One slot per [`ConverterMode`]: the audio and video shells can run
    /// side by side in one host process.
    static SHELL_INPUTS: std::cell::RefCell<Vec<(ConverterMode, relm4::Sender<ShellMessage>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Deliver a message to the running shell of the given binding from the main
/// thread (used by the application `open` handler). No-op if that shell is
/// not running yet.
pub fn send_to_shell(mode: ConverterMode, message: ShellMessage) {
    SHELL_INPUTS.with(|cell| {
        if let Some((_, sender)) = cell.borrow().iter().find(|(m, _)| *m == mode) {
            let _ = sender.send(message);
        }
    });
}

/// Every user-driven event in the converter, routed through one Relm4 model.
#[derive(Debug)]
pub enum ShellMessage {
    // ── files ────────────────────────────────────────────────────────
    /// Enqueue the given source files.
    AddFiles(Vec<PathBuf>),
    /// Open the "Add Files" picker.
    OpenFileChooser,
    /// Open the "Add Folder" picker.
    OpenFolderChooser,
    /// Open the network-URL dialog.
    OpenNetworkDialog,
    // ── conversion ───────────────────────────────────────────────────
    /// Begin converting every pending item.
    StartConversion,
    /// `big-convertd` accepted or rejected the current batch.
    DaemonSubmitFinished(Result<DaemonJobAccepted, DaemonSubmitError>),
    /// Latest `big-convertd` snapshot for the tracked batch.
    DaemonSnapshot(Result<DurableJobSnapshot, DaemonSubmitError>),
    /// `big-convertd` accepted or rejected a cancel request.
    DaemonCancelFinished(Result<(), DaemonSubmitError>),
    /// Cancel the in-flight conversion.
    CancelConversion,
    /// Header "Clear queue" — show the confirm dialog.
    ClearQueueRequested,
    /// Clear confirmed — empty the queue.
    ClearQueueConfirmed,
    /// 200 ms progress tick.
    Tick,
    /// Restore-defaults requested (header menu) — show confirm.
    RestoreDefaultsRequested,
    // ── sidebar ──────────────────────────────────────────────────────
    /// Cached conversion parameters changed.
    ParamsChanged(Box<ConversionParams>),
    /// Output-dir picker requested.
    ShowOutputDirChooser,
    /// Live audio preview parameters changed.
    AudioPreview {
        /// Effect snapshot.
        effects: crate::player::mpv_backend::AudioEffectsSnapshot,
        /// Volume factor.
        volume: f64,
        /// Speed factor.
        speed: f64,
    },
    /// Cut/trim mode toggled.
    CutModeActive(bool),
    /// Bind the shared volume/speed adjustments into the audio editor.
    BindAudioAdjustments {
        /// Volume adjustment.
        volume: gtk::Adjustment,
        /// Speed adjustment.
        speed: gtk::Adjustment,
    },
    /// Apply the default profile (extra-dialog reset).
    RestoreDefaultProfile,
    /// Mount the sidebar's output-destination footer into the content area.
    MountFooter(gtk::Box),
    // ── queue rows ───────────────────────────────────────────────────
    /// Remove row (after confirm).
    QueueRemoveRequested(u64),
    /// Remove confirmed.
    QueueRemoveConfirmed(u64),
    /// Open the containing folder.
    QueueOpenFolder(PathBuf),
    /// Show file metadata.
    QueueMetadata(u64),
    /// Delete file from disk (after confirm).
    QueueDeleteDisk(u64),
    /// Play / preview a row.
    QueuePlay(PathBuf),
    /// Row selected.
    QueueSelect(PathBuf),
    /// Edit a row (video editor).
    QueueEdit(PathBuf),
    /// Rows reordered.
    QueueReorder(Vec<u64>),
    // ── progress ─────────────────────────────────────────────────────
    /// Back to the queue from the progress page.
    ProgressBack,
    // ── video editor ─────────────────────────────────────────────────
    /// Apply edits + convert from the video editor.
    VideoApplyAndConvert(Box<VideoEditResult>),
    /// Fullscreen toggled in the video editor.
    VideoFullscreen(bool),
    /// Back from the video editor.
    VideoBack,
    // ── audio editor ─────────────────────────────────────────────────
    /// Audio segments edited (live).
    AudioSegmentsApplied(Vec<crate::converter::ffmpeg::TrimRange>),
    /// Apply + convert from the audio editor.
    AudioApplyAndConvert(Vec<crate::converter::ffmpeg::TrimRange>),
    /// Audio playback state changed.
    AudioPlaybackState(bool),
    /// Previous track in the audio preview.
    AudioPrevTrack,
    /// Next track in the audio preview.
    AudioNextTrack,
    /// Toggle audio play/pause (space accelerator).
    AudioTogglePlayPause,
    // ── dialogs forwarded to the sidebar (own its widgets) ───────────
    /// Show the noise / audio-settings dialog.
    ShowNoiseDialog,
    /// Show the equalizer dialog.
    ShowEqualizerDialog,
    // ── window ───────────────────────────────────────────────────────
    /// The window close was requested.
    CloseRequested,
    /// Close confirmed while a conversion was running (cancel queue + close).
    ConfirmedClose,
}

/// Relm4 root component: the converter window + its child component tree.
pub struct ConverterShell {
    mode: ConverterMode,
    header: Controller<BigConverterHeader>,
    sidebar: Controller<ConverterSidebar>,
    queue_view: Controller<BigConverterQueue>,
    progress: Controller<BigConverterProgress>,
    video_editor: Option<Controller<VideoEditor>>,
    audio_editor: Option<Controller<AudioEditor>>,

    window: adw::ApplicationWindow,
    main_stack: gtk::Stack,
    audio_preview_revealer: gtk::Revealer,
    footer_slot: gtk::Box,

    queue: ConversionQueue,
    settings: Settings,
    profiles: BTreeMap<String, Profile>,
    gpus: Vec<GpuDevice>,
    gpu_ids: Vec<String>,
    params: Option<ConversionParams>,

    converting: bool,
    daemon_job: Option<DaemonJobAccepted>,
    daemon_poll_in_flight: bool,
    editing_item_id: Option<u64>,
    current_audio_preview_item: Option<u64>,
    // Conversion progress counters for the live window title (the wrapped
    // progress component doesn't expose its counts).
    progress_total: usize,
    progress_done: usize,
    // Shared with the close-request handler so it can return Proceed once a
    // close is actually committed (window.close() re-emits close-request).
    is_closing: Rc<Cell<bool>>,
    tick_source: Option<glib::SourceId>,
}

impl Drop for ConverterShell {
    fn drop(&mut self) {
        if let Some(id) = self.tick_source.take() {
            id.remove();
        }
    }
}

impl Component for ConverterShell {
    type Init = ConverterMode;
    type Input = ShellMessage;
    type Output = ();
    type CommandOutput = ();
    type Root = adw::ApplicationWindow;
    type Widgets = ();

    fn init_root() -> Self::Root {
        // Window host is whatever application owns the process: the converter's
        // own `ConverterApplication` in standalone mode, or an external host
        // application (big-host) in embedded mode — no concrete type required.
        adw::ApplicationWindow::new(&main_application())
    }

    #[allow(clippy::too_many_lines)] // window assembly + child wiring — long by nature
    fn init(
        mode: Self::Init,
        window: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let settings = conv_settings::converter_settings(mode);

        SHELL_INPUTS.with(|cell| {
            let mut slots = cell.borrow_mut();
            slots.retain(|(m, _)| *m != mode);
            slots.push((mode, sender.input_sender().clone()));
        });

        // Geometry restore.
        let w = settings.get_int(keys::WINDOW_WIDTH);
        let h = settings.get_int(keys::WINDOW_HEIGHT);
        window.set_default_size(if w > 0 { w } else { 1200 }, if h > 0 { h } else { 720 });
        window.set_size_request(920, 600);
        if settings.get_bool(keys::WINDOW_MAXIMIZED) {
            window.maximize();
        }
        window.set_title(Some(&i18n(if mode.is_audio() {
            "Audio Converter"
        } else {
            "Video Converter"
        })));

        // ── child Controllers (Output → ShellMessage) ───────────────────────
        let header = BigConverterHeader::builder()
            .launch(BigConverterHeaderText::translated(i18n))
            .forward(sender.input_sender(), |out| match out {
                BigConverterHeaderOutput::Convert => ShellMessage::StartConversion,
                BigConverterHeaderOutput::ClearQueue => ShellMessage::ClearQueueRequested,
                BigConverterHeaderOutput::Cancel => ShellMessage::CancelConversion,
                BigConverterHeaderOutput::RestoreDefaults => ShellMessage::RestoreDefaultsRequested,
                BigConverterHeaderOutput::AddFiles => ShellMessage::OpenFileChooser,
                BigConverterHeaderOutput::AddFolder => ShellMessage::OpenFolderChooser,
                BigConverterHeaderOutput::AddNetwork => ShellMessage::OpenNetworkDialog,
            });

        let sidebar = ConverterSidebar::builder()
            .launch(ConverterSidebarInit {
                settings: settings.clone(),
                mode,
            })
            .forward(sender.input_sender(), |out| match out {
                ConverterSidebarOutput::ParamsChanged(p) => ShellMessage::ParamsChanged(p),
                ConverterSidebarOutput::OutputDirClicked => ShellMessage::ShowOutputDirChooser,
                ConverterSidebarOutput::AudioPreviewChanged {
                    effects,
                    volume,
                    speed,
                } => ShellMessage::AudioPreview {
                    effects,
                    volume,
                    speed,
                },
                ConverterSidebarOutput::CutModeActive(active) => {
                    ShellMessage::CutModeActive(active)
                }
                ConverterSidebarOutput::AudioAdjustmentsReady { volume, speed } => {
                    ShellMessage::BindAudioAdjustments { volume, speed }
                }
                ConverterSidebarOutput::RestoreDefaultRequested => {
                    ShellMessage::RestoreDefaultProfile
                }
                ConverterSidebarOutput::FooterReady(footer) => ShellMessage::MountFooter(footer),
            });

        let queue_mode = if mode.is_audio() {
            BigConversionQueueMode::Audio
        } else {
            BigConversionQueueMode::Video
        };
        let queue_view =
            BigConverterQueue::builder()
                .launch(queue_mode)
                .forward(sender.input_sender(), |out| match out {
                    BigConverterQueueOutput::Remove(id) => ShellMessage::QueueRemoveRequested(id),
                    BigConverterQueueOutput::OpenFolder(p) => ShellMessage::QueueOpenFolder(p),
                    BigConverterQueueOutput::Metadata(id) => ShellMessage::QueueMetadata(id),
                    BigConverterQueueOutput::DeleteDisk(id) => ShellMessage::QueueDeleteDisk(id),
                    BigConverterQueueOutput::Play(p) => ShellMessage::QueuePlay(p),
                    BigConverterQueueOutput::Select(p) => ShellMessage::QueueSelect(p),
                    BigConverterQueueOutput::Edit(p) => ShellMessage::QueueEdit(p),
                    BigConverterQueueOutput::Reorder(o) => ShellMessage::QueueReorder(o),
                });

        let progress =
            BigConverterProgress::builder()
                .launch(())
                .forward(sender.input_sender(), |out| match out {
                    BigConverterProgressOutput::Back => ShellMessage::ProgressBack,
                    BigConverterProgressOutput::CancelAll
                    | BigConverterProgressOutput::CancelConversion(_) => {
                        ShellMessage::CancelConversion
                    }
                });

        let (video_editor, audio_editor) = if mode.is_audio() {
            let ae = AudioEditor::builder()
                .launch(AudioEditorInit { compact: true })
                .forward(sender.input_sender(), |out| match out {
                    AudioEditorOutput::SegmentsApplied(r) => ShellMessage::AudioSegmentsApplied(r),
                    AudioEditorOutput::ApplyAndConvert(r) => ShellMessage::AudioApplyAndConvert(r),
                    AudioEditorOutput::ShowAudioSettings => ShellMessage::ShowNoiseDialog,
                    AudioEditorOutput::ShowEqualizer => ShellMessage::ShowEqualizerDialog,
                    AudioEditorOutput::PlaybackStateChanged(p) => {
                        ShellMessage::AudioPlaybackState(p)
                    }
                    AudioEditorOutput::PreviousTrack => ShellMessage::AudioPrevTrack,
                    AudioEditorOutput::NextTrack => ShellMessage::AudioNextTrack,
                });
            (None, Some(ae))
        } else {
            let ve = VideoEditor::builder()
                .launch(())
                .forward(sender.input_sender(), |out| match out {
                    VideoEditorOutput::ApplyAndConvert(r) => {
                        ShellMessage::VideoApplyAndConvert(Box::new(r))
                    }
                    VideoEditorOutput::ShowAudioSettings => ShellMessage::ShowNoiseDialog,
                    VideoEditorOutput::Fullscreen(fs) => ShellMessage::VideoFullscreen(fs),
                    VideoEditorOutput::Back => ShellMessage::VideoBack,
                    VideoEditorOutput::SegmentsChanged(_) => ShellMessage::Tick,
                });
            (Some(ve), None)
        };

        // ── window tree (ported from window/build.rs) ───────────────────
        let main_stack = gtk::Stack::new();
        let audio_preview_revealer = gtk::Revealer::new();
        let footer_slot = gtk::Box::new(gtk::Orientation::Vertical, 0);

        let sidebar_nav = adw::NavigationPage::builder()
            .title(i18n("Settings"))
            .child(sidebar.widget())
            .build();

        let content_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content_box.append(header.widget());
        let content_stack = adw::ViewStack::new();
        content_stack.add_titled_with_icon(
            queue_view.widget(),
            Some("queue"),
            &i18n("Files"),
            "document-open-symbolic",
        );
        content_box.append(&content_stack);
        content_box.append(&footer_slot);

        let content_nav = adw::NavigationPage::builder()
            .title(i18n(if mode.is_audio() {
                "Audio Converter"
            } else {
                "Video Converter"
            }))
            .child(&content_box)
            .build();

        let split_view = adw::OverlaySplitView::new();
        split_view.set_sidebar(Some(&sidebar_nav));
        split_view.set_content(Some(&content_nav));
        split_view.set_collapsed(false);
        split_view.set_sidebar_position(gtk::PackType::Start);

        let main_layout: gtk::Widget = if let Some(ae) = &audio_editor {
            audio_preview_revealer.set_transition_type(gtk::RevealerTransitionType::SlideUp);
            audio_preview_revealer.set_transition_duration(180);
            audio_preview_revealer.set_reveal_child(false);

            let preview_frame = gtk::Frame::new(None);
            preview_frame.set_hexpand(true);
            preview_frame.set_vexpand(false);
            preview_frame.set_child(Some(ae.widget()));
            audio_preview_revealer.set_child(Some(&preview_frame));

            let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
            split_view.set_vexpand(true);
            outer.append(&split_view);
            outer.append(&audio_preview_revealer);
            outer.upcast::<gtk::Widget>()
        } else {
            split_view.clone().upcast::<gtk::Widget>()
        };

        let progress_title = gtk::Label::new(Some(&i18n(if mode.is_audio() {
            "Converting Audio"
        } else {
            "Converting Videos"
        })));
        let progress_pane = BigToolbarPane::new(BigToolbarPaneSpec::new());
        let progress_header = BigToolbarHeader::with_title_widget(
            BigToolbarHeaderSpec::new()
                .show_start_title_buttons(false)
                .show_end_title_buttons(true)
                .css_class("flat"),
            &progress_title,
        );
        progress_pane.add_top_bar(progress_header.root());
        progress_pane.set_content(progress.widget());
        let progress_toolbar = progress_pane.into_root();

        main_stack.add_named(&main_layout, Some("main"));
        main_stack.add_named(&progress_toolbar, Some("progress"));
        if let Some(ve) = &video_editor {
            main_stack.add_named(ve.widget(), Some("edit"));
        }
        main_stack.set_visible_child_name("main");
        main_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        main_stack.set_transition_duration(200);

        let nav_view = adw::NavigationView::new();
        let wrapper = adw::NavigationPage::builder()
            .title(i18n(if mode.is_audio() {
                "Audio Converter"
            } else {
                "Video Converter"
            }))
            .child(&main_stack)
            .build();
        nav_view.push(&wrapper);

        let toolbar = BigToolbarPane::new(BigToolbarPaneSpec::new());
        toolbar.set_content(&nav_view);
        window.set_content(Some(toolbar.root()));

        setup::install_shortcuts(&window, &sender, mode);
        setup::install_file_drop(&window, &sender);
        let is_closing = Rc::new(Cell::new(false));
        window.connect_close_request(clone!(
            #[strong] // strong: relm4 ComponentSender (channel, not a widget)
            sender,
            #[strong] // strong: Rc<Cell> close-state flag, not a widget
            is_closing,
            move |_| {
                if is_closing.get() {
                    // A commit is in progress; let the default close proceed
                    // (window.close() re-emits this signal).
                    glib::Propagation::Proceed
                } else {
                    sender.input(ShellMessage::CloseRequested);
                    glib::Propagation::Stop
                }
            }
        ));

        let tick_source = glib::timeout_add_local(
            std::time::Duration::from_millis(200),
            clone!(
                #[strong] // strong: ComponentSender channel; tick source removed on teardown
                sender,
                move || {
                    sender.input(ShellMessage::Tick);
                    glib::ControlFlow::Continue
                }
            ),
        );

        let mut model = Self {
            mode,
            header,
            sidebar,
            queue_view,
            progress,
            video_editor,
            audio_editor,
            window: window.clone(),
            main_stack,
            audio_preview_revealer,
            footer_slot,
            queue: ConversionQueue::new(),
            settings,
            profiles: profiles::load_all_profiles(),
            gpus: detect_all_gpus(),
            gpu_ids: Vec::new(),
            params: None,
            converting: false,
            daemon_job: None,
            daemon_poll_in_flight: false,
            editing_item_id: None,
            current_audio_preview_item: None,
            progress_total: 0,
            progress_done: 0,
            is_closing,
            tick_source: Some(tick_source),
        };

        model.load_saved_profile();

        ComponentParts { model, widgets: () }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        // Panic boundary: in an embedding host (big-host) an uncaught panic
        // here would kill the component runtime and orphan the window; the
        // wrapper tears the window down instead and the process survives.
        let window = self.window.clone();
        big_app_kit::containment::contain_component_update(&window, "converter-shell", || {
            self.dispatch(message, &sender);
        });
    }
}

impl ConverterShell {
    #[allow(clippy::too_many_lines)] // central message dispatch — long by nature
    fn dispatch(&mut self, message: ShellMessage, sender: &ComponentSender<Self>) {
        match message {
            ShellMessage::AddFiles(paths) => self.add_files(&paths),
            ShellMessage::OpenFileChooser => {
                dialogs::show_file_chooser(&self.window, sender, self.mode);
            }
            ShellMessage::OpenFolderChooser => {
                dialogs::show_folder_chooser(&self.window, sender, self.mode);
            }
            ShellMessage::OpenNetworkDialog => {
                dialogs::show_network_url_dialog(&self.window, sender);
            }

            ShellMessage::StartConversion => self.start_conversion(sender),
            ShellMessage::DaemonSubmitFinished(result) => self.finish_daemon_submit(result, sender),
            ShellMessage::DaemonSnapshot(result) => self.apply_daemon_snapshot(result, sender),
            ShellMessage::DaemonCancelFinished(result) => self.finish_daemon_cancel(result),
            ShellMessage::CancelConversion => {
                self.cancel_conversion(sender);
            }
            ShellMessage::ClearQueueRequested => self.confirm_clear_queue(sender),
            ShellMessage::ClearQueueConfirmed => self.clear_queue(),
            ShellMessage::Tick => self.tick_progress(sender),
            ShellMessage::RestoreDefaultsRequested => self.confirm_restore_defaults(sender),

            ShellMessage::ParamsChanged(params) => {
                self.queue.set_profile(params.profile.clone());
                self.persist_gpu_device(params.gpu_index);
                self.params = Some(*params);
            }
            ShellMessage::ShowOutputDirChooser => self.show_output_dir_chooser(),
            ShellMessage::AudioPreview {
                effects,
                volume,
                speed,
            } => {
                if let Some(ae) = &self.audio_editor {
                    ae.emit(AudioEditorInput::ApplyPreview {
                        effects,
                        volume,
                        speed,
                    });
                } else if let Some(ve) = &self.video_editor {
                    ve.emit(VideoEditorInput::ApplyAudioEffects(effects));
                }
            }
            ShellMessage::CutModeActive(active) => {
                if let Some(ae) = &self.audio_editor {
                    ae.emit(AudioEditorInput::SetCutModeActive(active));
                }
            }
            ShellMessage::BindAudioAdjustments { volume, speed } => {
                if let Some(ae) = &self.audio_editor {
                    ae.emit(AudioEditorInput::BindAdjustments { volume, speed });
                }
            }
            ShellMessage::RestoreDefaultProfile => {
                if let Some(default) = self.profiles.get("universal").cloned() {
                    self.sidebar
                        .emit(ConverterSidebarInput::ApplyProfile(Box::new(
                            default.clone(),
                        )));
                    self.queue.set_profile(default);
                    info!("Settings reset to default");
                }
            }
            ShellMessage::MountFooter(footer) => {
                clear_box_children(&self.footer_slot);
                self.footer_slot.append(&footer);
            }

            ShellMessage::QueueRemoveRequested(id) => self.confirm_remove_queue_entry(id, sender),
            ShellMessage::QueueRemoveConfirmed(id) => self.remove_queue_entry(id),
            ShellMessage::QueueOpenFolder(path) => {
                big_app_kit::desktop::open_containing_folder(&path);
            }
            ShellMessage::QueueMetadata(id) => self.show_file_metadata(id),
            ShellMessage::QueueDeleteDisk(id) => self.confirm_delete_disk(id, sender),
            ShellMessage::QueuePlay(path) => self.on_queue_play(&path),
            ShellMessage::QueueSelect(path) => self.on_queue_select(&path),
            ShellMessage::QueueEdit(path) => self.on_queue_edit(&path),
            ShellMessage::QueueReorder(order) => self.queue.reorder(&order),

            ShellMessage::ProgressBack => {
                self.queue.reset_non_done();
                self.queue_view
                    .emit(BigConverterQueueInput::ResetStatusIcons);
                self.main_stack.set_visible_child_name("main");
                self.progress.emit(BigConverterProgressInput::Reset);
            }

            ShellMessage::VideoApplyAndConvert(result) => {
                self.apply_video_edits_and_convert(*result, sender);
            }
            ShellMessage::VideoFullscreen(fs) => self.window.set_fullscreened(fs),
            ShellMessage::VideoBack => {
                if let Some(ve) = &self.video_editor {
                    ve.emit(VideoEditorInput::Unload);
                }
                self.editing_item_id = None;
                self.main_stack.set_visible_child_name("main");
            }

            ShellMessage::AudioSegmentsApplied(ranges) => self.on_audio_segments_applied(ranges),
            ShellMessage::AudioApplyAndConvert(ranges) => {
                self.on_audio_apply_and_convert(ranges, sender);
            }
            ShellMessage::AudioPlaybackState(playing) => {
                self.queue_view
                    .emit(BigConverterQueueInput::SetPlayingState(
                        self.current_audio_preview_item,
                        playing,
                    ));
            }
            ShellMessage::AudioPrevTrack => self.step_audio_track(-1),
            ShellMessage::AudioNextTrack => self.step_audio_track(1),
            ShellMessage::AudioTogglePlayPause => {
                if let Some(ae) = &self.audio_editor {
                    ae.emit(AudioEditorInput::TogglePlayPause);
                }
            }

            ShellMessage::ShowNoiseDialog => {
                self.sidebar.emit(ConverterSidebarInput::ShowNoiseDialog);
            }
            ShellMessage::ShowEqualizerDialog => {
                self.sidebar
                    .emit(ConverterSidebarInput::ShowEqualizerDialog);
            }

            ShellMessage::CloseRequested => self.perform_window_close(sender),
            ShellMessage::ConfirmedClose => self.save_and_close(),
        }
    }
}
