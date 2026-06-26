//! Relm4 wrapper around [`Sidebar`] (encoding settings + audio effects).
//!
//! Push-model Component: it OWNS the 62 sidebar widgets and shows the encoding
//! dialogs internally (they keep binding to its own widgets — encapsulated).
//! Widget callbacks route to the Component's own `Input` so `update()` (holding
//! `&self.sidebar`) can `build_profile()` / show dialogs / read effect state.
//! It persists audio settings + the saved profile internally (holds a cheap
//! [`Settings`] handle) and emits [`ConverterSidebarOutput::ProfileChanged`] for
//! the shell to push onto the queue, plus [`ConverterSidebarOutput::AudioPreviewChanged`]
//! for the active editor. The shell never touches sidebar widgets.

use adw::prelude::*;
use gtk::glib;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::converter::app::ConverterMode;
use crate::converter::profiles::Profile;
use crate::converter::settings::{self as conv_settings, keys};
use crate::converter::ui::dialogs;
use crate::player::mpv_backend::AudioEffectsSnapshot;
use crate::settings::Settings;
use crate::shared::audio_effects::write_gtcrn_controls_raw;
use big_audio_effects::equalizer::EQ_BAND_FREQUENCIES_F64;

use super::Sidebar;

/// Boot configuration for [`ConverterSidebar`].
pub struct ConverterSidebarInit {
    /// Settings handle used for internal persistence of audio + profile state.
    pub settings: Settings,
    /// Binding this sidebar serves (audio or video).
    pub mode: ConverterMode,
}

/// Messages accepted by [`ConverterSidebar`].
#[derive(Debug)]
pub enum ConverterSidebarInput {
    /// Apply a profile to the widgets (e.g. restored from disk).
    ApplyProfile(Box<Profile>),
    /// Set the GPU dropdown labels.
    SetGpus(Vec<String>),
    /// Set the selected GPU combo index (restored from settings).
    SetGpuSelected(u32),
    /// Set the output-directory display.
    SetOutputDir(String),
    /// Switch the sidebar to the conversion page.
    ShowConversionPage,
    /// Switch the sidebar to the editing page.
    ShowEditingPage,
    /// Show the noise / audio-settings dialog (mode-aware).
    ShowNoiseDialog,
    /// Show the equalizer dialog.
    ShowEqualizerDialog,
    /// Enable cut mode (set the cut combo to "cut") if it's currently off —
    /// called when the user marks audio trim segments.
    EnsureCutMode,
    /// Reset every setting to defaults and reload the effect widgets from
    /// GSettings (restore-defaults menu action).
    RestoreDefaults,
    // ── internal (routed from widget signals) ─────────────────────────
    /// A preset profile radio was selected.
    ProfileSelected,
    /// An encoding setting changed.
    SettingsChanged,
    /// The output-dir row/button was clicked.
    OutputDirClicked,
    /// Show the video-encoding dialog.
    ShowEncodingDialog,
    /// Show the audio-encoding dialog.
    ShowAudioDialog,
    /// Show the subtitles dialog.
    ShowSubtitlesDialog,
    /// Show the extra (ffmpeg options) dialog.
    ShowExtraDialog,
    /// An audio-effect control changed (audio-mode live preview).
    EffectsChanged,
    /// An audio-format control (fmt/bitrate/channels/cut) changed.
    AudioFormatChanged,
    /// The cut-mode combo changed.
    CutModeChanged,
}

/// Everything the shell needs from the sidebar to start a conversion.
///
/// Pushed on every settings change so the shell always holds the current set
/// (the shell can't read the sidebar's widgets directly — it's a Component).
#[derive(Debug, Clone)]
pub struct ConversionParams {
    /// Encoding profile built from the sidebar controls.
    pub profile: Profile,
    /// Selected GPU combo index (`0` = Auto).
    pub gpu_index: usize,
    /// Delete-source-files toggle.
    pub delete_source: bool,
    /// Audio cut-mode combo selection (audio mode; `0` = no cut).
    pub audio_cut_mode: u32,
    /// Audio segment output split vs join (audio mode).
    pub audio_segment_split: bool,
}

