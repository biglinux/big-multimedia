// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! Shared noise-reduction model picker.
//!
//! Apps provide availability from their runtime/package checks. This
//! component owns model ordering, labels, scope filtering, and dropdown UI.

use adw::prelude::*;
use big_audio_effects::noise_reduction::NoiseModel;
use relm4::gtk;

use crate::text::text;

/// Where the picker is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BigNoiseModelPickerScope {
    /// Microphone chains may use the bridged 16 kHz DPDFNet models.
    Microphone,
    /// Output/playback chains must stay on single-rate realtime models.
    Output,
}

/// Runtime availability for optional denoiser plugins.
///
/// Each field reports whether the matching LADSPA backend was found at
/// startup. Apps fill this from their own probe and pass the result to
/// [`noise_model_dropdown`] and [`noise_model_description`].
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BigNoiseModelAvailability {
    pub deepfilter: bool,
    pub dpdfnet_v2_hr: bool,
    pub dpdfnet_v8_hr: bool,
    pub dpdfnet_baseline_16k: bool,
    pub dpdfnet_v2_16k: bool,
    pub dpdfnet_v4_16k: bool,
    pub dpdfnet_v8_16k: bool,
}

impl BigNoiseModelAvailability {
    /// `true` if at least one HR (48 kHz) DPDFNet variant is installed.
    #[must_use]
    pub fn has_dpdfnet_hr(self) -> bool {
        self.dpdfnet_v2_hr || self.dpdfnet_v8_hr
    }

    /// `true` if at least one 16 kHz DPDFNet variant is installed.
    #[must_use]
    pub fn has_dpdfnet_16k(self) -> bool {
        self.dpdfnet_baseline_16k
            || self.dpdfnet_v2_16k
            || self.dpdfnet_v4_16k
            || self.dpdfnet_v8_16k
    }
}

/// One selectable model row paired with its translated label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigNoiseModelEntry {
    /// The model identifier passed to the audio pipeline.
    pub model: NoiseModel,
    /// Translated label shown to the user.
    pub label: String,
}

/// Build the model dropdown and wire selected model changes.
pub fn noise_model_dropdown<F>(
    initial: NoiseModel,
    scope: BigNoiseModelPickerScope,
    availability: BigNoiseModelAvailability,
    on_change: F,
) -> gtk::DropDown
where
    F: Fn(NoiseModel) + 'static,
{
    let entries = available_noise_model_entries(scope, availability);
    let labels: Vec<String> = entries.iter().map(|entry| entry.label.clone()).collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&label_refs);

    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let label = gtk::Label::builder().xalign(0.0).build();
        list_item.set_child(Some(&label));
    });
    factory.connect_bind(move |_, list_item| {
        let Some(list_item) = list_item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let Some(string) = list_item.item().and_downcast::<gtk::StringObject>() else {
            return;
        };
        let Some(label) = list_item.child().and_downcast::<gtk::Label>() else {
            return;
        };
        label.set_label(&string.string());
        list_item.set_selectable(true);
        list_item.set_activatable(true);
        label.remove_css_class("dim-label");
    });

    let dropdown = gtk::DropDown::new(Some(model), gtk::Expression::NONE);
    dropdown.set_factory(Some(&factory));
    dropdown.set_selected(noise_model_index(initial, &entries));

    let entries_for_change = entries.clone();
    dropdown.connect_selected_notify(move |dropdown| {
        if let Some(entry) = entries_for_change.get(dropdown.selected() as usize) {
            on_change(entry.model);
        }
    });

    dropdown
}

