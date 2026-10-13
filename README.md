# big-multimedia

Workspace for the BigLinux media **players** (audio + video). It holds the
media engine (`big-media-runtime`: mpv playback, cast, media probing) and the
component crates the players share; the only external BigLinux dependency is
[big-gtk-kit](https://github.com/biglinux/big-gtk-kit), the shared GTK building
blocks. Conversion lives in the separate converter GUIs.

| Crate (binary) | Role |
|----------------|------|
| `crates/big-video-player` | Video player; marks cuts and hands them, or the whole file, to `big-video-converter-gui` (extract audio goes to `big-audio-converter-gui`) |
| `crates/big-audio-player` | Audio player; marks cuts and hands them, or the whole track, to `big-audio-converter-gui` |

The players convert nothing themselves: `big_media_runtime::shared::converter_handoff`
opens the converter GUI with `--segments=START-END,...` and the file.

Library crates under `crates/`: `media-runtime`, `foundation/*` (os-kit,
audio-effects, media-engines, mpv-render, media-data), `app/*` (app-kit,
app-kit-core), `ui/generic/big-relm4-components` and
`ui/media/{big-media-components,big-player-ui}`. The component crates are MIT;
the repository as a whole is GPL-3.0-or-later because the players link the GPL
engine. `third_party/relm4` is Relm4 with a lifetime patch (see
`CONTRIBUTING.md`).

## Build

```sh
cargo build --workspace --locked
cargo run --locked -p big-video-player
cargo run --locked -p big-audio-player
```

Per-crate `resources/`, `data/`, `packaging/`, and `build.rs` stay under each
crate. Translations are one shared catalog for the whole suite in
`crates/media-runtime/po` (regenerate with `crates/media-runtime/po/regen-pot.sh`).
The workspace `Cargo.toml` (with `[workspace.package]` /
`[workspace.dependencies]`), `clippy.toml`, `rust-toolchain.toml`, `deny.toml`,
and `LICENSE` live at the root.

Build, test and resource-lifetime rules: `CONTRIBUTING.md`.
