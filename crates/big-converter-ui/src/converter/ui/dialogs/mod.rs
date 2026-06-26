//! Converter dialogs: audio, equalizer, extra (profile import/export +
//! reset), file info, noise reduction, subtitles, video encoding. All
//! mirror sidebar state through bidirectional widget bindings — no Apply
//! button.

pub mod audio_dialog;
pub mod equalizer_dialog;
pub mod extra_dialog;
pub mod file_metadata_dialog;
pub mod noise_dialog;
pub mod subtitles_dialog;
pub mod video_encoding_dialog;
