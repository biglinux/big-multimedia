//! Security regression tests for `BigSubprocessSpec` argv validation.

use big_app_kit::subprocess::{BigSubprocessError, BigSubprocessSpec};

fn assert_rejects(byte: u8) {
    let argument_with_forbidden_byte = format!("safe-prefix{}suffix", byte as char);
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("echo")
        .arg(argument_with_forbidden_byte.clone())
        .build();
    let validation_error = subprocess_spec
        .try_resolved()
        .expect_err("argv should be rejected");
    assert!(
        matches!(validation_error, BigSubprocessError::ForbiddenArgvByte { byte: rejected_byte, .. } if rejected_byte == byte),
        "expected ForbiddenArgvByte({byte:#x}), got {validation_error:?}"
    );
}

#[test]
fn argv_rejects_nul() {
    assert_rejects(0x00);
}

#[test]
fn argv_rejects_newline() {
    assert_rejects(0x0a);
}

#[test]
fn argv_rejects_carriage_return() {
    assert_rejects(0x0d);
}

#[test]
fn argv_rejects_del() {
    assert_rejects(0x7f);
}

#[test]
fn program_with_nul_rejected() {
    let subprocess_spec = BigSubprocessSpec::builder().program("ec\0ho").build();
    let validation_error = subprocess_spec.try_resolved().unwrap_err();
    assert!(matches!(
        validation_error,
        BigSubprocessError::ForbiddenArgvByte { index: 0, byte: 0 }
    ));
}

#[test]
fn argv_accepts_normal_strings() {
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("ffprobe")
        .arg("-of")
        .arg("json")
        .arg("/tmp/file with spaces.mp4")
        .arg("--utf8=café")
        .build();
    assert!(subprocess_spec.try_resolved().is_ok());
}

#[test]
fn argv_accepts_tab() {
    // Tab is allowed; only NUL, LF, CR and DEL are rejected.
    let subprocess_spec = BigSubprocessSpec::builder()
        .program("printf")
        .arg("a\tb")
        .build();
    assert!(subprocess_spec.try_resolved().is_ok());
}
