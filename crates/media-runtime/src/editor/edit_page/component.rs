//! Relm4 wrapper around [`EditPage`] (video editor).
//!
//! Push-model Component: widget callbacks that need no editor state emit
//! [`VideoEditorOutput`] directly; the convert button routes to the
//! Component's own [`VideoEditorInput::ConvertRequested`] so `update()` (which
//! holds `&self.page`) can gather the full edit state via the page's getters
//! and emit [`VideoEditorOutput::ApplyAndConvert`]. The mpv/GL internals are
//! untouched. Manual `impl Component` idiom (no `view!` macro).

use std::path::PathBuf;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::converter::ffmpeg::TrimRange;
use crate::converter::profiles::CropRect;
use crate::converter::queue::VideoOverrides;
use crate::player::mpv_backend::AudioEffectsSnapshot;

use super::{EditPage, TrimSegment};

/// Full edit state gathered when the user converts from the video editor.
#[derive(Debug, Clone)]
pub struct VideoEditResult {
    /// Trim ranges built from the editor's segment list.
    pub ranges: Vec<TrimRange>,
    /// Codec/quality/filter overrides chosen in the editor.
    pub overrides: VideoOverrides,
    /// Crop rectangle, if the user cropped.
    pub crop: Option<CropRect>,
    /// `true` if each segment becomes its own output file.
    pub segment_mode_split: bool,
}

/// Messages accepted by [`VideoEditor`].
#[derive(Debug)]
pub enum VideoEditorInput {
    /// Load `path` into the editor (allocates mpv lazily on first call).
    LoadFile(PathBuf),
    /// Set split vs join segment output mode.
    SetSegmentModeSplit(bool),
    /// Push live audio-filter effects to the preview.
    ApplyAudioEffects(AudioEffectsSnapshot),
    /// Tear down the loaded file + mpv backend.
    Unload,
    /// Internal: the convert button was clicked — gather state and emit
    /// [`VideoEditorOutput::ApplyAndConvert`].
    ConvertRequested,
}

/// User actions emitted by [`VideoEditor`].
#[derive(Debug)]
pub enum VideoEditorOutput {
    /// Convert clicked: apply the gathered edits to the item and start.
    ApplyAndConvert(VideoEditResult),
    /// Open the audio-settings (noise) dialog.
    ShowAudioSettings,
    /// Fullscreen toggled.
    Fullscreen(bool),
    /// Back to the queue requested.
    Back,
    /// The in-memory segment list changed (add/remove/edit).
    SegmentsChanged(Vec<TrimSegment>),
}

/// Relm4 wrapper around the video [`EditPage`].
pub struct VideoEditor {
    page: EditPage,
}

impl Component for VideoEditor {
    type Init = ();
    type Input = VideoEditorInput;
    type Output = VideoEditorOutput;
    type CommandOutput = ();
    type Root = gtk::Box;
    type Widgets = ();

    fn init_root() -> Self::Root {
        gtk::Box::new(gtk::Orientation::Vertical, 0)
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let page = EditPage::new();
        root.append(page.widget());

        let s = sender.clone();
        page.connect_back(move |()| {
            let _ = s.output(VideoEditorOutput::Back);
        });
        let s = sender.clone();
        page.connect_fullscreen(move |fs| {
            let _ = s.output(VideoEditorOutput::Fullscreen(fs));
        });
        let s = sender.clone();
        page.connect_show_audio_settings(move |()| {
            let _ = s.output(VideoEditorOutput::ShowAudioSettings);
        });
        let s = sender.clone();
        page.connect_segments_changed(move |segments| {
            let _ = s.output(VideoEditorOutput::SegmentsChanged(segments));
        });
        // The convert button fires on_apply_trim then on_convert; we ignore
        // on_apply_trim and gather the complete edit state in update() instead.
        let s = sender.clone();
        page.connect_convert(move |()| {
            s.input(VideoEditorInput::ConvertRequested);
        });

        ComponentParts {
            model: Self { page },
            widgets: (),
        }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, _root: &Self::Root) {
        match message {
            VideoEditorInput::LoadFile(path) => self.page.load_file(&path),
            VideoEditorInput::SetSegmentModeSplit(split) => {
                self.page.set_segment_mode_split(split);
            }
            VideoEditorInput::ApplyAudioEffects(snapshot) => {
                self.page.apply_audio_effects(&snapshot.as_effects());
            }
            VideoEditorInput::Unload => self.page.unload(),
            VideoEditorInput::ConvertRequested => {
                let ranges: Vec<TrimRange> = self
                    .page
                    .segments()
                    .into_iter()
                    .map(|segment| TrimRange {
                        start: Some(segment.start),
                        end: Some(segment.end),
                    })
                    .collect();
                let result = VideoEditResult {
                    ranges,
                    overrides: self.page.video_overrides(),
                    crop: self.page.crop_rect(),
                    segment_mode_split: self.page.segment_mode_split(),
                };
                let _ = sender.output(VideoEditorOutput::ApplyAndConvert(result));
            }
        }
    }
}
