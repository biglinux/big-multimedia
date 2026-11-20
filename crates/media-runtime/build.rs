use std::env;
use std::error::Error;
use std::path::PathBuf;

include!("build_gresource.rs");

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);

    compile_gresource(
        &out_dir,
        "audio.gresource",
        "data/audio-resources",
        "data/audio-resources/resources.gresource.xml",
    )
}
