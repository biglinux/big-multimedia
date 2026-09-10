use std::sync::OnceLock;
use std::time::Duration;

use big_os_kit::subprocess::{BigSubprocessOutputMode, BigSubprocessSpec};

/// Check if ffmpeg is available.
pub fn ffmpeg_available() -> bool {
    BigSubprocessSpec::builder()
        .program("ffmpeg")
        .arg("-version")
        .stdout(BigSubprocessOutputMode::Null)
        .stderr(BigSubprocessOutputMode::Null)
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .is_ok_and(|o| o.status.success())
}

static AVAILABLE_ENCODERS: OnceLock<Vec<String>> = OnceLock::new();

fn available_encoders() -> &'static [String] {
    AVAILABLE_ENCODERS.get_or_init(|| {
        BigSubprocessSpec::builder()
            .program("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .stderr(BigSubprocessOutputMode::Null)
            .timeout(Duration::from_secs(10))
            .build()
            .run()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .filter_map(|line| {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 && parts[0].starts_with('V') {
                            Some(parts[1].to_string())
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    })
}

/// Check if a specific FFmpeg encoder is available.
pub fn ffmpeg_has_encoder(encoder: &str) -> bool {
    available_encoders().iter().any(|e| e == encoder)
}