/// Messages emitted by [`ConverterSidebar`].
#[derive(Debug)]
pub enum ConverterSidebarOutput {
    /// The conversion parameters changed; carries the freshly built set.
    ParamsChanged(Box<ConversionParams>),
    /// The output-directory picker was requested.
    OutputDirClicked,
    /// Live audio preview parameters changed (audio mode).
    AudioPreviewChanged {
        /// Current effect set snapshot.
        effects: AudioEffectsSnapshot,
        /// Volume factor.
        volume: f64,
        /// Speed factor.
        speed: f64,
    },
    /// The cut/trim mode toggled; forward to the audio editor.
    CutModeActive(bool),
    /// The shared volume/speed adjustments, emitted once at startup so the
    /// shell can bind them into the audio editor.
    AudioAdjustmentsReady {
        /// Shared volume adjustment.
        volume: gtk::Adjustment,
        /// Shared speed adjustment.
        speed: gtk::Adjustment,
    },
    /// The user asked to restore default settings (needs the shell's profile map).
    RestoreDefaultRequested,
    /// The output-destination footer bar (folder combo + delete switch), built
    /// from the sidebar's own widgets and emitted once for the shell to mount in
    /// the content area.
    FooterReady(gtk::Box),
}

/// Relm4 wrapper around the converter [`Sidebar`].
pub struct ConverterSidebar {
    sidebar: Sidebar,
    settings: Settings,
}

impl Component for ConverterSidebar {
    type Init = ConverterSidebarInit;
    type Input = ConverterSidebarInput;
    type Output = ConverterSidebarOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let sidebar = Sidebar::new(init.mode);
        root.append(sidebar.widget());

        wire_profile_signals(&sidebar, &sender);
        wire_dialog_signals(&sidebar, &sender);

        // folder_btn opens the output-dir chooser (same as the output row).
        let s = sender.clone();
        sidebar.folder_btn.connect_clicked(move |_| {
            s.input(ConverterSidebarInput::OutputDirClicked);
        });
        let _ = sender.output(ConverterSidebarOutput::FooterReady(build_footer(&sidebar)));

        if sidebar.mode.is_audio() {
            wire_audio_mode_signals(&sidebar, &sender);
            let _ = sender.output(ConverterSidebarOutput::AudioAdjustmentsReady {
                volume: sidebar.audio_vol_adj.clone(),
                speed: sidebar.audio_speed_adj.clone(),
            });
        }

