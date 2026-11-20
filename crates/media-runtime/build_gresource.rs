// Included by the build scripts of media-runtime and both players, which each
// compile their own GResource bundle.

/// Compile `xml`, whose files live under `source_dir`, into `out_dir/output`.
fn compile_gresource(
    out_dir: &std::path::Path,
    output: &str,
    source_dir: &str,
    xml: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let gresource_out = out_dir.join(output);
    let status = std::process::Command::new("glib-compile-resources")
        .args([
            &format!("--sourcedir={source_dir}"),
            &format!("--target={}", gresource_out.display()),
            xml,
        ])
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo::rerun-if-changed={source_dir}/");
            Ok(())
        }
        Ok(status) => Err(format!("glib-compile-resources failed for {xml}: {status}").into()),
        Err(error) => {
            Err(format!("failed to run glib-compile-resources for {xml}: {error}").into())
        }
    }
}
