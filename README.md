# big-multimedia

Self-contained workspace for the BigLinux media **players** (audio + video).
It vendors the media engine (`big-media-runtime`: mpv playback + ffmpeg export)
and every shared component crate, so it builds with **no external `big-*` path
dependencies** — the standalone batch converters now live in a separate project.

| Crate (binary) | Role |
|----------------|------|
| `crates/big-video-player` | Video player + in-place **clip & export** (cut for social), simple convert, extract audio |
| `crates/big-audio-player` | Audio player + in-place **trim & export**, simple convert |

The players reuse `big_quick_export` (container/codec/quality options + a
single-file export driver) for the in-place edit/export path, and hand batch
work off to the external converter binary by name.

Vendored under `crates/` (were external path deps, now workspace members):
`media-runtime`, `foundation/*` (os-kit, audio-effects, media-engines,
appearance, mpv-render, media-data), `app/*` (app-kit, app-kit-core),
`ui/generic/big-relm4-components`, `ui/media/{big-media-components,big-player-ui}`,
`dev/big-testkit`. These keep their upstream MIT license; the aggregate repo is
GPL-3.0-or-later because the players link the GPL engine.

## Build

```sh
cargo build --workspace
cargo run -p big-video-player
cargo run -p big-audio-player
```

Per-crate `po/`, `resources/`, `data/`, `packaging/`, and `build.rs` stay under
each crate; the workspace `Cargo.toml` (with `[workspace.package]` /
`[workspace.dependencies]`), `clippy.toml`, `rust-toolchain.toml`, `deny.toml`,
and `LICENSE` live at the root.
