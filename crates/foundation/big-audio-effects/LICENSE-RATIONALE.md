# big-audio-effects: MIT relicense rationale

This crate ships under MIT (workspace default), not GPL.

## Why MIT is correct

All source files (`lib.rs`, `dynamics.rs`, `equalizer.rs`, `noise_reduction.rs`)
are 100% original BigLinux code. The crate exposes:

- Constants (`/usr/lib/ladspa/lib*.so` paths as `&'static str`)
- Preset arrays (EQ band frequencies, gate/compressor curves)
- Pure math (`db_to_linear`, derived gate values)
- `format!` strings that FFmpeg/mpv parse later in their own process

There is **no**:

- `dlopen` of LADSPA/RNNoise/DeepFilterNet from Rust
- Import of GPL Rust crates
- Inlined or copied LADSPA/RNNoise/DeepFilterNet code
- Static linking against GPL libraries

LADSPA plugin paths are *referenced* as strings; the actual loading is
performed by FFmpeg/mpv as separate processes (`Command::new`), which is
mere aggregation under the GPL FAQ — not derivative work.

## Effect on the foundation

`big-rust-components` is MIT end-to-end. Apps that link `libmpv` via FFI
(`big-media-runtime`, players) carry GPL by their own choice and document
it in their own `LICENSE` + `deny.toml` GPL allow-list.

The MIT foundation does NOT contaminate GPL apps — it just lets MIT-only
consumers (e.g. a future BigLinux text editor that has no media deps)
reuse the audio-effect contracts without inheriting GPL obligations.
