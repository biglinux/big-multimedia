//! Video editor preview panel adapter.

use crate::i18n::i18n;
use crate::ui_labels;
use big_media_components::video::editor_panel::{
    BigVideoEditorPanelBuild as VideoPanelBuild, BigVideoEditorPanelText, build_video_editor_panel,
};

pub(super) fn build_video_panel() -> VideoPanelBuild {
    let speed = ui_labels::speed_labels();
    let speed_refs: Vec<&str> = speed.iter().map(String::as_str).collect();
    build_video_editor_panel(BigVideoEditorPanelText::translated(i18n), &speed_refs)
}
