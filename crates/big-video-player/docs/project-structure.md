# Project Structure

This tree is part of the agent interface. Names should tell future agents where
to edit before they open files.

## Source

- `src/main.rs`: binary entrypoint. Keep thin.
- `Cargo.toml`: Rust package contract.
- `Cargo.lock`: committed for application repositories.
- `AGENTS.md`: project-local agent rules. Keep local project rules here.

## Shared Code Policy

Prefer the sibling `../big-media-runtime` crate for shared multimedia runtime.
Prefer reviewed crates from `big-rust-components` for reusable UI, media,
tasks, file pickers, drag-and-drop, dialogs, and validation helpers.

If code is useful in more than one BigLinux app, move it to a component crate
instead of copying it between applications.

Native/media dependencies should stay system-owned when available. Use distro
packages for GTK, libadwaita, mpv, FFmpeg/libav, GStreamer, PipeWire, codecs,
and drivers. Do not add vendored codec/native source trees unless pacman has no
suitable package, licensing is acceptable, and an approved exception is recorded
in `docs/native-deps-exceptions.md`.

## Packaging

- `resources/applications/br.com.biglinux.VideoPlayer.desktop`: desktop launcher metadata.
- `resources/metainfo/br.com.biglinux.VideoPlayer.metainfo.xml`: AppStream metadata.
- `resources/icons/hicolor/scalable/apps/big-video-player.svg`: application icon.
- `packaging/pacman/PKGBUILD`: BigLinux/pacman source package.
- `packaging/nix/flake.nix`: Nix package source.
- `packaging/flatpak/br.com.biglinux.VideoPlayer.yml`: Flatpak manifest.

## Validation

- `dev/quality-check.sh --ci`: focused default gate.
- `dev/quality-check.sh --full`: deeper local gate. Missing required quality
  or security tools are failures.
- `dev/update-bigagents.sh`: optional. Regenerates the local, gitignored
  `.agent/` map and active skills from a `bigagents-tools` checkout
  without overwriting the committed `AGENTS.md`. Not required to build.

## Legacy

`legacy/` is reference-only migration material. Do not add new runtime code
there. Delete it after parity is proven and release migration is complete.

Project: `big-video-player` (Big Video Player).
