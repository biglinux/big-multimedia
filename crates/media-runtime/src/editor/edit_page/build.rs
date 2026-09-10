//! `EditPage` widget tree assembler — wires sidebar + video panel,
//! installs segment-overlay/draw/actions/crop interaction handlers.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use big_relm4_components::feedback::tooltip;
use big_relm4_components::layout::toolbar_pane::{
    BigToolbarHeader, BigToolbarHeaderSpec, BigToolbarPane, BigToolbarPaneSpec,
};

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::player::mpv_backend::fast_render::FastRenderContext;

use super::DEFAULT_SIDEBAR_POS;
use super::sidebar::build_sidebar;
use super::video_panel::build_video_panel;
use super::{Callback, EditPage, TrimSegment, actions, crop, fire, new_cb, render, segments};

pub(super) struct EditPageBuild {
    pub(super) page: EditPage,
    pub(super) brightness_reset: gtk::Button,
    pub(super) contrast_reset: gtk::Button,
    pub(super) saturation_reset: gtk::Button,
    pub(super) hue_reset: gtk::Button,
    pub(super) rot_left_button: gtk::Button,
    pub(super) rot_right_button: gtk::Button,
    pub(super) flip_h_button: gtk::Button,
    pub(super) flip_v_button: gtk::Button,
    pub(super) reset_transform_button: gtk::Button,
    pub(super) audio_row: adw::ActionRow,
    pub(super) add_segs_button: gtk::Button,
    pub(super) clear_segs_button: gtk::Button,
    pub(super) seek_back_button: gtk::Button,
    pub(super) frame_back_button: gtk::Button,
    pub(super) frame_fwd_button: gtk::Button,
    pub(super) seek_fwd_button: gtk::Button,
    pub(super) speed_box: gtk::Box,
    pub(super) crop_reset_button: gtk::Button,
    pub(super) updating_crop: Rc<Cell<bool>>,
}

pub(super) fn build_edit_page() -> EditPage {
    let build = build_widget_tree();
    segments::install_segment_overlay_behavior(&build);
    render::install_draw_handlers(&build);
    actions::install_edit_page_actions(&build);
    crop::install_crop_interactions(&build);
    build.page
}