        // Seed the shell's cached params so Convert works before any user
        // interaction (otherwise start_conversion early-returns on None).
        let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(
            build_params(&sidebar),
        )));

        ComponentParts {
            model: Self {
                sidebar,
                settings: init.settings,
            },
            widgets: (),
        }
    }

    #[allow(clippy::too_many_lines)] // central message dispatch — long by nature
    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            ConverterSidebarInput::ApplyProfile(profile) => {
                self.sidebar.apply_profile(&profile);
                let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(
                    build_params(&self.sidebar),
                )));
            }
            ConverterSidebarInput::SetGpus(labels) => self.sidebar.set_gpus(&labels),
            ConverterSidebarInput::SetGpuSelected(idx) => self.sidebar.gpu_combo.set_selected(idx),
            ConverterSidebarInput::SetOutputDir(path) => self.sidebar.set_output_dir(&path),
            ConverterSidebarInput::ShowConversionPage => self.sidebar.show_conversion_page(),
            ConverterSidebarInput::ShowEditingPage => self.sidebar.show_editing_page(),
            ConverterSidebarInput::ProfileSelected => {
                let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(
                    build_params(&self.sidebar),
                )));
            }
            ConverterSidebarInput::SettingsChanged => {
                let params = build_params(&self.sidebar);
                if let Ok(toml) = toml::to_string(&params.profile) {
                    let _ = self.settings.set_string(keys::SAVED_PROFILE.0, &toml);
                    self.settings.flush();
                }
                let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(params)));
            }
            ConverterSidebarInput::OutputDirClicked => {
                let _ = sender.output(ConverterSidebarOutput::OutputDirClicked);
            }
            ConverterSidebarInput::ShowEncodingDialog => {
                dialogs::video_encoding_dialog::show(self.sidebar.widget(), &self.sidebar);
            }
            ConverterSidebarInput::ShowAudioDialog => {
                dialogs::audio_dialog::show(self.sidebar.widget(), &self.sidebar);
            }
            ConverterSidebarInput::ShowNoiseDialog => {
                if self.sidebar.mode.is_audio() {
                    dialogs::noise_dialog::show_without_equalizer(
                        self.sidebar.widget(),
                        &self.sidebar,
                    );
                } else {
                    dialogs::noise_dialog::show(self.sidebar.widget(), &self.sidebar);
                }
            }
            ConverterSidebarInput::ShowSubtitlesDialog => {
                dialogs::subtitles_dialog::show(self.sidebar.widget(), &self.sidebar);
            }
            ConverterSidebarInput::ShowEqualizerDialog => {
                dialogs::equalizer_dialog::show(self.sidebar.widget(), &self.sidebar);
            }
            ConverterSidebarInput::EnsureCutMode => {
                if self.sidebar.audio_cut_combo.selected() == 0 {
                    // set_selected fires selected-notify → AudioFormatChanged +
                    // CutModeChanged, which persist + reveal the cut-out combo.
                    self.sidebar.audio_cut_combo.set_selected(1);
                }
            }
            ConverterSidebarInput::RestoreDefaults => {
                self.settings.reset_to_defaults();
                conv_settings::restore_audio_settings(&self.settings, &self.sidebar);
                let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(
                    build_params(&self.sidebar),
                )));
            }
            ConverterSidebarInput::ShowExtraDialog => {
                let reset_sender = sender.clone();
                dialogs::extra_dialog::show(
                    self.sidebar.widget(),
                    &self.sidebar,
                    || log::info!("Profile export requested"),
                    || log::info!("Profile import requested"),
                    move || {
                        let _ =
                            reset_sender.output(ConverterSidebarOutput::RestoreDefaultRequested);
                    },
                );
            }
            ConverterSidebarInput::EffectsChanged => {
                let effects = self.build_effects_snapshot();
                if effects.nr_enabled {
                    write_gtcrn_controls_raw(
                        effects.nr_strength,
                        effects.nr_model,
                        effects.nr_strength,
                        effects.nr_model_blend,
                        effects.nr_voice_recovery,
                    );
                }
                conv_settings::save_audio_settings(&self.settings, &self.sidebar);
                let _ = sender.output(ConverterSidebarOutput::AudioPreviewChanged {
                    effects,
                    volume: self.sidebar.audio_vol_adj.value(),
                    speed: self.sidebar.audio_speed_adj.value(),
                });
            }
            ConverterSidebarInput::AudioFormatChanged => {
                conv_settings::save_audio_settings(&self.settings, &self.sidebar);
                self.settings.put_string(
                    keys::SEGMENT_MODE,
                    if self.sidebar.audio_cut_out_combo.selected() == 0 {
                        "split"
                    } else {
                        "join"
                    },
                );
                self.settings.flush();
                let _ = sender.output(ConverterSidebarOutput::ParamsChanged(Box::new(
                    build_params(&self.sidebar),
                )));
            }
            ConverterSidebarInput::CutModeChanged => {
                let active = self.sidebar.audio_cut_combo.selected() != 0;
                self.sidebar.audio_cut_out_combo.set_visible(active);
                let _ = sender.output(ConverterSidebarOutput::CutModeActive(active));
            }
        }
    }
}

/// Snapshot everything the shell needs to start a conversion.
fn build_params(sidebar: &Sidebar) -> ConversionParams {
    ConversionParams {
        profile: sidebar.build_profile(),
        gpu_index: sidebar.gpu_combo.selected() as usize,
        delete_source: sidebar.delete_switch.is_active(),
        audio_cut_mode: sidebar.audio_cut_combo.selected(),
        audio_segment_split: sidebar.audio_cut_out_combo.selected() == 0,
    }
}

