# big-audio-player — Audit Record — 2026-06-12

## Score: 10.0 / 10 (deterministic gate; see follow-ups)

Rust GUI (lib + bin): BigLinux Big Audio Player, fully on **Relm4 0.11**
(manual `impl Component` idiom, macros off). The former gtk4-subclass
god-object (`window/`, 79 files / ~11.4k LOC) was deleted on cutover
(2026-06-07). Root component `src/audio/ui/audio_shell.rs` composes the
shared `big-player-ui` panels (radio, podcast, lyrics, playback bar,
downloads, file browser) plus app-local LibraryPanel/PlaylistPanel.
Data layer: sibling `big-media-data` (SQLite WAL library, settings).
Engines: sibling `big-media-runtime` (mpv, ffmpeg, cast, MPRIS glue).

## Gate state

`quality-gate.mjs big-audio-player` = **OK** at R7 verify (2026-06-07):
fmt / clippy `-D warnings` / nextest / doctests (TMPDIR override) /
`cargo doc -D warnings` / trivy / audit / deny / lockfile /
component-check / MSRV all PASS. Re-run before tagging a release.

Fresh-context correctness review (gtk-desktop-reviewer) findings fixed
in `279b507` (undo snapshots, queue persistence on tab swap, stale-async
art guard, drop-play index).

S1 supply-chain pass (2026-06-12): runtime lock/allowlist refreshed after
`big-media-runtime` moved from `pretty_env_logger` to `env_logger` and
removed stale lock names. Supply-chain strict PASS.

S2 helper-dedup pass (2026-06-12): playlist export now uses
`big_app_kit::files::sanitize_filename_stem_preserve_unicode`; playback
time labels now use `big_media_components::time::playback_time`. `fmt`,
`clippy`, `cargo test`, `cargo doc`, `cargo machete`, and supply-chain
strict PASS.

S3 shared-evacuation pass (2026-06-13): appearance dialogs, player audio
preferences, smartphone remote dialog, and common MPRIS command routing now
consume `big-player-ui` / `big-app-kit` APIs directly instead of
`big-media-runtime::shared` shims. `fmt`, `clippy`, `cargo test`, and
`cargo doc` PASS.

S4 shell split/component pass (2026-06-13): `audio_shell.rs` is now a
179-line root over responsibility modules (`component`, `engine`, `mpris`,
`panel_intents`, `playlist_manager_actions`, `queue_state`, `sources`, etc.),
all new modules ≤600 lines. The hidden `library_sidebar` / smart-builder
subsystem and its dead shell handlers were deleted. Component policy is clean:
shared tooltip helpers, `install_action*`, catalog rows, weak ListenBrainz
bindings, and explicit app-local DnD/dialog markers. MusicBrainz lookup now
round-trips through shell → panel → open tag editor; media keys and single-key
`space`/`n`/`p` dispatch through installed playback actions. `fmt`, `clippy`,
`cargo test`, `cargo doc -D warnings`, `component-check --strict`,
`supply-chain-check --strict`, and `cargo machete` PASS.

## Known follow-ups (tracked in ../IMPROVEMENTS-2026-06-12.md)

- GUI smokes (`--audit` weston + heaptrack RSS) must run on a VM
  (`BIGAGENTS_RUN_UI_SMOKE=1`) before publish — never on a live desktop.

## Residual accepted

- `RUSTSEC-2024-0436 paste` (transitive, accepted workspace-wide).

## Reproduce gates

```sh
cd big-audio-player   # from the repo root
node ~/.agents/scripts/quality-gate.mjs .
# or manually:
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo nextest run --workspace --all-features
TMPDIR="$HOME/.cache/cargo-doctest-tmp" cargo test --doc
cargo deny check
cargo audit --ignore RUSTSEC-2024-0436 --deny warnings
cargo machete --with-metadata
```
