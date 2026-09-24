//! Run the cast pipeline's `ffmpeg`/`ffprobe` under a bubblewrap sandbox.
//!
//! ffmpeg and ffprobe parse the media file — which can come from anywhere — with
//! a large C codebase and a long CVE history; a crafted file that exploits one
//! of them would otherwise run with the user's full privileges. Confining these
//! subprocesses to no network, no filesystem beyond the input file(s) plus an
//! owned scratch dir, and only the GPU render nodes keeps such an exploit from
//! reaching the network, other files, or other devices. Hardware transcode
//! (`h264_nvenc`, VAAPI) still works inside it — the GPU character devices are
//! world-accessible, so the sandbox's user namespace does not lose them.
//!
//! `bubblewrap` is a hard package dependency; when it is somehow absent every
//! entry point returns `None` and the caller refuses to run (secure default).

use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::sync::OnceLock;

/// Whether `bwrap` is on `PATH` (probed once).
pub fn available() -> bool {
    static AVAIL: OnceLock<bool> = OnceLock::new();
    *AVAIL.get_or_init(|| {
        std::process::Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// Fixed bubblewrap options: no network / IPC / PID / user namespace shared, a
/// cleared environment, a fresh `/proc` + minimal `/dev` with the GPU render
/// nodes bound for hardware transcode, and `/usr` (+ `/etc`, `/sys`) read-only
/// so the dynamic linker and GPU drivers resolve.
fn base_options() -> Vec<String> {
    let mut options: Vec<String> = [
        "--die-with-parent",
        "--new-session",
        "--unshare-all",
        "--clearenv",
        "--setenv",
        "PATH",
        "/usr/bin",
        "--setenv",
        "HOME",
        "/nonexistent",
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        // GPU nodes (character devices, world-accessible on this class) — the
        // hardware transcode paths need them; `-try` ignores absent ones.
        "--dev-bind-try",
        "/dev/dri",
        "/dev/dri",
        "--dev-bind-try",
        "/dev/nvidiactl",
        "/dev/nvidiactl",
        "--dev-bind-try",
        "/dev/nvidia-modeset",
        "/dev/nvidia-modeset",
        "--dev-bind-try",
        "/dev/nvidia-uvm",
        "/dev/nvidia-uvm",
        "--dev-bind-try",
        "/dev/nvidia-uvm-tools",
        "/dev/nvidia-uvm-tools",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind-try",
        "/etc",
        "/etc",
        "--symlink",
        "usr/lib",
        "/lib",
        "--symlink",
        "usr/lib",
        "/lib64",
        "--symlink",
        "usr/bin",
        "/bin",
        "--symlink",
        "usr/bin",
        "/sbin",
        // libva / the NVIDIA userspace driver read device info from /sys.
        "--ro-bind-try",
        "/sys",
        "/sys",
    ]
    .iter()
    .map(|opt| (*opt).to_string())
    .collect();
    if let Ok(entries) = std::fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name
                .strip_prefix("nvidia")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
                && entry.file_type().is_ok_and(|kind| kind.is_char_device())
            {
                let path = format!("/dev/{name}");
                options.extend(["--dev-bind-try".into(), path.clone(), path]);
            }
        }
    }
    // Keep CUDA ordinals identical to the detecting process, including masks
    // and explicit PCI ordering. All other inherited variables remain cleared.
    for key in ["CUDA_VISIBLE_DEVICES", "CUDA_DEVICE_ORDER"] {
        if let Ok(value) = std::env::var(key) {
            options.extend(["--setenv".into(), key.into(), value]);
        }
    }
    options
}

/// bubblewrap option argv (everything between `bwrap` and the program). `ro`
/// paths are bound read-only (the media input, an extracted subtitle); `rw`
/// read-write (the 0700 scratch dir). `None` when bwrap is unavailable.
pub fn options(ro: &[&Path], rw: &[&Path]) -> Option<Vec<String>> {
    if !available() {
        return None;
    }
    let mut args = base_options();
    for path in ro {
        let path = path.to_string_lossy().into_owned();
        args.push("--ro-bind".to_string());
        args.push(path.clone());
        args.push(path);
    }
    for path in rw {
        let path = path.to_string_lossy().into_owned();
        args.push("--bind".to_string());
        args.push(path.clone());
        args.push(path);
    }
    Some(args)
}

/// Full argv to pass a sandboxed `program` to a `BigSubprocessSpec` whose own
/// `program` is `bwrap`: `[sandbox-options.., program, args..]`. `None` when
/// bwrap is unavailable.
pub fn wrap_args(program: &str, args: &[&str], ro: &[&Path], rw: &[&Path]) -> Option<Vec<String>> {
    let mut argv = options(ro, rw)?;
    argv.push(program.to_string());
    argv.extend(args.iter().map(|arg| (*arg).to_string()));
    Some(argv)
}
