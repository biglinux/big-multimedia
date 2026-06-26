# Big Video Converter

GPU-aware video converter for BigLinux

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
cargo build
cargo nextest run --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Full gate matrix (fmt, clippy, doc, tests, deny, audit, machete): see
`REVIEW.md`.

## Tree

- `src/main.rs`: thin binary launcher; starts `big-host --separate converter-video`.
- `src/lib.rs`: delegates to `big_converter_ui::run(RunOptions { mode: Video, … })`
  and exports `APP_MODE`.
- `tests/`: integration (`converter_integration.rs`) and smoke (`smoke.rs`).
- `docs/`: agent-readable structure and maintenance notes.
- `po/`: gettext translation catalogs.
- `resources/`: installed desktop metadata and icons shared by packages.
- `packaging/pacman/`: BigLinux/pacman packaging source.
- `packaging/nix/`: Nix packaging source.
- `packaging/flatpak/`: Flatpak/Flathub packaging source.

The converter window, sidebar, dialogs, edit/queue pages, and settings are
owned upstream in `../big-converter-ui`, not in this
repo. See `ARCHITECTURE.md` for the wrapper design and `INVARIANTS.md` for the
enforced contract.

## Packaging

- Pacman/BigLinux: `packaging/pacman/PKGBUILD`
- Nix: `packaging/nix/flake.nix`
- Flatpak/Flathub: `packaging/flatpak/br.com.biglinux.VideoConverter.yml`

Packaging metadata is source. Keep desktop files, AppStream metadata, icons,
install hooks, and manifests validated before release.
Native library dependencies belong in package manifests, not copied into this
repository.