impl ConverterSidebar {
    /// Build the owned effect snapshot from the current effect-widget state,
    /// mirroring the values the filter chain reads.
    fn build_effects_snapshot(&self) -> AudioEffectsSnapshot {
        let sb = &self.sidebar;
        let eq_bands: Vec<(f64, f64)> = if sb.eq_enabled_switch.is_active() {
            sb.eq_band_adjs
                .iter()
                .enumerate()
                .filter_map(|(i, adj)| EQ_BAND_FREQUENCIES_F64.get(i).map(|&f| (f, adj.value())))
                .collect()
        } else {
            Vec::new()
        };

        AudioEffectsSnapshot {
            normalize: sb.normalize_switch.is_active(),
            nr_enabled: sb.nr_enabled_switch.is_active(),
            nr_strength: sb.nr_speech_adj.value(),
            nr_model: sb.nr_model_combo.selected() as i32,
            nr_model_blend: false,
            nr_voice_recovery: sb.nr_recovery_adj.value(),
            gate_enabled: sb.gate_enabled_switch.is_active(),
            gate_intensity: sb.gate_intensity_adj.value(),
            hpf_enabled: sb.hpf_enabled_switch.is_active(),
            hpf_frequency: sb.hpf_freq_adj.value(),
            eq_bands,
            compressor_enabled: sb.compressor_enabled_switch.is_active(),
            compressor_intensity: sb.compressor_intensity_adj.value(),
        }
    }
}

/// Build the output-destination footer bar from the sidebar's own widgets
/// (ported from the old window `build_ui`). The shell mounts the returned box
/// at the bottom of the content area.
fn build_footer(sidebar: &Sidebar) -> gtk::Box {
    use crate::i18n::i18n;

    let footer = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    footer.set_margin_start(12);
    footer.set_margin_end(12);
    footer.set_margin_top(6);
    footer.set_margin_bottom(6);

    sidebar.folder_combo.set_hexpand(false);
    footer.append(&sidebar.folder_combo);

    let folder_entry_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    folder_entry_box.set_visible(false);
    folder_entry_box.append(&sidebar.folder_entry);
    folder_entry_box.append(&sidebar.folder_btn);
    footer.append(&folder_entry_box);
    {
        let feb = folder_entry_box.clone();
        sidebar
            .folder_combo
            .connect_selected_notify(move |dd| feb.set_visible(dd.selected() == 1));
    }

    let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    footer.append(&spacer);

    let delete_box = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    delete_box.set_valign(gtk::Align::Center);
    delete_box.append(&gtk::Label::new(Some(&i18n("Delete original files"))));
    // The bare switch has only an adjacent visual label; give AT-SPI an
    // explicit accessible label so it isn't reported as unnamed.
    sidebar
        .delete_switch
        .update_property(&[gtk::accessible::Property::Label(&i18n(
            "Delete original files",
        ))]);
    delete_box.append(&sidebar.delete_switch);
    footer.append(&delete_box);

    footer
}

fn wire_profile_signals(sidebar: &Sidebar, sender: &ComponentSender<ConverterSidebar>) {
    let s = sender.clone();
    sidebar.connect_profile_selected(move |_preset| {
        s.input(ConverterSidebarInput::ProfileSelected);
    });
    let s = sender.clone();
    sidebar.connect_settings_changed(move |()| {
        s.input(ConverterSidebarInput::SettingsChanged);
    });
    let s = sender.clone();
    sidebar.connect_output_dir_clicked(move |()| {
        s.input(ConverterSidebarInput::OutputDirClicked);
    });
    // GPU + delete-source feed the cached conversion params; refresh on change.
    let s = sender.clone();
    sidebar.gpu_combo.connect_selected_notify(move |_| {
        s.input(ConverterSidebarInput::ProfileSelected);
    });
    let s = sender.clone();
    sidebar.delete_switch.connect_active_notify(move |_| {
        s.input(ConverterSidebarInput::ProfileSelected);
    });
}

