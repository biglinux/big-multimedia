//! Video editor sidebar adapter.

use big_media_components::video::editor_sidebar::{
    BigVideoEditorSidebarBuild as SidebarBuild, BigVideoEditorSidebarText,
    build_video_editor_sidebar,
};

use crate::i18n::i18n;

pub(super) fn build_sidebar() -> SidebarBuild {
    build_video_editor_sidebar(BigVideoEditorSidebarText::translated(i18n))
}
