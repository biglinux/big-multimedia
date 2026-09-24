# Big Video Player

Hardware-accelerated video player for BigLinux

This repository is a BigLinux Rust desktop application. App code should stay
thin and reuse tested crates from
[big-multimedia](https://github.com/biglinux/big-multimedia).
Profile: `media`.

Native dependency policy: prefer distro/system libraries and dynamic linking.
Bundling is allowed only when pacman has no suitable package, licensing is
acceptable, and `docs/native-deps-exceptions.md` records the exception.

## Status

This repository is self-contained: the media engine (`big-media-runtime`) and
the shared component crates are vendored as workspace members under `crates/`,
so it builds with no external `big-*` path dependencies.

## Build

```bash
cargo build
./dev/quality-check.sh --ci
```

Optional: regenerate the local, gitignored `.agent/` map from a
`bigagents-tools` checkout (not needed to build or contribute):

```bash
BIGAGENTS_TOOLS=/path/to/bigagents-tools ./dev/update-bigagents.sh
```

## Tree

- `AGENTS.md`: canonical agent rules (build/test, module map, where to edit).
- `src/`: current Rust application entrypoints and app-owned code.
- `docs/`: agent-readable structure and maintenance notes.
- `dev/`: repository maintenance commands, not application runtime code.
- `resources/`: installed desktop metadata and icons shared by packages.
- `packaging/pacman/`: BigLinux/pacman packaging source.
- `legacy/`: previous implementation kept as reference only, when present.
- `.agent/`: optional bigagents-tools project map, generated locally by
  `dev/update-bigagents.sh`; gitignored and not part of a clone.

## Packaging

- Pacman/BigLinux: `packaging/pacman/PKGBUILD`

Packaging metadata is source. Keep desktop files, AppStream metadata, icons,
install hooks, and manifests validated before release.
Native library dependencies belong in package manifests, not copied into this
repository.
