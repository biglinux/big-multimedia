//! App-local Relm4 video filters/shaders dialog (video port, V5).
//!
//! Aggregates every per-feature filter group onto one `adw::PreferencesPage`:
//! denoise/sharpen + color + style presets + effects (software `vf` chain) and
//! GPU processing + upscaling shaders + neural + Anime4K (mpv render properties
//! / GLSL shaders). Controls drive the shell's live `Arc<MpvBackend>` directly;
//! shader/upscaler choices persist through `Settings`. Filter-string and
//! shader-percentage math is the testable `filter_specs` module.
//!
//! Self-contained: the dialog owns no playback state and emits no intents — it
//! mutates the mpv filter chain in place (the legacy did the same). The shell
//! launches it `.detach()`ed and presents its widget in an `adw::Dialog`.

mod filter_specs;
mod groups_shader;
mod groups_vf;
mod vf_row;

use std::rc::Rc;
use std::sync::Arc;

use adw::prelude::*;
use relm4::{Component, ComponentParts, ComponentSender};

use crate::player::mpv_backend::MpvBackend;
use crate::settings::Settings;

/// Init payload: the shell's live mpv backend + the player settings store.
pub struct VideoFiltersDialogInit {
    /// Live mpv backend (the filters mutate its `vf` chain + render props).
    pub mpv: Arc<MpvBackend>,
    /// Player settings (shader/upscaler choices persist here).
    pub settings: Settings,
    /// Called after render/shader filter changes so active cast sessions can
    /// restart transcoding with a fresh snapshot.
    pub on_filter_changed: Rc<dyn Fn()>,
}

/// The filters dialog has no inbound messages — every control wires itself to
/// mpv on construction.
#[derive(Debug)]
pub enum VideoFiltersDialogInput {}

/// Relm4 video filters/shaders dialog.
pub struct VideoFiltersDialog;

impl Component for VideoFiltersDialog {
    type Init = VideoFiltersDialogInit;
    type Input = VideoFiltersDialogInput;
    type Output = ();
    type CommandOutput = ();
    type Root = adw::PreferencesPage;
    type Widgets = ();

    fn init_root() -> Self::Root {
        adw::PreferencesPage::new()
    }

    fn init(
        init: Self::Init,
        page: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let mpv = init.mpv;
        let settings = init.settings;
        let current_vf = mpv.get_vf();

        page.add(&groups_vf::build_denoise_sharpen_group(&mpv, &current_vf));
        let on_filter_changed = init.on_filter_changed;

        page.add(&groups_shader::build_gpu_group(&mpv, &on_filter_changed));
        page.add(&groups_shader::build_shaders_group(
            &mpv,
            &settings,
            &on_filter_changed,
        ));
        page.add(&groups_shader::build_neural_group(
            &mpv,
            &settings,
            &on_filter_changed,
        ));
        page.add(&groups_shader::build_anime4k_group(
            &mpv,
            &settings,
            &on_filter_changed,
        ));
        page.add(&groups_vf::build_color_group(&mpv, &current_vf));
        page.add(&groups_vf::build_presets_group(&mpv, &current_vf));
        page.add(&groups_vf::build_effects_group(&mpv, &current_vf));

        ComponentParts {
            model: Self,
            widgets: (),
        }
    }

    fn update(
        &mut self,
        _message: Self::Input,
        _sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
    }
}