fn wire_dialog_signals(sidebar: &Sidebar, sender: &ComponentSender<ConverterSidebar>) {
    let s = sender.clone();
    sidebar.connect_show_encoding_dialog(move |()| {
        s.input(ConverterSidebarInput::ShowEncodingDialog);
    });
    let s = sender.clone();
    sidebar.connect_show_audio_dialog(move |()| {
        s.input(ConverterSidebarInput::ShowAudioDialog);
    });
    let s = sender.clone();
    sidebar.connect_show_noise_dialog(move |()| {
        s.input(ConverterSidebarInput::ShowNoiseDialog);
    });
    let s = sender.clone();
    sidebar.connect_show_subtitles_dialog(move |()| {
        s.input(ConverterSidebarInput::ShowSubtitlesDialog);
    });
    let s = sender.clone();
    sidebar.connect_show_extra_dialog(move |()| {
        s.input(ConverterSidebarInput::ShowExtraDialog);
    });
}

#[allow(clippy::too_many_lines)] // audio-effect signal fan-out — long by nature
fn wire_audio_mode_signals(sidebar: &Sidebar, sender: &ComponentSender<ConverterSidebar>) {
    // Effect controls → live preview refresh.
    let effect = |sender: &ComponentSender<ConverterSidebar>| {
        let s = sender.clone();
        move || s.input(ConverterSidebarInput::EffectsChanged)
    };

    let f = effect(sender);
    sidebar.nr_enabled_switch.connect_state_set(move |_, _| {
        f();
        glib::Propagation::Proceed
    });
    let f = effect(sender);
    sidebar.nr_strength_adj.connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.nr_model_combo.connect_selected_notify(move |_| f());
    let f = effect(sender);
    sidebar.nr_speech_adj.connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.nr_recovery_adj.connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.gate_enabled_switch.connect_state_set(move |_, _| {
        f();
        glib::Propagation::Proceed
    });
    let f = effect(sender);
    sidebar.normalize_switch.connect_state_set(move |_, _| {
        f();
        glib::Propagation::Proceed
    });
    let f = effect(sender);
    sidebar
        .compressor_enabled_switch
        .connect_state_set(move |_, _| {
            f();
            glib::Propagation::Proceed
        });
    let f = effect(sender);
    sidebar
        .compressor_intensity_adj
        .connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.hpf_enabled_switch.connect_state_set(move |_, _| {
        f();
        glib::Propagation::Proceed
    });
    let f = effect(sender);
    sidebar.hpf_freq_adj.connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.eq_enabled_switch.connect_state_set(move |_, _| {
        f();
        glib::Propagation::Proceed
    });
    let f = effect(sender);
    sidebar
        .eq_preset_combo
        .connect_selected_notify(move |_| f());
    for adj in &sidebar.eq_band_adjs {
        let f = effect(sender);
        adj.connect_value_changed(move |_| f());
    }
    let f = effect(sender);
    sidebar.audio_vol_adj.connect_value_changed(move |_| f());
    let f = effect(sender);
    sidebar.audio_speed_adj.connect_value_changed(move |_| f());

    // Audio-format controls → persist + profile rebuild.
    let fmt = |sender: &ComponentSender<ConverterSidebar>| {
        let s = sender.clone();
        move || s.input(ConverterSidebarInput::AudioFormatChanged)
    };
    let f = fmt(sender);
    sidebar
        .audio_fmt_combo
        .connect_selected_notify(move |_| f());
    let f = fmt(sender);
    sidebar.audio_br_combo.connect_selected_notify(move |_| f());
    let f = fmt(sender);
    sidebar.audio_ch_combo.connect_selected_notify(move |_| f());
    let f = fmt(sender);
    sidebar
        .audio_cut_out_combo
        .connect_selected_notify(move |_| f());

    // Cut-mode combo → toggle cut-out visibility + notify the editor.
    let s = sender.clone();
    sidebar.audio_cut_combo.connect_selected_notify(move |_| {
        s.input(ConverterSidebarInput::AudioFormatChanged);
        s.input(ConverterSidebarInput::CutModeChanged);
    });
}
