# INVARIANTS

Living document of the invariants the state-of-the-art pipeline
discovered, the test or check that defends each one, and the
file:line where it is enforced.

Re-runs of the pipeline append to or refine this document; never
delete an invariant — replace it with a tightened version and
note the prior wording in the commit message.

## Stage 4 — Supply chain

- **All workspace deps satisfy `deny.toml`.** Defender:
  `cargo deny check` job in `.github/workflows/ci.yml`. Waiver
  list lives at `deny.toml` (RUSTSEC-2024-0436 paste — transitive
  via `lofty`).
- **Project license is GPL-3.0-or-later.** Defender: `Cargo.toml`
  metadata + `LICENSE` at repo root (canonical GPL v3 text).

## Stage 5 — Threats (high-severity, landed)

- **H1: external-converter argv carries a `--` end-of-options
  sentinel before user-supplied paths.** Defender:
  `src/audio/converter_launcher.rs:20,27`. A `grep` for
  `Command::new("big-audio-converter").arg("--")` must stay green.
- **L5: M3U/PLS playlist import is capped at 16 MiB.** Defender:
  `MAX_PLAYLIST_BYTES` at `src/audio/window/playlists/mod.rs:8` +
  the two `#[test]`s `import_rejects_oversize_playlist` and
  `import_accepts_small_playlist`.

## Stage 6 — Resource lifecycle

- **No `/tmp` fallback for the favicon / explore / MPRIS-art
  caches.** Defender:
  - `src/audio/window/album_art.rs::save_mpris_art` returns `None`
    when `dirs::cache_dir()` is unavailable.
  - `src/audio/radio/cache.rs::{favicon_cache_dir,
    explore_cache_path}` return `Option<PathBuf>`; callers
    early-return on `None`.
  - There is no `PathBuf::from("/tmp")` in `src/`.
- **Every reassignable `glib::SourceId` field has a matching
  `.remove()` on the prior id.** Defender: `src/audio/window/mod.rs`
  field list lines 227–286, with releases in `state.rs:67`,
  `dialogs.rs:204`, `radio/build.rs:281`, `radio_playback.rs:23`,
  `library/rebuild.rs:28`, `browser/setup.rs:122`,
  `playback/track.rs:18`, `playback/queue.rs:24`,
  `library_scan/scan.rs:153`, `window/mod.rs:530`.
- **`glib::clone!` discipline: zero `#[strong]` self captures.**
  Defender: `grep -rn '#\[strong\]' src/` must stay at 0.
  All 31 self-captures use `#[weak]`.

## Stage 7 — Performance budgets (measured)

- **Stripped release binary ≤ 25 MiB.** Current 18.08 MiB.
- **`cargo test --locked` wall ≤ 5 s.** Current 0.22 s debug / 2.56 s release.
- **Cold-start RSS @ 1 s ≤ 240 MiB.** Current 207 MiB (PSS 144 MiB).
- **Idle RSS @ 60 s ≤ 360 MiB.** Current 333 MiB (PSS 269 MiB).
- **sched_switch ≤ 100/s idle.** Current 77/s.
- **sched_wakeup ≤ 2/s idle.** Current 0.27/s.
- **Click → local-file playback start: p50 ≤ 80 ms, p99 ≤ 300 ms.** Current 62 / 184 ms.
- **Click → next track: p50 ≤ 100 ms, p99 ≤ 400 ms.** Current 71 / 211 ms.
- **Search filter keystroke → refresh: p50 ≤ 16 ms, p99 ≤ 50 ms.** Current 9 / 38 ms.
- **Largest single live allocation ≤ 64 MiB.** Current peak 32.79 MiB
  (`av_buffer_realloc` in libavcodec). Owned by upstream mpv.
- Budgets are re-measured by the `budgets` job in
  `.github/workflows/ci.yml` / `make -f Makefile.ci budgets`.

## Stage 8 — i18n total coverage

- **Every user-visible Rust string routes through
  `i18n(&str)` or `ni18n(singular, plural, n)`.** Strings resolve
  through `big_media_runtime::i18n`; the 10 bare `set_label` sites
  are all `""` / `"● LIVE"` and excluded. This repo has no `po/`
  catalog — translation catalogs are owned by `big-media-runtime`.
  The CI `i18n` job runs an xgettext/msgfmt round-trip only when a
  `po/` directory is present (a no-op here).
- **Plural forms use `ni18n`, never `format!`.** Defender: the
  two count-bearing strings (`detail/tracks.rs:75`,
  `browser/context_menu.rs:126`) use `ni18n`. A future linter
  pass can codify this.

## Stage 9 — Accessibility

- **Every icon-only `gtk::Button` has either a textual `.label()`
  or an `update_property(&[accessible::Property::Label(…)])` call
  within the same builder block.** Three icon-only sites were lifted
  in commit `fix(a11y): add Property::Label to icon-only sub/unsub/add
  buttons`. Runtime enforcement requires an AT-SPI walk via the
  `linux-ui-a11y` harness (opt-in `atspi-smoke` job; out of the
  default CI gate).

## Stage 10 — UX

- **Permanent file deletion requires both a checkbox flip and an
  explicit "delete" dialog response.** Defender:
  `src/audio/window/browser/context_menu.rs:226` (`response !=
  "delete"` early-return) + `perm_row.is_active()` gate at the
  callback. Same pattern at `playlist_panel/menu.rs:196`.

## Stage 11 — Agent CL (debt log)

- **0 hand-written files exceed the 400-LoC budget.** The lone
  remaining >400 LoC file (`src/audio/window/imp.rs`, 477) is
  template-child boilerplate required as a single unit by the
  `gtk::CompositeTemplate` derive; splitting would require a
  new abstraction layer and is explicitly excluded.
  Original 7-file backlog split in commits
  `refactor(library_grouping)`, `refactor(detail/columns)`,
  `refactor(radio_search)`, `refactor(library_scan/folders)`,
  `refactor(playlist_panel)`, `refactor(window)`,
  `refactor(radio)`.

## Stage 12 — Tests

- **77 keep + 2 added = 79 tests.** Defender: CI `cargo test --locked`.
  Any deletion requires an INVARIANT update with rationale.

## Stage 13 — CI gates

- `.github/workflows/ci.yml` splits the gate into separate jobs:
  `shellcheck`, `fmt`, `clippy`, `test`, `deny`, `component-check`,
  `audit`, `machete`, `doc`, `semver-checks`, `miri`, `complexity`,
  `i18n`, `budgets`, `appstream-validate`, `desktop-validate` (plus
  opt-in `atspi-smoke` / `vm-smoke`).
- BigLinux component-check is strict; it expects a sibling
  `../bigagents-tools` checkout in CI to run
  `scripts/component-check.mjs . --strict`. If missing, the job
  emits a `::notice` rather than failing — so the workflow stays
  green on a fork without the sibling tooling, but produces a
  loud audit signal upstream.

## How to re-enter

This document is the persisted record of the audit pipeline; the
per-stage working notes are generated locally and not committed.
Re-audit whole-pipeline quarterly, on
`big-rust-components`/`big-media-runtime` public-API changes
(stages 1, 3, 7, 12, 13 only), or after a security advisory
(stage 4 only).
