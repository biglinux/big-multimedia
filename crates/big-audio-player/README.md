# Big Audio Player

Music and audio player for BigLinux

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
make -f Makefile.ci ci   # full gate: fmt clippy test deny audit machete doc complexity i18n budgets
```

The individual cargo gate commands (`fmt`/`clippy`/`nextest`/`doc`/`deny`/
`audit`/`machete`) are listed in [REVIEW.md](REVIEW.md). CI runs each as a
separate job (`.github/workflows/ci.yml`).

## Install

Packaged via PKGBUILD (Arch/Manjaro/BigLinux):

```bash
cd packaging/pacman && makepkg -si
```

The PKGBUILD builds from a local checkout when one is detected, otherwise
fetches release tarballs. See [Packaging](#packaging) for Nix and Flatpak.

## Run

```bash
big-audio-player
```

Launches the GTK4 player window. Transcoding is handed off to
`big-audio-converter` (preferred) or `big-video-converter` (fallback) when
installed.

## Tree

- `src/`: Rust application entrypoints and app-owned code.
- `docs/`: structure and maintenance notes (`project-structure.md`, `native-deps-exceptions.md`).
- `resources/`: installed desktop metadata and icons shared by packages.
- `application-assets/`: gresource sources, CSS, and desktop-integration assets.
- `packaging/pacman/`: BigLinux/pacman packaging source.
- `packaging/nix/`: Nix packaging source.
- `packaging/flatpak/`: Flatpak/Flathub packaging source.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

## Contributing

Read [ARCHITECTURE.md](ARCHITECTURE.md) (module map) and
[INVARIANTS.md](INVARIANTS.md) (enforced invariants) first, then run
`make -f Makefile.ci ci` before opening a PR.

AI agents and new contributors: start with [AGENTS.md](AGENTS.md), then
ARCHITECTURE.md and INVARIANTS.md.

## Packaging

- Pacman/BigLinux: `packaging/pacman/PKGBUILD`
- Nix: `packaging/nix/flake.nix`
- Flatpak/Flathub: `packaging/flatpak/br.com.biglinux.AudioPlayer.yml`

Packaging metadata is source. Keep desktop files, AppStream metadata, icons,
install hooks, and manifests validated before release.
Native library dependencies belong in package manifests, not copied into this
repository.
