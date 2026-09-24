use std::env;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    compile_gresource(
        &out_dir,
        "video-player-shell.gresource",
        "application-assets/resources",
        "application-assets/resources/resources.gresource.xml",
    )?;
    // big-os-kit's jemalloc shim replaces the C allocator for the whole process;
    // export it so libraries opened later with dlopen (GPU drivers, GIO modules)
    // bind to it instead of glibc's.
    for symbol in [
        "malloc",
        "calloc",
        "realloc",
        "free",
        "posix_memalign",
        "aligned_alloc",
        "memalign",
        "valloc",
        "pvalloc",
        "malloc_usable_size",
    ] {
        println!("cargo:rustc-link-arg-bins=-Wl,--export-dynamic-symbol={symbol}");
    }
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

    if status.success() {
        println!("cargo:rerun-if-changed={source_dir}/");
        return Ok(());
    }

    Err(std::io::Error::other(format!(
        "glib-compile-resources failed for {xml} with status {status}"
    ))
    .into())
}
