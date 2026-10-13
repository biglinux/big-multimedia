// SPDX-License-Identifier: MIT

//! Tests for the subprocess spec and its Linux supervisor.

use super::*;

#[test]
fn empty_program_rejected() {
    let spec = BigSubprocessSpec::builder().program("").build();
    let err = spec.try_resolved().unwrap_err();
    assert!(matches!(err, BigSubprocessError::EmptyProgram));
}

#[test]
fn redact_replaces_secret_in_captured_stdout() {
    let mut buf = b"token=ABCDEF rest".to_vec();
    redact_in_place(&["ABCDEF".to_string()], &mut buf);
    assert_eq!(buf, b"token=[REDACTED] rest");
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// concurrent stdout draining against a real child process.
#[cfg_attr(miri, ignore)]
fn run_does_not_deadlock_on_large_output() {
    // A child writing far more than the OS pipe buffer (~64 KiB) must not
    // deadlock: with a short timeout, the only way this completes is if
    // stdout is drained concurrently while the child runs.
    let bytes = 512 * 1024;
    let spec = BigSubprocessSpec::builder()
        .program("head")
        .arg("-c")
        .arg(bytes.to_string())
        .arg("/dev/zero")
        .timeout(Duration::from_secs(5))
        .build();
    let out = spec.run().expect("head runs without timeout");
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), bytes);
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// captured stdout from a real child process.
#[cfg_attr(miri, ignore)]
fn run_captures_stdout() {
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("hello big-app-kit")
        .timeout(Duration::from_secs(5))
        .build();
    let out = spec.run().expect("echo runs");
    assert!(out.status.success());
    assert!(out.stdout_lossy().contains("hello big-app-kit"));
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// detached launch behavior against a real child process.
#[cfg_attr(miri, ignore)]
fn spawn_detached_launches_and_returns() {
    BigSubprocessSpec::builder()
        .program("true")
        .build()
        .spawn_detached()
        .expect("detached true");
}

#[test]
fn forbidden_control_byte_in_program_rejected() {
    for forbidden in ['\u{0}', '\n', '\r', '\u{7f}'] {
        let spec = BigSubprocessSpec::builder()
            .program(format!("ec{forbidden}ho"))
            .build();
        let err = spec.try_resolved().unwrap_err();
        assert!(
            matches!(err, BigSubprocessError::ForbiddenArgvByte { index: 0, byte } if byte == forbidden as u8),
            "byte {:#04x} must be rejected at the program field, got {err:?}",
            forbidden as u32
        );
    }
}

#[test]
fn control_byte_error_uses_one_based_index() {
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("clean")
        .arg("bad\ninjection")
        .build();
    let err = spec.try_resolved().unwrap_err();
    // argv[1] (the second token) carries the newline → reported index 2.
    assert!(
        matches!(
            err,
            BigSubprocessError::ForbiddenArgvByte {
                index: 2,
                byte: 0x0a
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn clean_argv_with_shell_metacharacters_passes_validation() {
    // Shell metacharacters are data (no shell is used); only control bytes are
    // rejected, so a clean-but-spicy argv must resolve OK.
    let spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg("; rm -rf / && curl evil | sh")
        .arg("/tmp/file name.mp4")
        .build();
    assert!(spec.try_resolved().is_ok());
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// captured stderr and status code against a real failing child process.
#[cfg_attr(miri, ignore)]
fn captures_stderr_and_exit_code_on_failure() {
    // `ls` on a missing path writes to stderr and exits non-zero (code 2 on
    // GNU coreutils). Exercises BigSubprocessStatus::code + stderr_lossy.
    let out = BigSubprocessSpec::builder()
        .program("ls")
        .arg("/nonexistent_path_xyzzy_42")
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .expect("ls runs");
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stderr_lossy().contains("/nonexistent_path_xyzzy_42"),
        "stderr should name the missing path: {:?}",
        out.stderr_lossy()
    );
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks
// environment inheritance against a real child process.
#[cfg_attr(miri, ignore)]
fn env_inherited_by_default() {
    // Default specs inherit the parent environment, so the child sees PATH.
    let out = BigSubprocessSpec::builder()
        .program("env")
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .expect("env runs");
    assert!(out.status.success());
    assert!(
        out.stdout_lossy().contains("PATH="),
        "inherited environment must include PATH"
    );
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// flipping the cancel flag kills a long-lived child well under its timeout.
#[cfg_attr(miri, ignore)]
fn run_cancellable_kills_child_on_cancel_flag() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    // 5 s sleep with a 30 s timeout: the only way this returns fast is the
    // cancel flag, not the deadline.
    let spec = BigSubprocessSpec::builder()
        .program("sleep")
        .arg("5")
        .timeout(Duration::from_secs(30))
        .build();

    let cancel = Arc::new(AtomicBool::new(false));
    let flipper = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        flipper.store(true, std::sync::atomic::Ordering::Relaxed);
    });

    let start = std::time::Instant::now();
    let err = spec.run_cancellable(&cancel).unwrap_err();
    let elapsed = start.elapsed();

    assert!(
        matches!(err, BigSubprocessError::Cancelled),
        "expected Cancelled, got {err:?}"
    );
    // Killed within one poll tick of the flip, far under the 5 s sleep / 30 s
    // timeout. Generous 2 s bound tolerates loaded CI.
    assert!(
        elapsed < Duration::from_secs(2),
        "cancel must kill fast, took {elapsed:?}"
    );
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// a fast command completes normally when the cancel flag never flips.
#[cfg_attr(miri, ignore)]
fn run_cancellable_returns_output_when_not_cancelled() {
    use std::sync::atomic::AtomicBool;

    let cancel = AtomicBool::new(false);
    let out = BigSubprocessSpec::builder()
        .program("echo")
        .arg("uncancelled")
        .timeout(Duration::from_secs(5))
        .build()
        .run_cancellable(&cancel)
        .expect("echo runs");
    assert!(out.status.success());
    assert!(out.stdout_lossy().contains("uncancelled"));
    assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
// Miri does not implement `posix_spawn`; normal `cargo test` still checks that
// the timeout still fires when neither cancel nor completion happens.
#[cfg_attr(miri, ignore)]
fn run_cancellable_still_times_out_without_cancel() {
    use std::sync::atomic::AtomicBool;

    // Never-flipped cancel + a sleep that outlives the short timeout: the
    // deadline path must still win and report Timeout, not Cancelled.
    let cancel = AtomicBool::new(false);
    let err = BigSubprocessSpec::builder()
        .program("sleep")
        .arg("5")
        .timeout(Duration::from_millis(200))
        .build()
        .run_cancellable(&cancel)
        .unwrap_err();
    assert!(
        matches!(err, BigSubprocessError::Timeout),
        "expected Timeout, got {err:?}"
    );
}

#[test]
fn redaction_survives_non_utf8() {
    let mut buf = b"secret\xffsecret".to_vec();
    redact_in_place(&["secret".to_owned()], &mut buf);
    assert_eq!(buf, b"[REDACTED]\xff[REDACTED]");
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn delayed_descendant_output_is_captured() {
    let result = BigSubprocessSpec::builder()
        .program("sh")
        .args(["-c", "(sleep 0.1; printf FINAL) &"])
        .timeout(Duration::from_secs(2))
        .build()
        .run()
        .expect("delayed output");
    assert!(result.status.success());
    assert_eq!(result.stdout, b"FINAL");
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn inherited_pipe_and_unread_stdin_obey_deadline() {
    for args in [["-c", "sleep 10"], ["-c", "sleep 10 &"]] {
        let start = std::time::Instant::now();
        let error = BigSubprocessSpec::builder()
            .program("sh")
            .args(args)
            .stdin(vec![0; 1024 * 1024])
            .timeout(Duration::from_millis(150))
            .build()
            .run()
            .unwrap_err();
        assert!(matches!(
            error,
            BigSubprocessError::Timeout | BigSubprocessError::Io(_)
        ));
        assert!(start.elapsed() < Duration::from_secs(3));
    }
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn large_stdin_and_stdout_complete() {
    let result = BigSubprocessSpec::builder()
        .program("sh")
        .args(["-c", "head -c 131072 /dev/zero; cat >/dev/null"])
        .stdin(vec![7; 1024 * 1024])
        .timeout(Duration::from_secs(5))
        .build()
        .run()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(result.stdout.len(), 131072);
}

#[test]
fn argv_preserves_order_and_separators() {
    let spec = BigSubprocessSpec::builder()
        .program("ffprobe")
        .arg("-of")
        .arg("json")
        .arg("/tmp/x.mp4")
        .build();
    let r = spec.try_resolved().unwrap();
    assert_eq!(r.program, "ffprobe");
    assert_eq!(r.argv, ["-of", "json", "/tmp/x.mp4"]);
}

#[test]
fn defaults_are_safe() {
    let spec = BigSubprocessSpec::builder().program("echo").build();
    let r = spec.try_resolved().unwrap();
    assert_eq!(r.stdout, BigSubprocessOutputMode::Capture);
    assert_eq!(r.stderr, BigSubprocessOutputMode::Capture);
    assert_eq!(r.timeout_ms, 30_000);
    assert!(r.env_inherit);
    assert_eq!(r.stdin_len, 0);
}

#[test]
fn empty_allow_list_denies_every_program() {
    let policy = BigSubprocessAllowList::default();
    assert!(!policy.permits("sh"));
    assert!(!policy.permits("/usr/bin/true"));
}

#[cfg(target_os = "linux")]
#[test]
#[cfg_attr(miri, ignore)]
fn oversized_capture_is_an_error() {
    let error = BigSubprocessSpec::builder()
        .program("head")
        .arg("-c")
        .arg((DEFAULT_MAX_CAPTURE_BYTES + 1).to_string())
        .arg("/dev/zero")
        .timeout(Duration::from_secs(10))
        .build()
        .run()
        .unwrap_err();
    assert!(matches!(
        error,
        BigSubprocessError::OutputLimit {
            stream: "stdout",
            ..
        }
    ));
}
