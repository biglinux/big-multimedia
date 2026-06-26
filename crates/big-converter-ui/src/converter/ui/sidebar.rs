//! Sidebar root: ViewStack with conversion + editing pages, plus the
//! per-codec/per-format ID/label tables shared by combos and profile
//! serialization. Hidden ComboRow/Switch/Adjustment widgets serve as the
//! authoritative state holders that dialog controls bind back to.

#![allow(clippy::needless_borrows_for_generic_args)]

mod bindings;
mod build;
mod combo;
mod component;
mod conversion_page;
mod editing_page;
mod profile;

use std::cell::RefCell;
use std::rc::Rc;

pub use combo::{combo_id, make_combo, set_combo_by_id};
pub use component::{
    ConversionParams, ConverterSidebar, ConverterSidebarInit, ConverterSidebarInput,
    ConverterSidebarOutput,
};

type Callback<T> = Rc<RefCell<Option<Box<dyn Fn(T)>>>>;

fn new_cb<T>() -> Callback<T> {
    Rc::new(RefCell::new(None))
}

fn fire<T: Clone>(cb: &Callback<T>, val: T) {
    if let Some(ref f) = *cb.borrow() {
        f(val);
    }
}

// IDs (machine-readable, never translated). User-visible LABELS live in
// `big_media_runtime::ui_labels` so every string flows through `i18n()`.

/// Video-codec id table; index matches the codec combo row order.
pub const CODEC_IDS: &[&str] = &["copy", "h264", "h265", "av1", "vp9", "prores"];
/// Video quality preset id table.
pub const QUALITY_IDS: &[&str] = &[
    "veryhigh", "high", "good", "medium", "low", "verylow", "superlow",
];
/// FFmpeg `-preset` speed/efficiency id table.
pub const PRESET_IDS: &[&str] = &[
    "ultrafast",
    "veryfast",
    "fast",
    "balanced",
    "slow",
    "veryslow",
];
/// Output resolution id table (`"custom"` enables the entry).
pub const RESOLUTION_IDS: &[&str] = &[
    "auto",
    "3840x2160",
    "2560x1440",
    "1920x1080",
    "1280x720",
    "854x480",
    "640x360",
    "custom",
];
/// Container/extension id table for video mode.
pub const CONTAINER_IDS: &[&str] = &["mp4", "mkv", "mov", "webm"];
/// Audio-handling id table: copy, re-encode, or drop the audio stream.
pub const AUDIO_HANDLING_IDS: &[&str] = &["copy", "encode", "none"];
/// Audio-codec id table for video mode.
pub const AUDIO_CODEC_IDS: &[&str] = &["aac", "opus", "mp3", "ac3", "flac", "pcm"];
/// Audio-bitrate id table for video mode (`"custom"` enables the entry).
pub const AUDIO_BITRATE_IDS: &[&str] = &["64k", "96k", "128k", "192k", "256k", "320k", "custom"];
/// Audio-channel id table for video mode (`"custom"` enables the entry).
pub const AUDIO_CHANNELS_IDS: &[&str] = &["auto", "1", "2", "6", "custom"];
/// Subtitle-handling id table.
pub const SUBTITLE_IDS: &[&str] = &["extract", "embedded", "burn", "none"];
/// Output-format id table used in audio-only mode.
pub const AUDIO_FORMAT_IDS: &[&str] = &["copy", "mp3", "ogg", "flac", "wav", "aac", "opus"];
/// Audio-bitrate id table used in audio-only mode.
pub const AUDIO_BITRATE_IDS_A: &[&str] = &["32k", "64k", "128k", "192k", "256k", "320k"];
/// Audio-channel id table used in audio-only mode.
pub const AUDIO_CHANNELS_IDS_A: &[&str] = &["auto", "1", "2"];

/// Helper: turn an owned label `Vec<String>` into a `Vec<&str>` for APIs that
/// take `&[&str]`. Caller must keep the owned vec alive for the slice's life.
#[must_use]
pub fn as_str_refs(labels: &[String]) -> Vec<&str> {
    labels.iter().map(String::as_str).collect()
}

/// Pre-canned encoding profile selected from the sidebar radio group.
///
/// [`PresetProfile::Custom`] hands off to the free-form encoding
/// dialog instead of applying preset values.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PresetProfile {
    /// Stream-copy whenever possible — fastest, no re-encode.
    FastCopy,
    /// Universal compatibility preset (H.264/AAC, MP4).
    Universal,
    /// Smaller-file preset using a more efficient codec.
    MoreEfficient,
    /// Aggressive size preset trading quality for footprint.
    SmallerFile,
    /// Free-form configuration via the encoding dialog.
    Custom,
}

