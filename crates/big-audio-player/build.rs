use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    compile_gresource(
        &out_dir,
        "audio-player-shell.gresource",
        "application-assets/resources",
        "application-assets/resources/resources.gresource.xml",
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
        .status()?;

    if !status.success() {
        return Err(format!("glib-compile-resources failed for {xml}").into());
    }

    println!("cargo:rerun-if-changed={source_dir}/");
    Ok(())
}
