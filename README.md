# big-multimedia

Unified workspace for the BigLinux media suite — players and converters in one
repo, because they share the same engine (`big-media-runtime`: mpv playback +
ffmpeg conversion) and the same edit/format-quality components
(`big_media_components::video::*`). Co-locating them lets the players embed the
converter's clip/export surface in-process.

| Crate (binary) | Role |
|----------------|------|
| `crates/big-video-player` | Video player + in-place **clip & export** (cut for social), simple convert, extract audio |
| `crates/big-audio-player` | Audio player + in-place **trim & export**, simple convert |
| `crates/big-video-converter` | Batch video converter (full editor + queue) |
| `crates/big-audio-converter` | Batch audio converter |

The players reuse the embeddable `big_converter_ui::embed` panel for the single-
file edit/export path; the converters remain the batch powerhouse. All four also
ship as `big-host` modules (`big-modules/big-{player,converter}-{audio,video}`).

GPL-3.0-or-later (all four link the GPL `big-media-runtime` → libmpv).

## Build

```sh
cargo build --workspace
cargo run -p big-video-player
cargo run -p big-audio-player
cargo run -p big-video-converter
cargo run -p big-audio-converter
```

Per-crate `po/`, `resources/`, `data/`, `packaging/`, `build.rs`, and the audit
doc-set stay under each crate; shared `rust-toolchain.toml`/`deny.toml`/`LICENSE`
+ the workspace `Cargo.toml` live at the root.
