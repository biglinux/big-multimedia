//! The voice (noise reduction) strength slider both players show: heard at
//! once, written into the filter chain once the slider rests.

use crate::audio_effects;
use crate::player::mpv_backend::MpvBackend;
use crate::settings::{Settings, keys};
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
/// the chain from its text on every seek, loop and file, which would
/// otherwise restore the strength the text last held.
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
        big_gtk_kit::source::remove(source);
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
