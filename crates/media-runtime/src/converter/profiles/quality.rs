//! Quality / preset → ffmpeg-arg mapping for software encoders. AV1 and VP9
//! use numeric speed scales instead of named presets; CRF ranges
//! are tuned per-codec because the same number means different things to
//! libx264 vs libx265 vs libsvtav1 vs libvpx-vp9.

#[must_use]
pub fn quality_to_crf(codec: &str, quality: &str) -> u32 {
    match codec {
        "h264" => match quality {
            "veryhigh" => 16,
            "high" => 19,
            "medium" => 28,
            "low" => 33,
            "verylow" => 38,
            "superlow" => 45,
            _ => 23,
        },
        "h265" => match quality {
            "veryhigh" => 18,
            "high" => 22,
            "medium" => 30,
            "low" => 34,
            "verylow" => 40,
            "superlow" => 48,
            _ => 26,
        },
        "av1" => match quality {
            "veryhigh" => 20,
            "high" => 25,
            "medium" => 36,
            "low" => 42,
            "verylow" => 50,
            "superlow" => 58,
            _ => 30,
        },
        "vp9" => match quality {
            "veryhigh" => 18,
            "high" => 24,
            "medium" => 36,
            "low" => 42,
            "verylow" => 50,
            "superlow" => 58,
            _ => 30,
        },
        _ => 23,
    }
}

#[must_use]
pub fn preset_to_ffmpeg(codec: &str, preset: &str) -> &'static str {
    if codec == "vp9" {
        return match preset {
            "veryfast" => "5",
            "fast" => "4",
            "slightlyfast" => "3",
            "slow" => "1",
            "veryslow" => "0",
            _ => "2",
        };
    }
    if codec == "av1" {
        return match preset {
            "veryfast" => "8",
            "fast" => "6",
            "slightlyfast" => "5",
            "medium" => "3",
            "slow" => "2",
            "veryslow" => "1",
            _ => "4",
        };
    }

    match preset {
        "veryfast" => "veryfast",
        "fast" => "fast",
        "slightlyfast" => "faster",
        "slow" => "slow",
        "veryslow" => "veryslow",
        _ => "medium",
    }
}
