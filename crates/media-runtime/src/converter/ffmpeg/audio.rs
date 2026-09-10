//! Audio filter graph: HPF / compressor / normalize → optional GTCRN
//! denoise (mono path inline; multi-channel via channelsplit + per-channel
//! LADSPA + amerge + pan because GTCRN is single-channel) → post-NR
//! gate / EQ / downmix / volume / atempo. Returns `(filter, use_filter_complex)`
//! so the command builder picks `-af` vs `-filter_complex` accordingly.

use big_audio_effects::dynamics::{ffmpeg_acompressor_filter, ffmpeg_agate_filter};
use big_audio_effects::noise_reduction::{NoiseModel, ffmpeg_ladspa_filter_for_model};

use super::*;

impl FfmpegJob {
    pub fn build_audio_filters(&self) -> (String, bool) {
        let audio = &self.profile.audio;
        if audio.handling == "copy" || audio.handling == "none" {
            return (String::new(), false);
        }

        let mut filters = Vec::new();
        let effects = &audio.effects;

        if effects.hpf_enabled && effects.hpf_frequency > 0 {
            filters.push(format!("highpass=f={}:poles=2", effects.hpf_frequency));
        }

        if effects.compressor_enabled {
            filters.push(ffmpeg_acompressor_filter(effects.compressor_intensity));
        }

        if effects.normalize {
            filters.push("speechnorm=e=12.5:r=0.0001:l=1".into());
        }

        let nr = &audio.noise_reduction;
        let nr_model = NoiseModel::from_i32_or_default(nr.model);
        let nr_controls = big_audio_effects::noise_reduction::GtcrnControls::converter(
            nr.strength,
            nr_model as i32,
            nr.speech_strength,
            nr.voice_recovery,
        );
        let nr_filter = if nr.enabled {
            ffmpeg_ladspa_filter_for_model(nr_model, &nr_controls)
        } else {
            None
        };
        let nr_enabled = nr_filter.is_some();
        let channels = self.input_audio_channels;

        if nr_enabled && channels > 1 {
            let ladspa = nr_filter.expect("nr_enabled implies filter");

            let layout = match channels {
                1 => "mono",
                3 => "3.0",
                4 => "quad",
                5 => "5.0",
                6 => "5.1",
                7 => "6.1",
                8 => "7.1",
                _ => "stereo",
            };

            if nr_model.is_attenuation_only() {
                filters.push("aresample=48000".into());
            }

            let pre_chain = if filters.is_empty() {
                String::new()
            } else {
                format!("{},", filters.join(","))
            };

            let mut graph = format!("[0:a]{pre_chain}channelsplit=channel_layout={layout}");
            for ch in 0..channels {
                graph.push_str(&format!("[ch{ch}]"));
            }
            graph.push(';');
            for ch in 0..channels {
                graph.push_str(&format!("[ch{ch}]{ladspa}[nr{ch}];"));
            }
            for ch in 0..channels {
                graph.push_str(&format!("[nr{ch}]"));
            }
            graph.push_str(&format!("amerge=inputs={channels}"));

            let pan = match channels {
                2 => "pan=stereo|FL=c0|FR=c1",
                3 => "pan=3.0|FL=c0|FR=c1|FC=c2",
                4 => "pan=quad|FL=c0|FR=c1|BL=c2|BR=c3",
                5 => "pan=5.0|FL=c0|FR=c1|FC=c2|BL=c3|BR=c4",
                6 => "pan=5.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5",
                7 => "pan=6.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5|BC=c6",
                8 => "pan=7.1|FL=c0|FR=c1|FC=c2|LFE=c3|BL=c4|BR=c5|SL=c6|SR=c7",
                _ => "",
            };
            if !pan.is_empty() {
                graph.push_str(&format!(",{pan}"));
            }

            let mut post = Vec::new();
            self.build_post_nr_filters(&mut post);
            if !post.is_empty() {
                graph.push_str(&format!(",{}", post.join(",")));
            }

            graph.push_str("[aout]");
            return (graph, true);
        }

        if let Some(filter) = nr_filter {
            if nr_model.is_attenuation_only() {
                filters.push("aresample=48000".into());
            }
            filters.push(filter);
        }

        self.build_post_nr_filters(&mut filters);
        (filters.join(","), false)
    }

    fn build_post_nr_filters(&self, filters: &mut Vec<String>) {
        let effects = &self.profile.audio.effects;

        if effects.gate_enabled {
            filters.push(ffmpeg_agate_filter(effects.gate_intensity));
        }

        if effects.eq_enabled && !effects.eq_bands.is_empty() {
            for (i, &gain) in effects.eq_bands.iter().enumerate() {
                if i >= EQ_BAND_FREQUENCIES_HZ.len() {
                    break;
                }
                if gain.abs() > 0.1 {
                    filters.push(format!(
                        "equalizer=f={}:width_type=o:w={CONVERTER_EQ_BANDWIDTH_OCTAVES}:g={gain:.1}",
                        EQ_BAND_FREQUENCIES_HZ[i]
                    ));
                }
            }
        }

        let audio = &self.profile.audio;
        if audio.channels != "auto" && !audio.channels.is_empty() {
            filters.push(format!("aformat=channel_layouts={}", audio.channels));
        }

        if (effects.volume - 1.0).abs() > 0.001 {
            filters.push(format!("volume={:.4}", effects.volume));
        }

        let speed = effects.speed;
        if (speed - 1.0).abs() > 0.001 {
            let mut remaining = speed;
            while remaining < 0.5 {
                filters.push("atempo=0.5".into());
                remaining /= 0.5;
            }
            while remaining > 100.0 {
                filters.push("atempo=100.0".into());
                remaining /= 100.0;
            }
            filters.push(format!("atempo={remaining:.4}"));
        }
    }
}