#[allow(clippy::too_many_lines)] // GTK widget-tree builder — long by nature
fn build_widget_tree() -> EditPageBuild {
    let header_shell = BigToolbarHeader::new(BigToolbarHeaderSpec::new());
    let back_button = tooltip::icon_button("go-previous-symbolic", &i18n("Back to queue"), &[]);
    header_shell.pack_start(&back_button);

    let convert_button = gtk::Button::builder()
        .label(i18n("Convert this file"))
        .css_classes(["suggested-action"])
        .build();
    header_shell.pack_end(&convert_button);
    let header = header_shell.into_root();

    let sidebar = build_sidebar();
    let video = build_video_panel();

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Horizontal)
        .start_child(&sidebar.sidebar_scroll)
        .end_child(&video.video_panel)
        .position(DEFAULT_SIDEBAR_POS)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .resize_start_child(false)
        .resize_end_child(true)
        .build();

    let toolbar_pane = BigToolbarPane::new(BigToolbarPaneSpec::new());
    toolbar_pane.add_top_bar(&header);
    toolbar_pane.set_content(&paned);
    let toolbar_view = toolbar_pane.into_root();

    let mpv = Rc::new(RefCell::new(None::<MpvBackend>));
    let render_context = Rc::new(RefCell::new(None::<FastRenderContext>));
    let video_backend: Rc<
        RefCell<Option<crate::player::mpv_backend::offload_renderer::VideoBackend>>,
    > = Rc::new(RefCell::new(None));
    let segments = Rc::new(RefCell::new(Vec::<TrimSegment>::new()));
    let mark_point = Rc::new(Cell::new(None));
    let duration = Rc::new(Cell::new(0.0));
    let position = Rc::new(Cell::new(0.0));
    let seeking = Rc::new(Cell::new(false));
    let is_fullscreen = Rc::new(Cell::new(false));
    let show_grid = Rc::new(Cell::new(false));
    let rotation = Rc::new(Cell::new(0));
    let flip_h = Rc::new(Cell::new(false));
    let flip_v = Rc::new(Cell::new(false));
    let video_width = Rc::new(Cell::new(0));
    let video_height = Rc::new(Cell::new(0));
    let file_path = Rc::new(RefCell::new(None::<String>));
    let hide_timer = Rc::new(Cell::new(None::<gtk::glib::SourceId>));
    let on_apply_trim = new_cb();
    let on_segments_changed: Callback<Vec<TrimSegment>> = new_cb();
    let on_fullscreen: Callback<bool> = new_cb();
    let on_convert: Callback<()> = new_cb();
    let on_show_audio_settings: Callback<()> = new_cb();
    let on_back: Callback<()> = new_cb();
    let updating_crop = Rc::new(Cell::new(false));

    {
        let cb = on_back.clone();
        back_button.connect_clicked(move |_| fire(&cb, ()));
    }

    let page = EditPage {
        toolbar_view,
        header,
        convert_button,
        back_button,
        paned,
        brightness_scale: sidebar.brightness_scale,
        contrast_scale: sidebar.contrast_scale,
        saturation_scale: sidebar.saturation_scale,
        hue_scale: sidebar.hue_scale,
        rotation,
        flip_h,
        flip_v,
        segments_listbox: sidebar.segments_listbox,
        segments_placeholder: sidebar.segments_placeholder,
        segments_list_row: sidebar.segments_list_row,
        output_dropdown: sidebar.output_dropdown,
        segment_overlay: video.segment_overlay,
        video_area: video.video_area,
        overlay: video.overlay,
        transport_overlay_box: video.transport_overlay_box,
        timeline: video.timeline,
        position_label: video.position_label,
        duration_label: gtk::Label::new(None),
        frame_label: video.frame_label,
        play_button: video.play_button,
        mark_button: video.mark_button,
        speed_button: video.speed_button,
        volume_button: video.volume_button,
        volume_scale: video.volume_scale,
        fullscreen_button: video.fullscreen_button,
        audio_track_button: video.audio_track_button,
        subtitle_track_button: video.subtitle_track_button,
        audio_track_box: video.audio_track_box,
        subtitle_track_box: video.subtitle_track_box,
        crop_left: video.crop_left,
        crop_right: video.crop_right,
        crop_top: video.crop_top,
        crop_bottom: video.crop_bottom,
        crop_overlay_area: video.crop_overlay_area,
        show_grid,
        grid_button: video.grid_button,
        media_metadata_resolution: video.media_metadata_resolution,
        media_metadata_codec: video.media_metadata_codec,
        media_metadata_file_size: video.media_metadata_file_size,
        media_metadata_duration: video.media_metadata_duration,
        segments,
        mark_point,
        duration,
        position,
        seeking,
        is_fullscreen,
        video_width,
        video_height,
        file_path,
        mpv,
        render_ctx: render_context,
        video_backend,
        realize_wired: Rc::new(Cell::new(false)),
        af_timer: Rc::new(Cell::new(None)),
        hide_timer,
        on_apply_trim,
        on_segments_changed,
        on_fullscreen,
        on_convert,
        on_show_audio_settings,
        on_back,
    };

    EditPageBuild {
        page,
        brightness_reset: sidebar.brightness_reset,
        contrast_reset: sidebar.contrast_reset,
        saturation_reset: sidebar.saturation_reset,
        hue_reset: sidebar.hue_reset,
        rot_left_button: sidebar.rot_left_button,
        rot_right_button: sidebar.rot_right_button,
        flip_h_button: sidebar.flip_h_button,
        flip_v_button: sidebar.flip_v_button,
        reset_transform_button: sidebar.reset_transform_button,
        audio_row: sidebar.audio_row,
        add_segs_button: sidebar.add_segment_button,
        clear_segs_button: sidebar.clear_segs_button,
        seek_back_button: video.seek_back_button,
        frame_back_button: video.frame_back_button,
        frame_fwd_button: video.frame_fwd_button,
        seek_fwd_button: video.seek_fwd_button,
        speed_box: video.speed_box,
        crop_reset_button: video.crop_reset_button,
        updating_crop,
    }
}
