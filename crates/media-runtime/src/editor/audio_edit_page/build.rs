//! Audio edit page builder adapter.

use big_media_components::audio::editor_panel::{
    BigAudioEditorPanelBuild, BigAudioEditorPanelText, build_audio_editor_panel,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::i18n::i18n;

use super::{AudioEditPage, AudioSegment, DragMode, MpvBackend, actions, new_cb, waveform};

pub(super) struct AudioEditPageBuild {
    pub(super) page: AudioEditPage,
    pub(super) apply_button: gtk::Button,
    pub(super) convert_button: gtk::Button,
    pub(super) prev_button: gtk::Button,
    pub(super) next_button: gtk::Button,
    pub(super) clear_button: gtk::Button,
    pub(super) settings_button: gtk::Button,
    pub(super) eq_toggle_button: gtk::Button,
    pub(super) zoom_button: gtk::Button,
}

pub(super) fn build_audio_edit_page() -> AudioEditPage {
    let build = build_widget_tree();
    actions::install_basic_actions(&build);
    waveform::install_waveform_behavior(&build);
    build.page
}

fn build_widget_tree() -> AudioEditPageBuild {
    let panel = build_audio_editor_panel(BigAudioEditorPanelText::translated(i18n));
    let BigAudioEditorPanelBuild {
        toolbar,
        header,
        root,
        controls,
        trim_group,
        play_button,
        prev_button,
        next_button,
        play_selection_button,
        auto_advance_button,
        eq_toggle_button,
        clear_button,
        settings_button,
        volume_button,
        volume_label,
        speed_button,
        speed_label,
        zoom_button,
        zoom_label,
        waveform,
        timeline,
        position_label,
        duration_label,
        segments_list,
        segments_placeholder,
        segments_row,
        cut_revealer,
        apply_button,
        convert_button,
    } = panel;

    let page = AudioEditPage {
        toolbar,
        header,
        root,
        controls,
        trim_group,
        play_button,
        play_selection_button,
        auto_advance_button,
        volume_button,
        volume_label,
        speed_button,
        speed_label,
        waveform,
        timeline,
        position_label,
        duration_label,
        segments_list,
        segments_placeholder,
        segments_row,
        mpv: Rc::new(RefCell::new(None::<MpvBackend>)),
        duration: Rc::new(Cell::new(0.0)),
        position: Rc::new(Cell::new(0.0)),
        timeline_syncing: Rc::new(Cell::new(false)),
        suppress_auto_advance_after_manual_seek: Rc::new(Cell::new(false)),
        segments: Rc::new(RefCell::new(Vec::<AudioSegment>::new())),
        hover_segment: Rc::new(Cell::new(None)),
        hover_delete_button: Rc::new(Cell::new(false)),
        drag_mode: Rc::new(RefCell::new(DragMode::None)),
        pending_mark: Rc::new(Cell::new(None)),
        waveform_data: Rc::new(RefCell::new(None)),
        waveform_generation: Rc::new(Cell::new(0)),
        on_apply_trim: new_cb(),
        on_convert: new_cb(),
        on_show_audio_settings: new_cb(),
        on_show_equalizer: new_cb(),
        on_playback_state_changed: new_cb(),
        on_previous_track: new_cb(),
        on_next_track: new_cb(),
        cut_mode_active: Rc::new(Cell::new(false)),
        cut_revealer,
        zoom_label,
        zoom_level: Rc::new(Cell::new(1.0)),
    };

    AudioEditPageBuild {
        page,
        apply_button,
        convert_button,
        prev_button,
        next_button,
        clear_button,
        settings_button,
        eq_toggle_button,
        zoom_button,
    }
}
