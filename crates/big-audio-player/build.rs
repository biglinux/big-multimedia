use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    compile_gresource(
        &out_dir,
        "audio-player-shell.gresource",
        "application-assets/resources",
        "application-assets/resources/resources.gresource.xml",
    );
}

fn compile_gresource(out_dir: &Path, output: &str, source_dir: &str, xml: &str) {
    let gresource_out = out_dir.join(output);
    let status = Command::new("glib-compile-resources")
        .args([
            &format!("--sourcedir={source_dir}"),
            &format!("--target={}", gresource_out.display()),
            xml,
        ])
        .status();

    match status {
        Ok(status) if status.success() => println!("cargo:rerun-if-changed={source_dir}/"),
        _ => panic!("glib-compile-resources failed for {xml}"),
    }
}
