# Big Audio Converter

Audio converter for BigLinux

This repository is a BigLinux Rust desktop application. App code should stay
thin and reuse tested crates from
[big-rust-components](https://github.com/biglinux/big-rust-components).
Profile: `media`.

Native dependency policy: prefer distro/system libraries and dynamic linking.
Bundling is allowed only when pacman has no suitable package, licensing is
acceptable, and `docs/native-deps-exceptions.md` records the exception.

## Status

This split repository is standalone at the app layer. Runtime modules shared
with the other multimedia apps live in the sibling `big-media-runtime`
repository.

## Build

```bash
cargo build --release --locked
```

Quality gate (same checks CI runs — see `.github/workflows/ci.yml`):

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features   # or: cargo test --workspace --all-features
cargo deny check
```

This crate is a thin wrapper: `src/` holds only `main.rs` + `lib.rs`. The
converter UI/state lives in `../big-rust-components/crates/big-converter-ui`
and ffmpeg orchestration in `../big-media-runtime`. See `ARCHITECTURE.md` and
`docs/project-structure.md`.

## Tree

- `src/main.rs`: thin binary entrypoint; delegates to `lib.rs`.
- `src/lib.rs`: run binding (boots the shared UI in `ConverterMode::Audio`) and
  exported constants.
- `docs/`: agent-readable structure and maintenance notes.
- `resources/`: installed desktop metadata, AppStream metainfo, and icons.
- `packaging/pacman/`: BigLinux/pacman packaging source.
- `packaging/nix/`: Nix packaging source.
- `packaging/flatpak/`: Flatpak/Flathub packaging source.

## Packaging

- Pacman/BigLinux: `packaging/pacman/PKGBUILD`
- Nix: `packaging/nix/flake.nix`
- Flatpak/Flathub: `packaging/flatpak/br.com.biglinux.AudioConverter.yml`

Packaging metadata is source. Keep desktop files, AppStream metadata, icons,
install hooks, and manifests validated before release.
Native library dependencies belong in package manifests, not copied into this
repository.
