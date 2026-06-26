# Project Structure

This tree is part of the agent interface. Names should tell future agents where
to edit before they open files.

## Source

This repo is a thin wrapper. Its only source is:

- `src/main.rs`: binary entrypoint. A **launcher** — it execs
  `big-host --separate converter-video`; it does NOT run the UI in-process.
- `src/lib.rs`: standalone binding. `main_impl()` boots the shared UI
  in-process (`big_converter_ui::run(RunOptions { mode: Video, … })`) and
  exports `APP_MODE`. Kept as the reference in-proc path; the shipped binary
  uses the launcher above, not `main_impl`.
- `Cargo.toml`: Rust package contract.
- `Cargo.lock`: committed for application repositories.

### Why video launches through big-host (and audio does not)

`big-audio-converter`'s binary runs its UI **in-process** (`main_impl`): it
needs no native media engine. `big-video-converter` instead execs `big-host`
because video transcoding is `engine_owner` with `native_caps = ["mpv",
"ffmpeg"]` and `requires_daemon = ["big-convertd"]` — routing the standalone
binary through the host gives it the **same isolated mpv engine + convertd
queue** as the embedded module path, instead of duplicating that backend in a
second process. This asymmetry is intentional, not a bug.

The `dlopen`-able host module (`libbig_converter_video.so` + its
`converter-video.toml` manifest) is owned by `big-modules/big-converter-video`,
packaged as `big-converter-video` — NOT by this repo.

Converter window, sidebar, dialogs, edit/queue pages, and settings are owned
upstream in `../big-converter-ui`, not in this repo.
Edits to that UI/settings logic belong there, not here. See `ARCHITECTURE.md`
for the cross-repo map.

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

- `resources/applications/br.com.biglinux.VideoConverter.desktop`: desktop launcher metadata.
- `resources/metainfo/br.com.biglinux.VideoConverter.metainfo.xml`: AppStream metadata.
- `resources/icons/hicolor/scalable/apps/big-video-converter.svg`: application icon.
- `packaging/pacman/PKGBUILD`: BigLinux/pacman source package.
- `packaging/nix/flake.nix`: Nix package source.
- `packaging/flatpak/br.com.biglinux.VideoConverter.yml`: Flatpak manifest.

## Validation

- `cargo nextest run --workspace --all-features`: tests.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: lint.
- `cargo fmt --all -- --check`: format.
- Full gate matrix (fmt, clippy, doc, tests, deny, audit, machete): see
  `REVIEW.md`.

Project: `big-video-converter` (Big Video Converter).