/// Description that matches the actually visible model set.
#[must_use]
pub fn noise_model_description(
    scope: BigNoiseModelPickerScope,
    availability: BigNoiseModelAvailability,
) -> String {
    let has_16k = scope == BigNoiseModelPickerScope::Microphone && availability.has_dpdfnet_16k();
    if has_16k {
        return text(
            "DNS3 is a lightweight 16 kHz model that removes steady noise. \
             DeepFilterNet3 and DPDFNet HR are full-band 48 kHz models. \
             The lighter 16 kHz DPDFNet variants run inside a dedicated \
             resampled chain — strong noise reduction at a fraction of \
             the CPU.",
        );
    }

    match (availability.deepfilter, availability.has_dpdfnet_hr()) {
        (false, false) => text(
            "DNS3 is a lightweight 16 kHz model that removes steady noise — \
             good for podcasts.",
        ),
        (true, false) => text(
            "DNS3 is a lightweight 16 kHz model that removes steady noise. \
             DeepFilterNet3 is a full-band 48 kHz model with the cleanest \
             output and the highest CPU cost.",
        ),
        (false, true) => text(
            "DNS3 is a lightweight 16 kHz model that removes steady noise. \
             DPDFNet HR variants are full-band 48 kHz models with strong \
             noise reduction at a higher CPU cost.",
        ),
        (true, true) => text(
            "DNS3 is a lightweight 16 kHz model that removes steady noise. \
             DeepFilterNet3 and the DPDFNet HR variants are full-band \
             48 kHz models — the cleanest output, with the highest CPU \
             cost.",
        ),
    }
}

/// Available entries for a given app scope.
#[must_use]
pub fn available_noise_model_entries(
    scope: BigNoiseModelPickerScope,
    availability: BigNoiseModelAvailability,
) -> Vec<BigNoiseModelEntry> {
    let mut entries = vec![BigNoiseModelEntry {
        model: NoiseModel::GtcrnDns3,
        label: text("GTCRN – DNS3 (16 kHz, strong)"),
    }];

    if availability.deepfilter {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DeepFilterNet3,
            label: text("DeepFilterNet3 (48 kHz)"),
        });
    }
    if availability.dpdfnet_v2_hr {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetV2Hr,
            label: text("DPDFNet-2 HR (48 kHz, balanced)"),
        });
    }
    if availability.dpdfnet_v8_hr {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetV8Hr,
            label: text("DPDFNet-8 HR (48 kHz, premium)"),
        });
    }

    if scope == BigNoiseModelPickerScope::Microphone {
        push_dpdfnet_16k_entries(&mut entries, availability);
    }

    entries
}

/// Return the dropdown index of `model` within `entries`, or `0` if not found.
#[must_use]
pub fn noise_model_index(model: NoiseModel, entries: &[BigNoiseModelEntry]) -> u32 {
    entries
        .iter()
        .position(|entry| entry.model == model)
        .map_or(0, |position| u32::try_from(position).unwrap_or(0))
}

