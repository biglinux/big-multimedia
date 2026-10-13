//! App-local Relm4 video filters/shaders dialog.
//!
//! Aggregates every per-feature filter group onto one `adw::PreferencesPage`:
//! denoise/sharpen + color + style presets + effects (software `vf` chain) and
//! GPU processing + upscaling shaders + neural + Anime4K (mpv render properties
//! / GLSL shaders). Controls drive the shell's live `Arc<MpvBackend>` directly;
//! shader/upscaler choices persist through `Settings`. Filter-string and
//! shader-percentage math is the testable `filter_specs` module.
//!
//! Self-contained: the dialog owns no playback state and emits no intents; it
//! mutates the mpv filter chain in place. The shell
//! launches it `.detach()`ed and presents its widget in an `adw::Dialog`.

mod filter_specs;
mod groups_shader;
mod groups_vf;
mod vf_row;

pub(crate) use groups_shader::{apply_saved_enhancements, saved_enhancements};

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
}

/// The filters dialog has no inbound messages: every control wires itself to
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

        page.add(&groups_shader::build_gpu_group(&mpv));
        page.add(&groups_shader::build_shaders_group(&mpv, &settings));
        page.add(&groups_shader::build_neural_group(&mpv, &settings));
        page.add(&groups_shader::build_anime4k_group(&mpv, &settings));
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

#[cfg(test)]
mod lifetime_tests {
    use super::*;

    #[test]
    #[ignore = "requires an isolated GTK session"]
    fn filter_controls_release_the_backend() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct Finalized(Rc<Cell<usize>>);
        impl Drop for Finalized {
            fn drop(&mut self) {
                self.0.set(self.0.get() + 1);
            }
        }
        gtk::init().unwrap();
        adw::init().unwrap();
        let (sender, _receiver) = async_channel::unbounded();
        let mpv = Arc::new(MpvBackend::new_audio_only(sender).unwrap());
        let finalized = Rc::new(Cell::new(0));
        let mut created = 0;
        for _ in 0..25 {
            let presets = groups_vf::build_presets_group(&mpv, "");
            let adjustments = groups_vf::build_denoise_sharpen_group(&mpv, "");
            let colors = groups_vf::build_color_group(&mpv, "");
            let mut widgets = vec![
                colors.clone().upcast(),
                presets.clone().upcast::<gtk::Widget>(),
                adjustments.clone().upcast(),
            ];
            while let Some(widget) = widgets.pop() {
                let object = if let Some(row) = widget.downcast_ref::<adw::SpinRow>() {
                    Some(row.adjustment().upcast::<gtk::glib::Object>())
                } else {
                    widget
                        .downcast_ref::<gtk::DropDown>()
                        .map(|dropdown| dropdown.clone().upcast())
                };
                if let Some(object) = object {
                    // SAFETY: the test owns this key and never accesses its stored data.
                    unsafe {
                        object.set_qdata(
                            gtk::glib::Quark::from_str("filter-finalization"),
                            Finalized(finalized.clone()),
                        );
                    }
                    created += 1;
                }
                let mut child = widget.first_child();
                while let Some(current) = child {
                    child = current.next_sibling();
                    widgets.push(current);
                }
            }
            drop(presets);
            drop(adjustments);
            drop(colors);
            assert_eq!(Arc::strong_count(&mpv), 1, "filter handlers retained mpv");
        }
        assert_eq!(created, 200, "seven adjustments and one dropdown per cycle");
        assert_eq!(finalized.get(), created, "filter control finalizations");
    }
}
