//! Advanced preferences groups: general playback (resume + repeat) and
//! end-of-file behavior (keep-open / pause-at-end / idle mode). The latter
//! two settings are gated by `is_advanced` and self-register in `adv_widgets`.

use adw::prelude::*;
use big_relm4_components::layout::preferences::track_widget_visibility;
use std::sync::Arc;

use crate::i18n::i18n;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{keys, Settings};

use super::super::AdvancedWidgets;
use super::super::{add_combo_row, add_switch_row};
use super::common::illustration;

pub(super) fn build_adv_playback_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
) -> adw::PreferencesGroup {
    let playback = adw::PreferencesGroup::builder()
        .title(i18n("Playback"))
        .description(i18n(
            "General playback behavior such as resuming and looping",
        ))
        .build();

    let m = mpv.cloned();
    let resume_row = add_switch_row(
        &playback,
        &i18n("Resume Playback"),
        &i18n("Remember position when reopening files"),
        "resume-playback",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_save_position(v);
            }
        },
    );
    resume_row.add_prefix(&illustration("resume.svg"));

    let loop_labels = &[i18n("Off"), i18n("Repeat file"), i18n("Repeat playlist")];
    let loop_options = gtk::StringList::new(
        &loop_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
    );
    let loop_row = adw::ComboRow::builder()
        .title(i18n("Repeat"))
        .subtitle(i18n("Repeat the current file or the entire playlist"))
        .model(&loop_options)
        .build();
    let loop_values: &[&str] = &["no", "file", "playlist"];
    let current_loop = settings.get_string(keys::LOOP_MODE);
    let loop_idx = loop_values
        .iter()
        .position(|v| *v == current_loop.as_str())
        .unwrap_or(0);
    loop_row.set_selected(loop_idx as u32);
    {
        let s = settings.clone();
        let m = mpv.cloned();
        loop_row.connect_selected_notify(move |row| {
            let selected = row.selected() as usize;
            if let Some(val) = loop_values.get(selected) {
                let _ = s.put_string(keys::LOOP_MODE, val);
                if let Some(ref mpv) = m {
                    match *val {
                        "file" => {
                            mpv.set_loop_file(true);
                            mpv.set_loop_playlist(false);
                        }
                        "playlist" => {
                            mpv.set_loop_file(false);
                            mpv.set_loop_playlist(true);
                        }
                        _ => {
                            mpv.set_loop_file(false);
                            mpv.set_loop_playlist(false);
                        }
                    }
                }
            }
        });
    }
    playback.add(&loop_row);
    loop_row.add_prefix(&illustration("repeat.svg"));

    playback
}

pub(super) fn build_adv_eof_group(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    is_advanced: bool,
    adv_widgets: &AdvancedWidgets,
) -> adw::PreferencesGroup {
    let eof_group = adw::PreferencesGroup::builder()
        .title(i18n("When Video Ends"))
        .description(i18n(
            "Choose what happens when the file or playlist finishes playing",
        ))
        .build();

    let m = mpv.cloned();
    let keep_open_labels = &[
        i18n("Keep last frame"),
        i18n("Always keep window"),
        i18n("Close window"),
    ];
    let eof_row = add_combo_row(
        &eof_group,
        &i18n("Window behavior"),
        &i18n("What to do when the video finishes"),
        &keep_open_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["yes", "always", "no"],
        "keep-open",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_keep_open(v);
            }
        },
    );
    eof_row.add_prefix(&illustration("eof.svg"));

    let m = mpv.cloned();
    let pause_end_row = add_switch_row(
        &eof_group,
        &i18n("Pause at End"),
        &i18n("Pause when reaching end of file"),
        "keep-open-pause",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_keep_open_pause(v);
            }
        },
    );
    pause_end_row.add_prefix(&illustration("pause.svg"));
    track_widget_visibility(&pause_end_row, is_advanced, adv_widgets);

    let idle_labels = &[
        i18n("Stay open"),
        i18n("Close player"),
        i18n("Close after first file"),
    ];
    let m = mpv.cloned();
    let idle_row = add_combo_row(
        &eof_group,
        &i18n("Idle Mode"),
        &i18n("What happens after the playlist finishes"),
        &idle_labels
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<_>>(),
        &["yes", "no", "once"],
        "idle-mode",
        settings,
        move |v| {
            if let Some(ref mpv) = m {
                mpv.set_idle(v);
            }
        },
    );
    idle_row.add_prefix(&illustration("idle.svg"));
    track_widget_visibility(&idle_row, is_advanced, adv_widgets);

    eof_group
}
