# Project Structure

This tree is part of the agent interface. Names should tell future agents where
to edit before they open files.

## Source

This is a thin wrapper. `src/` holds only two files:

- `src/main.rs`: binary entrypoint. Delegates to `lib.rs`. Keep thin.
- `src/lib.rs`: run binding — boots the shared UI in `ConverterMode::Audio`;
  exports `APP_MODE`, `BOOTSTRAP_ENV_KEY`, `BOOTSTRAP_ENV_VAL`.
- `Cargo.toml`: Rust package contract.
- `Cargo.lock`: committed for application repositories (gate uses `--locked`).
- `AGENTS.md`: project-local agent rules. Keep local project rules here.

The converter window, sidebar, dialogs, edit pages, queue page, audio preview,
and settings persistence are **not** in this repo — they live in the sibling
crate `../big-converter-ui`. ffmpeg orchestration
lives in `../big-media-runtime`. Edit converter behaviour there.

Note: this binary runs the UI **in-process** (`run_audio_converter_application`) — audio conversion
needs no native media engine. Its twin `big-video-converter` instead execs
`big-host` to share an isolated mpv engine + `big-convertd` queue (video is
`engine_owner`). That entrypoint asymmetry between the two converters is
intentional. The audio host module (`libbig_converter_audio.so`) is owned by
`big-modules/big-converter-audio`, not this repo.

## Shared Code Policy

Prefer the sibling `../big-media-runtime` crate for shared multimedia backend
runtime only.
Prefer reviewed crates from `big-rust-components` for reusable UI, media,
tasks, file pickers, drag-and-drop, dialogs, and validation helpers.

If code is useful in more than one BigLinux app, move it to a component crate
instead of copying it between applications.
Do not move app-owned converter windows/dialog flows back into
`big-media-runtime`; runtime must not own application UI.

Native/media dependencies should stay system-owned when available. Use distro
packages for GTK, libadwaita, mpv, FFmpeg/libav, GStreamer, PipeWire, codecs,
and drivers. Do not add vendored codec/native source trees unless pacman has no
suitable package, licensing is acceptable, and an approved exception is recorded
in `docs/native-deps-exceptions.md`.

## Packaging

- `resources/applications/br.com.biglinux.AudioConverter.desktop`: desktop launcher metadata.
- `resources/metainfo/br.com.biglinux.AudioConverter.metainfo.xml`: AppStream metadata.
- `resources/icons/hicolor/scalable/apps/big-audio-converter.svg`: application icon.
- `packaging/pacman/PKGBUILD`: BigLinux/pacman source package.
- `packaging/nix/flake.nix`: Nix package source.
- `packaging/flatpak/br.com.biglinux.AudioConverter.yml`: Flatpak manifest.

## Validation

The gate is the cargo matrix defined in `.github/workflows/ci.yml`:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features   # or: cargo test --workspace --all-features
cargo deny check
cargo audit -D warnings
cargo machete --with-metadata
for po in po/*.po; do msgfmt -c -v -o /dev/null "$po"; done
```

Missing required quality or security tools are failures. See `REVIEW.md` for
the full reproduce list and `AGENTS.md` for the agent workflow.

Project: `big-audio-converter` (Big Audio Converter).
