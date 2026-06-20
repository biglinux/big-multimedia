# big-video-player — Audit Record — 2026-06-12

## Score: 10.0 / 10 (deterministic gate; see follow-ups)

Rust GUI (lib + bin): BigLinux Big Video Player, fully on **Relm4 0.11**
(manual `impl Component` idiom, macros off). The former gtk4-subclass
god-object (`window/`, ~17k LOC) was deleted during the V-phase port;
root component `src/video/ui/video_shell.rs` composes app-local panels
(browser sidebar with grid cards + hover-scrub previews, controls bar,
IPTV, cast, filters, appearance, preferences). Engines: sibling
`big-media-runtime` (mpv render context, cast). No library DB by design
(player only). Shared widgets via `big-relm4-components` /
`big-media-components` (e.g. `BigVideoTransport`, copyable info rows,
`bytes`/`time` formatters).

## Gate state

Last full pass green during the sidebar series (`2b67b9d`…`fb2f5c7`):
fmt / clippy `-D warnings` / nextest / doc / deny / audit / machete.
Re-run `quality-gate.mjs` before tagging a release.

S1 supply-chain pass (2026-06-12): removed unused `tracing-subscriber` and
the stale direct `big-mpv-render` dependency; lock/allowlist refreshed.
`fmt`, `clippy`, `cargo test`, `cargo doc`, `cargo machete`,
component-check, and supply-chain strict PASS.

S2 dialog-helper pass (2026-06-12): media-adjust dialogs use the promoted
runtime binding over `big_app_kit::dialogs::persist_dialog_size`. `fmt`,
`clippy`, `cargo test`, `cargo doc`, `cargo machete`, and supply-chain
strict PASS.

S3 shared-evacuation pass (2026-06-13): appearance dialogs, player audio
preferences, IPTV HTTP/URL helpers, duration formatting, and common player UI
imports now use `big-player-ui`, `big-app-kit`, `big-media-components`, or
`big-audio-effects` directly instead of runtime shared shims. `fmt`,
`clippy`, `cargo test`, and `cargo doc` PASS.

S5 split/residual pass (2026-06-13): `video_shell.rs` is now a 127-line root
over responsibility modules (largest new shell module: `component.rs`, 565 L);
`video_controls_bar.rs` is 355 L with popover/seek/track/zones modules;
sidebar `views.rs` and `chrome.rs` are thin facades over focused modules
(largest: `views/grid_view.rs`, 367 L). Bottom OSD button-zone reparenting,
image rotate/flip/crop, cast transcode filter snapshots plus shader/GPU cast
sync, and yt-dlp quality selection are implemented. Screenshot parity is via
the in-app `mpv.screenshot_to_file` path. Smartphone web remote is deliberately
not ported until video has a thread-safe playback status snapshot equivalent
to audio's `SharedMprisState`.

## Known follow-ups (tracked in ../IMPROVEMENTS-2026-06-12.md)

- Continue migrating remaining reusable player panels to `big-player-ui` as
  part of future file-size/module splits; the S3 runtime-shim dependency is
  removed.
- GUI smokes (`--audit` weston + heaptrack RSS) on a VM before publish.

## Residual accepted

- `cargo +nightly miri test` deferred for FFI-heavy crates (mpv, gtk4).
- `RUSTSEC-2024-0436 paste` (transitive, accepted workspace-wide).

## Reproduce gates

```sh
cd big-video-player   # from the repo root
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
