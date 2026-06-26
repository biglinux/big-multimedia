//! Sidebar editing tab adapter.

use adw::prelude::*;
use big_media_components::video::editor_sidebar::{
    BigVideoEditorSidebarText, build_video_editor_sidebar,
};

use crate::i18n::i18n;

pub(super) struct EditingPageWidgets {
    pub(super) edit_scroll: gtk::ScrolledWindow,
    pub(super) editing_box: gtk::Box,
    pub(super) brightness_scale: gtk::Scale,
    pub(super) contrast_scale: gtk::Scale,
    pub(super) saturation_scale: gtk::Scale,
    pub(super) hue_scale: gtk::Scale,
    pub(super) rotate_ccw_btn: gtk::Button,
    pub(super) rotate_cw_btn: gtk::Button,
    pub(super) flip_h_btn: gtk::Button,
    pub(super) flip_v_btn: gtk::Button,
    pub(super) reset_transform_btn: gtk::Button,
    pub(super) segments_listbox: gtk::ListBox,
    pub(super) segments_placeholder: adw::ActionRow,
    pub(super) segments_list_row: adw::ActionRow,
    pub(super) add_segment_btn: gtk::Button,
    pub(super) clear_segments_btn: gtk::Button,
    pub(super) output_mode_combo: gtk::DropDown,
    pub(super) nr_sidebar_group: adw::PreferencesGroup,
    pub(super) nr_sidebar_row: adw::ActionRow,
}

pub(super) fn build_editing_page() -> EditingPageWidgets {
    let built = build_video_editor_sidebar(BigVideoEditorSidebarText::translated(i18n));
    built.audio_group.set_visible(false);

    EditingPageWidgets {
        edit_scroll: built.sidebar_scroll,
        editing_box: built.sidebar_box,
        brightness_scale: built.brightness_scale,
        contrast_scale: built.contrast_scale,
        saturation_scale: built.saturation_scale,
        hue_scale: built.hue_scale,
        rotate_ccw_btn: built.rot_left_button,
        rotate_cw_btn: built.rot_right_button,
        flip_h_btn: built.flip_h_button,
        flip_v_btn: built.flip_v_button,
        reset_transform_btn: built.reset_transform_button,
        segments_listbox: built.segments_listbox,
        segments_placeholder: built.segments_placeholder,
        segments_list_row: built.segments_list_row,
        add_segment_btn: built.add_segment_button,
        clear_segments_btn: built.clear_segs_button,
        output_mode_combo: built.output_dropdown,
        nr_sidebar_group: built.audio_group,
        nr_sidebar_row: built.audio_row,
    }
}
