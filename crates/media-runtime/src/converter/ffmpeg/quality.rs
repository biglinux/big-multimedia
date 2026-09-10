//! Per-encoder quality args. Each vendor has its own knobs (NVENC: cq +
//! lookahead + temporal-aq; QSV: global_quality + adaptive_b + low_delay;
//! VAAPI: CQP + global_quality; software: codec-specific CRF). H.264 high
//! profile + level 4.1 forced on QSV/VAAPI for broad device compatibility.

use super::*;

impl FfmpegJob {
    pub(crate) fn add_nvidia_quality_args(
        cmd: &mut Command,
        codec: &str,
        quality: &str,
        preset: &str,
    ) {
        let cq = nvidia_cq(codec, quality);
        let nvidia_preset = nvidia_preset_num(preset);

        cmd.args(["-rc:v", "vbr"]);
        cmd.args(["-cq", &cq.to_string()]);
        cmd.args(["-tune:v", "hq"]);
        cmd.args(["-preset:v", &format!("p{nvidia_preset}")]);
        cmd.args(["-temporal-aq", "1"]);
        cmd.args(["-rc-lookahead", "100"]);

        if codec == "h265" {
            cmd.args(["-spatial-aq", "1"]);
            cmd.args(["-b_ref_mode", "middle"]);
        }
    }

    pub(crate) fn add_intel_quality_args(
        cmd: &mut Command,
        codec: &str,
        quality: &str,
        preset: &str,
    ) {
        let gq = intel_global_quality(codec, quality);
        let ffmpeg_preset = profiles::preset_to_ffmpeg(codec, preset);

        cmd.args(["-preset", ffmpeg_preset]);
        cmd.args(["-look_ahead_depth", "100"]);
        cmd.args(["-low_delay_brc", "1"]);
        cmd.args(["-extbrc", "1"]);
        cmd.args(["-global_quality", &gq.to_string()]);
        cmd.args(["-mbbrc", "1"]);
        cmd.args(["-adaptive_i", "1"]);
        cmd.args(["-adaptive_b", "1"]);

        if codec == "h264" {
            cmd.args(["-profile:v", "high"]);
            cmd.args(["-level:v", "4.1"]);
        }
    }

    pub(crate) fn add_vaapi_quality_args(cmd: &mut Command, codec: &str, quality: &str) {
        let gq = vaapi_global_quality(codec, quality);

        cmd.args(["-rc_mode", "CQP"]);
        cmd.args(["-global_quality", &gq.to_string()]);

        if codec == "h264" {
            cmd.args(["-profile:v", "high"]);
            cmd.args(["-level:v", "4.1"]);
        }
    }

    pub(crate) fn add_software_quality_args(
        cmd: &mut Command,
        codec: &str,
        quality: &str,
        preset: &str,
    ) {
        let crf = profiles::quality_to_crf(codec, quality);
        let ffmpeg_preset = profiles::preset_to_ffmpeg(codec, preset);

        match codec {
            "h264" => {
                cmd.args(["-preset", ffmpeg_preset]);
                cmd.args(["-profile:v", "high"]);
                cmd.args(["-level:v", "4.1"]);
                cmd.args(["-crf", &crf.to_string()]);
            }
            "h265" => {
                cmd.args(["-preset", ffmpeg_preset]);
                cmd.args(["-crf", &crf.to_string()]);
                cmd.args(["-x265-params", "log-level=warning"]);
            }
            "av1" => {
                cmd.args(["-preset", ffmpeg_preset]);
                cmd.args(["-crf", &crf.to_string()]);
                cmd.args(["-svtav1-params", "tune=0"]);
            }
            "vp9" => {
                cmd.args(["-crf", &crf.to_string()]);
                cmd.args(["-b:v", "0"]);
                cmd.args(["-quality", "good"]);
                cmd.args(["-cpu-used", ffmpeg_preset]);
                cmd.args(["-row-mt", "1"]);
            }
            "prores" => {
                let prores_profile = prores_profile_num(quality);
                cmd.args(["-profile:v", &prores_profile.to_string()]);
                cmd.args(["-vendor", "apl0"]);
                cmd.args(["-pix_fmt", "yuva444p10le"]);
            }
            _ => {
                cmd.args(["-crf", &crf.to_string()]);
            }
        }
    }
}
