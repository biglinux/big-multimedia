//! No `sh -c` / `bash -c` shell strings may appear anywhere in `src/**`: every
//! subprocess goes through argv arrays. Source-level grep, cheaper and more
//! robust than mocking `std::process::Command`.

#[cfg(not(miri))]
use std::fs;
#[cfg(not(miri))]
use std::path::Path;

#[cfg(not(miri))]
const SRC_ROOT: &str = "src";

#[cfg(not(miri))]
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

#[cfg(not(miri))]
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
