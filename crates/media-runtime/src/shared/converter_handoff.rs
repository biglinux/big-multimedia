//! Hand files to the BigLinux converter GUIs. The players do no conversion of
//! their own: cuts marked in a player travel as `--segments=START-END,...`
//! (seconds), which both GUIs read into their queue before the user converts.

use big_os_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

/// Video converter GUI. Plain `big-video-converter` is the headless CLI, which
/// would start converting at once with default settings.
pub const VIDEO_CONVERTER: &str = "big-video-converter-gui";
/// Audio converter GUI; it also takes video files and keeps only their audio.
pub const AUDIO_CONVERTER: &str = "big-audio-converter-gui";

/// True when `program` is on `PATH`.
pub fn is_installed(program: &str) -> bool {
    gtk::glib::find_program_in_path(program).is_some()
}

/// Open `paths` in the converter `program`, with the cuts to keep, if any.
pub fn open(
    program: &str,
    paths: &[String],
    cuts: &[(f64, f64)],
) -> Result<(), BigSubprocessError> {
    BigSubprocessSpec::builder()
        .program(program)
        .args(converter_args(paths, cuts))
        .build()
        .spawn_detached()
}

fn converter_args(paths: &[String], cuts: &[(f64, f64)]) -> Vec<String> {
    let mut args = Vec::with_capacity(paths.len() + 2);
    if !cuts.is_empty() {
        let ranges: Vec<String> = cuts
            .iter()
            .map(|(start, end)| format!("{start:.3}-{end:.3}"))
            .collect();
        args.push(format!("--segments={}", ranges.join(",")));
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
            converter_args(&paths, &[(1.5, 3.0), (10.0, 12.25)]),
            [
                "--segments=1.500-3.000,10.000-12.250",
                "--",
                "/v/a.mkv",
                "-dash.mkv"
            ]
        );
        assert_eq!(converter_args(&paths, &[]), ["--", "/v/a.mkv", "-dash.mkv"]);
    }
}
