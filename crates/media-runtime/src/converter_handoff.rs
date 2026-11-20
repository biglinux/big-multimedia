//! Hand files to the BigLinux converter GUIs. The players do no conversion of
//! their own: cuts marked in a player travel as `--segments=START-END,...`
//! (seconds), which both GUIs read into their queue before the user converts,
//! and the video converter takes `--source-hdr=pq|hlg|sdr` for a file whose
//! colors the user told the player how to read.

use gdk::prelude::*;

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
/// to read their colors, if any. Returns what to tell the person when the
/// converter could not be started.
pub fn open(
    program: &str,
    paths: &[String],
    cuts: &[(f64, f64)],
    source_hdr: Option<&str>,
) -> Option<String> {
    // The launch context hands the converter an activation token, so the
    // compositor focuses its window.
    let context = gdk::Display::default().map(|display| display.app_launch_context());
    match launch(program, paths, cuts, source_hdr, context.as_ref()) {
        // The converter's own window is the answer.
        Ok(()) => None,
        Err(error) => {
            log::warn!("could not launch {program}: {error}");
            Some(i18n("Could not launch the converter"))
        }
    }
}

/// Start `program` once with every path, through GIO, which reaps it.
fn launch(
    program: &str,
    paths: &[String],
    cuts: &[(f64, f64)],
    source_hdr: Option<&str>,
    context: Option<&gdk::AppLaunchContext>,
) -> Result<(), glib::Error> {
    // GLib splits the command line like a shell without running one, then
    // expands `%` field codes; the paths go in as files through `%F`.
    let mut commandline = glib::shell_quote(program);
    for arg in converter_args(cuts, source_hdr) {
        commandline.push(" ");
        commandline.push(glib::shell_quote(arg.replace('%', "%%")));
    }
    commandline.push(" %F");
    // Without startup notification GLib asks the context for no token.
    let app = gio::AppInfo::create_from_commandline(
        commandline,
        None,
        gio::AppInfoCreateFlags::SUPPORTS_STARTUP_NOTIFICATION,
    )?;
    let files: Vec<gio::File> = paths.iter().map(gio::File::for_commandline_arg).collect();
    app.launch(&files, context)
}

fn converter_args(cuts: &[(f64, f64)], source_hdr: Option<&str>) -> Vec<String> {
    let mut args = Vec::with_capacity(3);
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
    args
}

#[cfg(test)]
mod tests {
    use super::{converter_args, launch};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    #[test]
    fn cuts_precede_the_end_of_options_marker() {
        assert_eq!(
            converter_args(&[(1.5, 3.0), (10.0, 12.345_678_9)], Some("pq")),
            ["--segments=1.5-3,10-12.3456789", "--source-hdr=pq", "--"]
        );
        assert_eq!(converter_args(&[], None), ["--"]);
    }

    /// The paths reach one converter process as separate, unparsed arguments.
    #[test]
    fn launch_keeps_paths_verbatim() {
        let temp = tempfile::tempdir().unwrap();
        let record = temp.path().join("argv");
        let program = temp.path().join("converter");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.part'\nmv '{0}.part' '{0}'\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let paths = [
            "/v/a b'c$HOME%f.mkv".to_string(),
            "/v/-dash.mkv".to_string(),
            "file:///v/u%20ri.mkv".to_string(),
        ];
        launch(
            program.to_str().unwrap(),
            &paths,
            &[(1.5, 3.0)],
            Some("hlg"),
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !record.exists() {
            assert!(Instant::now() < deadline, "converter did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&record).unwrap(),
            "--segments=1.5-3\n--source-hdr=hlg\n--\n/v/a b'c$HOME%f.mkv\n/v/-dash.mkv\n/v/u ri.mkv\n"
        );
    }
}