/// Sidebar root: ViewStack with conversion + editing pages plus
/// hidden ComboRow/Switch/Adjustment widgets that act as the
/// authoritative state holders for the dialog controls.
///
/// Public widget fields are widget handles consumed by the dialog
/// layer and the window signal modules within this crate; they are
/// intentionally not part of the supported external API surface, even
/// though they must be `pub` so sibling modules can reach them.
#[allow(missing_docs)]
pub struct Sidebar {
    /// Binding this sidebar instance renders (audio or video).
    pub mode: crate::converter::app::ConverterMode,
    container: gtk::Box,
    view_stack: adw::ViewStack,
    gpu_group: adw::PreferencesGroup,
    radio_copy: gtk::CheckButton,
    radio_universal: gtk::CheckButton,
    radio_smaller: gtk::CheckButton,
    radio_quality: gtk::CheckButton,
    radio_custom: gtk::CheckButton,
    customize_row: adw::ActionRow,
    audio_encoding_row: adw::ActionRow,
    audio_settings_row: adw::ActionRow,
    subtitles_row: adw::ActionRow,
    extra_row: adw::ActionRow,
    audio_settings_row_audio_mode: Option<adw::ActionRow>,
    pub video_codec_combo: adw::ComboRow,
    pub video_quality_combo: adw::ComboRow,
    pub preset_combo: adw::ComboRow,
    pub resolution_combo: adw::ComboRow,
    pub output_format_combo: adw::ComboRow,
    pub gpu_combo: adw::ComboRow,
    pub gpu_device_combo: adw::ComboRow,
    pub audio_handling_combo: adw::ComboRow,
    pub audio_codec_combo: adw::ComboRow,
    pub audio_bitrate_combo: adw::ComboRow,
    pub audio_channels_combo: adw::ComboRow,
    pub subtitle_combo: adw::ComboRow,
    pub gpu_partial_switch: adw::SwitchRow,
    pub only_extract_subs_switch: adw::SwitchRow,
    pub custom_resolution_entry: gtk::Entry,
    pub custom_bitrate_entry: gtk::Entry,
    pub custom_channels_entry: gtk::Entry,
    pub ffmpeg_options_entry: gtk::Entry,
    pub nr_enabled_switch: gtk::Switch,
    pub nr_strength_adj: gtk::Adjustment,
    pub nr_model_combo: gtk::DropDown,
    pub nr_speech_adj: gtk::Adjustment,
    pub nr_lookahead_adj: gtk::Adjustment,
    pub nr_recovery_adj: gtk::Adjustment,
    pub gate_enabled_switch: gtk::Switch,
    pub gate_intensity_adj: gtk::Adjustment,
    pub hpf_enabled_switch: gtk::Switch,
    pub hpf_freq_adj: gtk::Adjustment,
    pub normalize_switch: gtk::Switch,
    pub compressor_enabled_switch: gtk::Switch,
    pub compressor_intensity_adj: gtk::Adjustment,
    pub eq_enabled_switch: gtk::Switch,
    pub eq_preset_combo: gtk::DropDown,
    pub eq_band_adjs: Vec<gtk::Adjustment>,
    pub audio_fmt_combo: adw::ComboRow,
    pub audio_br_combo: adw::ComboRow,
    pub audio_ch_combo: adw::ComboRow,
    pub audio_cut_combo: adw::ComboRow,
    pub audio_cut_out_combo: adw::ComboRow,
    pub audio_vol_adj: gtk::Adjustment,
    pub audio_speed_adj: gtk::Adjustment,
    pub editing_box: gtk::Box,
    pub brightness_scale: gtk::Scale,
    pub contrast_scale: gtk::Scale,
    pub saturation_scale: gtk::Scale,
    pub hue_scale: gtk::Scale,
    pub rotate_ccw_btn: gtk::Button,
    pub rotate_cw_btn: gtk::Button,
    pub flip_h_btn: gtk::Button,
    pub flip_v_btn: gtk::Button,
    pub reset_transform_btn: gtk::Button,
    pub segments_listbox: gtk::ListBox,
    pub segments_placeholder: adw::ActionRow,
    pub segments_list_row: adw::ActionRow,
    pub add_segment_btn: gtk::Button,
    pub clear_segments_btn: gtk::Button,
    pub output_mode_combo: gtk::DropDown,
    pub nr_sidebar_group: adw::PreferencesGroup,
    nr_sidebar_row: adw::ActionRow,
    output_row: adw::ActionRow,
    pub delete_switch: gtk::Switch,
    pub folder_combo: gtk::DropDown,
    pub folder_entry: gtk::Entry,
    pub folder_btn: gtk::Button,
    on_profile_selected: Callback<PresetProfile>,
    on_show_encoding_dialog: Callback<()>,
    on_show_audio_dialog: Callback<()>,
    on_show_noise_dialog: Callback<()>,
    on_show_subtitles_dialog: Callback<()>,
    on_show_extra_dialog: Callback<()>,
    on_output_dir_clicked: Callback<()>,
    on_settings_changed: Callback<()>,
}

impl Sidebar {
    /// Build the sidebar widget tree for the given binding.
    #[must_use]
    pub fn new(mode: crate::converter::app::ConverterMode) -> Self {
        build::build_sidebar(mode)
    }
}
