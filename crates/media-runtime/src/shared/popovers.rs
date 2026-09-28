//! Player popover builders re-exported from `big-media-components::player_chrome`
//! (pure-gtk, reusable). The mpv/settings-bound helpers below stay here — they
//! are engine-coupled (`MpvBackend`, audio-effects, settings).
//! bigagents: app-local-adapter; remaining helpers are mpv/settings-bound.

pub use big_media_components::player_chrome::{
    SPEED_PRESETS, VOICE_PRESETS, build_speed_popover, build_voice_popover, build_volume_popover,
    setup_exclusive_hover_popover_group, setup_hover_controller_exclusive,
};

use gtk::prelude::*;

use crate::player::mpv_backend::MpvBackend;
use crate::settings::{self, Settings, keys};
use crate::shared::audio_effects;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use big_audio_effects::noise_reduction::NoiseModel;

/// Voice slider state kept by each player between slider events.
pub struct VoiceStrength {
    active: Cell<bool>,
    rebuild: RefCell<Option<gtk::glib::SourceId>>,
}

impl VoiceStrength {
    #[must_use]
    pub fn new(active: bool) -> Self {
        Self {
            active: Cell::new(active),
            rebuild: RefCell::new(None),
        }
    }
}

/// How long the slider rests before the chain is rebuilt with its value.
const REBUILD_AFTER_SLIDER_REST: Duration = Duration::from_millis(600);

/// Persists strength and applies it. Turning the filter on or off rebuilds
/// the chain. A change while it runs is heard at once through a live
/// command, then written into the chain once the slider rests: mpv rebuilds
/// the chain from its text on every seek, loop and file, and without that
/// the filter fell back to the strength of the first slider step.
/// Returns the new `is_active`.
pub fn handle_voice_strength_changed(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    pct: f64,
    state: &Rc<VoiceStrength>,
) -> bool {
    let strength = pct / 100.0;
    let _ = settings.put_double(keys::NOISE_STRENGTH, strength);

    let is_active = strength > 0.0;
    let was_active = state.active.replace(is_active);
    if let Some(source) = state.rebuild.take() {
        source.remove();
    }
    let Some(mpv) = mpv else {
        return is_active;
    };
    if was_active != is_active {
        audio_effects::apply_voice_change(settings, mpv);
    } else if is_active {
        let model = NoiseModel::from_i32_or_default(settings.get_int(keys::NOISE_MODEL));
        mpv.set_noise_strength(model, strength);
        let settings = settings.clone();
        let mpv = Arc::downgrade(mpv);
        let pending = Rc::downgrade(state);
        let source = gtk::glib::timeout_add_local_once(REBUILD_AFTER_SLIDER_REST, move || {
            if let Some(state) = pending.upgrade() {
                state.rebuild.take();
            }
            if let Some(mpv) = mpv.upgrade() {
                audio_effects::apply_voice_change(&settings, &mpv);
            }
        });
        state.rebuild.replace(Some(source));
    }
    is_active
}

pub fn show_nr_settings_dialog(
    settings: &Settings,
    mpv: Option<&Arc<MpvBackend>>,
    parent: &impl gtk::prelude::IsA<gtk::Window>,
) {
    let dialog = audio_effects::create_nr_settings_dialog(settings, mpv);
    dialog.set_transient_for(Some(parent));
    settings::bind_dialog_size_persistence(&dialog, settings, "nr-settings", 840, 580);
    dialog.present();
}
