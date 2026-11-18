use std::env;
use std::error::Error;
use std::path::PathBuf;

include!("../media-runtime/build_gresource.rs");

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
    // allow-noisy-log: cargo build directives, not log output.
    for symbol in env::var("DEP_BIG_OS_KIT_ALLOCATOR_SYMBOLS")?.split(',') {
        println!("cargo::rustc-link-arg-bins=-Wl,--export-dynamic-symbol={symbol}");
    }
    Ok(())
}
