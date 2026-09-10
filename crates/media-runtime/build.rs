use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    // Compile video GResource
    compile_gresource(
        &out_dir,
        "video.gresource",
        "data/resources",
        "data/resources/resources.gresource.xml",
    )?;

    // Compile audio GResource
    compile_gresource(
        &out_dir,
        "audio.gresource",
        "data/audio-resources",
        "data/audio-resources/resources.gresource.xml",
    )?;

    Ok(())
}

fn compile_gresource(
    out_dir: &Path,
    output: &str,
    source_dir: &str,
    xml: &str,
) -> Result<(), Box<dyn Error>> {
    let gresource_out = out_dir.join(output);
    let status = Command::new("glib-compile-resources")
        .args([
            &format!("--sourcedir={source_dir}"),
            &format!("--target={}", gresource_out.display()),
            xml,
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rerun-if-changed={source_dir}/");
            Ok(())
        }
        Ok(status) => Err(format!("glib-compile-resources failed for {xml}: {status}").into()),
        Err(error) => {
            Err(format!("failed to run glib-compile-resources for {xml}: {error}").into())
        }
    }
}