fn push_dpdfnet_16k_entries(
    entries: &mut Vec<BigNoiseModelEntry>,
    availability: BigNoiseModelAvailability,
) {
    if availability.dpdfnet_baseline_16k {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetBaseline,
            label: text("DPDFNet baseline (16 kHz, lightest)"),
        });
    }
    if availability.dpdfnet_v2_16k {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetV2,
            label: text("DPDFNet-2 (16 kHz, balanced)"),
        });
    }
    if availability.dpdfnet_v4_16k {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetV4,
            label: text("DPDFNet-4 (16 kHz, stronger)"),
        });
    }
    if availability.dpdfnet_v8_16k {
        entries.push(BigNoiseModelEntry {
            model: NoiseModel::DpdfnetV8,
            label: text("DPDFNet-8 (16 kHz, strongest)"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn availability_with_high_resolution_dpdfnet_variant(
        model: NoiseModel,
    ) -> BigNoiseModelAvailability {
        match model {
            NoiseModel::DpdfnetV2Hr => BigNoiseModelAvailability {
                dpdfnet_v2_hr: true,
                ..BigNoiseModelAvailability::default()
            },
            NoiseModel::DpdfnetV8Hr => BigNoiseModelAvailability {
                dpdfnet_v8_hr: true,
                ..BigNoiseModelAvailability::default()
            },
            other => panic!("unexpected high-resolution DPDFNet variant: {other:?}"),
        }
    }

    fn availability_with_sixteen_kilohertz_dpdfnet_variant(
        model: NoiseModel,
    ) -> BigNoiseModelAvailability {
        match model {
            NoiseModel::DpdfnetBaseline => BigNoiseModelAvailability {
                dpdfnet_baseline_16k: true,
                ..BigNoiseModelAvailability::default()
            },
            NoiseModel::DpdfnetV2 => BigNoiseModelAvailability {
                dpdfnet_v2_16k: true,
                ..BigNoiseModelAvailability::default()
            },
            NoiseModel::DpdfnetV4 => BigNoiseModelAvailability {
                dpdfnet_v4_16k: true,
                ..BigNoiseModelAvailability::default()
            },
            NoiseModel::DpdfnetV8 => BigNoiseModelAvailability {
                dpdfnet_v8_16k: true,
                ..BigNoiseModelAvailability::default()
            },
            other => panic!("unexpected 16 kHz DPDFNet variant: {other:?}"),
        }
    }

    #[test]
    fn availability_reports_any_high_resolution_dpdfnet_variant() {
        assert!(!BigNoiseModelAvailability::default().has_dpdfnet_hr());

        for model in [NoiseModel::DpdfnetV2Hr, NoiseModel::DpdfnetV8Hr] {
            assert!(
                availability_with_high_resolution_dpdfnet_variant(model).has_dpdfnet_hr(),
                "{model:?} must enable high-resolution DPDFNet availability",
            );
        }
    }

    #[test]
    fn availability_reports_any_sixteen_kilohertz_dpdfnet_variant() {
        assert!(!BigNoiseModelAvailability::default().has_dpdfnet_16k());

        for model in [
            NoiseModel::DpdfnetBaseline,
            NoiseModel::DpdfnetV2,
            NoiseModel::DpdfnetV4,
            NoiseModel::DpdfnetV8,
        ] {
            assert!(
                availability_with_sixteen_kilohertz_dpdfnet_variant(model).has_dpdfnet_16k(),
                "{model:?} must enable 16 kHz DPDFNet availability",
            );
        }
    }

    #[test]
    fn gtcrn_dns3_is_always_present() {
        for scope in [
            BigNoiseModelPickerScope::Microphone,
            BigNoiseModelPickerScope::Output,
        ] {
            let entries =
                available_noise_model_entries(scope, BigNoiseModelAvailability::default());
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.model == NoiseModel::GtcrnDns3)
            );
            assert!(
                entries
                    .iter()
                    .all(|entry| entry.model != NoiseModel::GtcrnVctk),
                "VCTK must not appear in picker",
            );
        }
    }

    #[test]
    fn output_scope_excludes_sixteen_kilohertz_dpdfnet_variants() {
        let availability = BigNoiseModelAvailability {
            dpdfnet_baseline_16k: true,
            dpdfnet_v2_16k: true,
            dpdfnet_v4_16k: true,
            dpdfnet_v8_16k: true,
            ..BigNoiseModelAvailability::default()
        };
        let entries = available_noise_model_entries(BigNoiseModelPickerScope::Output, availability);

        for model in [
            NoiseModel::DpdfnetBaseline,
            NoiseModel::DpdfnetV2,
            NoiseModel::DpdfnetV4,
            NoiseModel::DpdfnetV8,
        ] {
            assert!(
                entries.iter().all(|entry| entry.model != model),
                "output picker must not list {model:?}",
            );
        }
    }

    #[test]
    fn microphone_scope_lists_installed_sixteen_kilohertz_variants() {
        let availability = BigNoiseModelAvailability {
            dpdfnet_v4_16k: true,
            ..BigNoiseModelAvailability::default()
        };
        let entries =
            available_noise_model_entries(BigNoiseModelPickerScope::Microphone, availability);

        assert!(
            entries
                .iter()
                .any(|entry| entry.model == NoiseModel::DpdfnetV4)
        );
    }

    #[test]
    fn model_index_falls_back_to_zero_for_missing_model() {
        let entries = vec![
            BigNoiseModelEntry {
                model: NoiseModel::GtcrnDns3,
                label: "a".into(),
            },
            BigNoiseModelEntry {
                model: NoiseModel::DeepFilterNet3,
                label: "b".into(),
            },
        ];

        assert_eq!(noise_model_index(NoiseModel::DpdfnetV2Hr, &entries), 0);
        assert_eq!(noise_model_index(NoiseModel::DeepFilterNet3, &entries), 1);
    }

    #[test]
    fn description_for_default_availability_mentions_only_dns3() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Output,
            BigNoiseModelAvailability::default(),
        );

        assert!(description.contains("DNS3 is a lightweight 16 kHz model"));
        assert!(description.contains("good for podcasts"));
        assert!(!description.contains("DeepFilterNet3"));
        assert!(!description.contains("DPDFNet HR"));
        assert!(!description.contains("dedicated resampled chain"));
    }

    #[test]
    fn description_for_deepfilter_availability_mentions_cpu_cost() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Output,
            BigNoiseModelAvailability {
                deepfilter: true,
                ..BigNoiseModelAvailability::default()
            },
        );

        assert!(description.contains("DeepFilterNet3 is a full-band 48 kHz model"));
        assert!(description.contains("highest CPU cost"));
        assert!(!description.contains("DPDFNet HR variants"));
        assert!(!description.contains("dedicated resampled chain"));
    }

    #[test]
    fn description_for_high_resolution_dpdfnet_availability_mentions_strong_reduction() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Output,
            availability_with_high_resolution_dpdfnet_variant(NoiseModel::DpdfnetV2Hr),
        );

        assert!(description.contains("DPDFNet HR variants"));
        assert!(description.contains("strong noise reduction"));
        assert!(description.contains("higher CPU cost"));
        assert!(!description.contains("DeepFilterNet3 is"));
        assert!(!description.contains("dedicated resampled chain"));
    }

    #[test]
    fn description_for_deepfilter_and_high_resolution_dpdfnet_mentions_both_families() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Output,
            BigNoiseModelAvailability {
                deepfilter: true,
                dpdfnet_v8_hr: true,
                ..BigNoiseModelAvailability::default()
            },
        );

        assert!(description.contains("DeepFilterNet3 and the DPDFNet HR variants"));
        assert!(description.contains("full-band"));
        assert!(description.contains("48 kHz"));
        assert!(description.contains("highest CPU"));
        assert!(!description.contains("dedicated resampled chain"));
    }

    #[test]
    fn description_for_microphone_sixteen_kilohertz_variants_mentions_resampled_chain() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Microphone,
            availability_with_sixteen_kilohertz_dpdfnet_variant(NoiseModel::DpdfnetV4),
        );

        assert!(description.contains("lighter 16 kHz DPDFNet variants"));
        assert!(description.contains("dedicated"));
        assert!(description.contains("resampled chain"));
        assert!(description.contains("fraction of the CPU"));
    }

    #[test]
    fn description_ignores_sixteen_kilohertz_variants_outside_microphone_scope() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Output,
            availability_with_sixteen_kilohertz_dpdfnet_variant(NoiseModel::DpdfnetV8),
        );

        assert!(description.contains("good for podcasts"));
        assert!(!description.contains("dedicated resampled chain"));
        assert!(!description.contains("fraction of the CPU"));
    }

    #[test]
    fn microphone_description_without_sixteen_kilohertz_variants_uses_visible_model_set() {
        let description = noise_model_description(
            BigNoiseModelPickerScope::Microphone,
            BigNoiseModelAvailability {
                deepfilter: true,
                ..BigNoiseModelAvailability::default()
            },
        );

        assert!(description.contains("DeepFilterNet3 is a full-band 48 kHz model"));
        assert!(!description.contains("dedicated resampled chain"));
        assert!(!description.contains("fraction of the CPU"));
    }
}
