//! Hand files to the BigLinux converter GUIs. The players do no conversion of
//! their own: cuts marked in a player travel as `--segments=START-END,...`
//! (seconds), which both GUIs read into their queue before the user converts,
//! and the video converter takes `--source-hdr=pq|hlg|sdr` for a file whose
//! colours the user told the player how to read.

use big_os_kit::subprocess::BigSubprocessSpec;

use crate::i18n::i18n;

/// Video converter GUI. Plain `big-video-converter` is the headless CLI, which
/// would start converting at once with default settings.
pub const VIDEO_CONVERTER: &str = "big-video-converter-gui";
/// Audio converter GUI; it also takes video files and keeps only their audio.
pub const AUDIO_CONVERTER: &str = "big-audio-converter-gui";

/// True when `program` is on `PATH`.
pub fn is_installed(program: &str) -> bool {
    gtk::glib::find_program_in_path(program).is_some()
}

/// Open `paths` in the converter `program`, with the cuts to keep and how
/// to read their colours, if any. Returns what to tell the person when the
/// converter could not be started.
pub fn open(
    program: &str,
    paths: &[String],
    cuts: &[(f64, f64)],
    source_hdr: Option<&str>,
) -> Option<String> {
    let spawned = BigSubprocessSpec::builder()
        .program(program)
        .args(converter_args(paths, cuts, source_hdr))
        .build()
        .spawn_detached();
    match spawned {
        // The converter's own window is the answer.
        Ok(()) => None,
        Err(error) => {
            log::warn!("could not launch {program}: {error}");
            Some(i18n("Could not launch the converter"))
        }
    }
}

fn converter_args(paths: &[String], cuts: &[(f64, f64)], source_hdr: Option<&str>) -> Vec<String> {
    let mut args = Vec::with_capacity(paths.len() + 3);
    if !cuts.is_empty() {
        let ranges: Vec<String> = cuts
            .iter()
            // Exact shortest form: rounding could push an end-of-file cut
            // past the duration the converter measures.
            .map(|(start, end)| format!("{start}-{end}"))
            .collect();
        args.push(format!("--segments={}", ranges.join(",")));
    }
    if let Some(mode) = source_hdr {
        args.push(format!("--source-hdr={mode}"));
    }
    // A file name starting with `-` must not read as an option.
    args.push("--".to_string());
    args.extend(paths.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::converter_args;

    #[test]
    fn cuts_precede_the_end_of_options_marker() {
        let paths = vec!["/v/a.mkv".to_string(), "-dash.mkv".to_string()];
        assert_eq!(
            converter_args(&paths, &[(1.5, 3.0), (10.0, 12.345_678_9)], Some("pq")),
            [
                "--segments=1.5-3,10-12.3456789",
                "--source-hdr=pq",
                "--",
                "/v/a.mkv",
                "-dash.mkv"
            ]
        );
        assert_eq!(
            converter_args(&paths, &[], None),
            ["--", "/v/a.mkv", "-dash.mkv"]
        );
    }
}
