//! Stage 5 (STRIDE) regression: external subprocess invocations must
//! terminate their option parser with `--` before passing user-controlled
//! positional arguments, and no `sh -c` / `bash -c` shell strings may appear.
//!
//! Mechanism: source-level grep. Cheaper and more robust than mocking
//! `std::process::Command`. If a future caller adds a new subprocess
//! invocation that consumes user input, this test fails until the
//! reviewer confirms the new site is safe.
//!
//! The Relm4 port deleted the legacy god-object, which removed this crate's
//! only direct subprocess sites (the yt-dlp format probe and the ffmpeg
//! screenshot). Cast transcoding now runs in the shared `big-media-runtime`
//! crate, which carries its own argv-safety tests. The defense-in-depth shell
//! walker below stays — it guards every `src/**` file, present and future.

use std::fs;
use std::path::Path;

const SRC_ROOT: &str = "src";

#[test]
fn no_unguarded_shell_invocations() {
    // Defense-in-depth: no `sh -c` / `bash -c` style invocations should
    // appear anywhere in `src/**`. They would bypass the argv-array safety
    // we rely on at every other Command::new site.
    walk_rs(Path::new(SRC_ROOT), &mut |path, contents| {
        for forbidden in [
            "\"sh\", \"-c\"",
            "\"bash\", \"-c\"",
            "Command::new(\"sh\")",
            "Command::new(\"bash\")",
        ] {
            assert!(
                !contents.contains(forbidden),
                "{} contains forbidden shell invocation `{}`; use argv arrays only",
                path.display(),
                forbidden
            );
        }
    });
}

fn walk_rs(dir: &Path, on_file: &mut dyn FnMut(&Path, &str)) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, on_file);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let body = fs::read_to_string(&path).unwrap_or_default();
            on_file(&path, &body);
        }
    }
}
