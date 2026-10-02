# big-audio-effects: MIT relicense rationale

This crate ships under MIT (workspace default), not GPL.

## Why MIT is correct

All source files (`lib.rs`, `equalizer.rs`, `noise_reduction.rs`)
are 100% original BigLinux code. The crate exposes:

- Constants (`/usr/lib/ladspa/lib*.so` paths as `&'static str`)
- Constant tables (EQ band frequencies and labels)
- `format!` strings that FFmpeg/mpv parse later in their own process

There is **no**:

- `dlopen` of LADSPA/RNNoise/DeepFilterNet from Rust
- Import of GPL Rust crates
- Inlined or copied LADSPA/RNNoise/DeepFilterNet code
- Static linking against GPL libraries

LADSPA plugin paths are *referenced* as strings; the actual loading is
performed by FFmpeg/mpv as separate processes (`Command::new`), which is
mere aggregation under the GPL FAQ — not derivative work.

## Effect on the workspace

The vendored component crates are MIT. The crates that link `libmpv` via FFI
(`big-media-runtime`, the two players) carry GPL-3.0-or-later and document it
in their own `LICENSE`; the aggregate repository is GPL-3.0-or-later.
