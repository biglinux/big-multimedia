# INVARIANTS

Living document of the invariants the state-of-the-art pipeline discovered,
the test or check that defends each one, and the file:line where it is
enforced.

Re-runs of the pipeline append to or refine this document; never delete
an invariant — replace it with a tightened version and note the prior
wording in the commit message.

## Stage 4 — Supply chain

- **All workspace deps satisfy `deny.toml`.** Defender: `cargo deny check`
  job in `.github/workflows/ci.yml`. Waiver list lives at `deny.toml`
  (`RUSTSEC-2024-0436` paste — transitive via `lofty`).
- **Project license is GPL-3.0-or-later.** Defender: `Cargo.toml` metadata
  + `LICENSE` at repo root (canonical GPL v3 text).
- **License allowance set tracks the actual dep surface.** Defender:
  pruned in commit `chore(deny): drop unencountered license allowances`
  (BSD-2-Clause / LGPL-3.0-or-later / Unicode-DFS-2016 removed).

## Stage 5 — Threats (high-severity, landed)

- **H1: yt-dlp argv carries `--` before user-supplied URL.** Defender:
  `tests/stage05_argv_terminator.rs::ytdlp_argv_uses_double_dash_separator`
  + grep regression at `src/video/dialogs/format_selection.rs:109`. A URL
  beginning with `-` would otherwise be parsed as a yt-dlp option
  (e.g. `--exec`, `--load-info-json`).
- **H2: ffmpeg screenshot argv carries `--` before positional output path.**
  Defender:
  `tests/stage05_argv_terminator.rs::ffmpeg_screenshot_argv_uses_double_dash_separator`
  + grep regression at `src/video/window/overlays/screenshot.rs`. A path
  beginning with `-` would otherwise be parsed as an option.
- **No `sh -c` / `bash -c` / `Command::new("sh"/"bash")` anywhere in `src/`.**
  Defender:
  `tests/stage05_argv_terminator.rs::no_unguarded_shell_invocations`
  (the `test` CI job runs it; there is no separate shell-grep gate).

## Stage 6 — Resource lifecycle

- **Every reassignable `glib::SourceId` field has a matching `.remove()`
  on the prior id.** Defender: five `RefCell<Option<SourceId>>` fields
  on `imp::Window` (`src/video/window/mod.rs:177,211,217,222,233`) each
  with `take().remove()` cancel sites at those same sites.
- **Long-lived `glib::timeout_add_seconds_local` callbacks use `#[weak]`
  self + `ControlFlow::Break` upgrade fallback.** Defender:
  `src/video/window/init/lifecycle.rs:147 setup_periodic_flush` (pattern
  re-used elsewhere). Auto-cancels on window finalize.
- **All `std::process::Command` spawns are awaited (`.status()` /
  `.output()`).** Defender: the four spawn sites (yt-dlp in
  `src/video/dialogs/format_selection.rs`, ffmpeg in
  `src/video/window/overlays/screenshot.rs`, qdbus6 + wmctrl in
  `src/video/window/init/controls.rs`) each await their child.

## Stage 7 — Performance budgets

- **Stripped release binary ≤ 22 MiB.** Defender: `budgets` job in
  `.github/workflows/ci.yml` (binary-size step). Current measurement:
  17.29 MiB.
- **Workspace LOC ≤ 18,000.** Tracked target (not gated by the `budgets`
  job, which checks only binary size + idle RSS). Current: 16,579.
- **`cargo test --workspace` runtime ≤ 20 s.** Tracked target run by
  the `test` CI job. Current measurement: <1 s. (Not a hard CI gate.)
- **Idle RSS ≤ 256 MiB.** Defender: `budgets` job in
  `.github/workflows/ci.yml` (idle-RSS step, best-effort under `xvfb`).
  Local baseline under `kwin_wayland --virtual` / `dbus-run-session`:
  174 MiB, zero growth between t+1 s and t+60 s.
- **Idle context-switches ≤ 500/s.** Tracked target from a local
  `/proc/<pid>/schedstat` delta baseline (not CI-gated). Current:
  ~322/s (idle gradient tick at 30 Hz dominates).
- **Idle thread count ≤ 64; FD count ≤ 32.** Tracked target from the
  same local baseline. Current: 42 threads, 23 fds at t+60 s.
- **Live latency / heaptrack / flamegraph / dropped-frames** still pending
  a runner with DRI nodes. Skipped on the headless host because
  (a) `kernel.perf_event_paranoid=2` blocks `sched:sched_switch` for
  non-root and (b) `kwin_wayland --virtual` has no DRI nodes wired in, so
  mpv would not initialise a gl/vaapi context.

## Stage 8 — i18n total coverage

- **Every user-visible Rust string routes through `i18n(&str)` or
  `ni18n(singular, plural, n)`.** Defender: source grep for bare
  label-setter literals. 691 `i18n(...)` call sites; 0 bare
  label-setter literals at last audit.
- **Count-bearing strings use `ni18n`, never `format!`.** Defender:
  four `ni18n` sites at
  `src/video/window/sidebar_build/info_footer.rs:99,143`,
  `src/video/iptv_view/channel_list.rs:117`,
  `src/video/dialogs/cast/device_dialog.rs:60`.
- **`SHORTCUTS` const-array entries are translated at consumer sites**
  (`src/video/dialogs/keyboard_shortcuts.rs:67,95`). xgettext picks up
  the `i18n(...)` call regardless of the literal's storage location.

## Stage 9 — Accessibility

- **Every icon-only `gtk::Button`/`gtk::MenuButton` carries either a
  text `.label()` in `adw::ButtonContent` or an
  `update_property(&[accessible::Property::Label(…)])` call.** Defender:
  source audit of the 13 icon-only button sites. Runtime enforcement
  requires an AT-SPI walk (out of CI scope today; see `linux-ui-a11y`).

## Stage 10 — UX

- **Closing the window while casting requires an explicit "Stop TV
  transmission?" confirmation.** Defender:
  `src/video/window/init/lifecycle.rs:108–143`.
- **First-launch welcome screen exposes all three openers as primary
  buttons (no settings wall).** Defender:
  `src/video/window/overlays/mod.rs` (`setup_welcome_screen`).
- **All listed empty states (sidebar, recent, IPTV channel, cast device)
  explain themselves with status text and CTAs.** Defender: source audit
  of the empty-state widgets.

## Stage 11 — Agent CL (debt log)

- **Zero source files exceed the 400-LOC budget.** Tracked convention,
  not a CI gate (the `budgets` job checks only binary size + idle RSS).
  All 8 originally listed offenders were split in a residual pass:
  `playback_effects`,
  `input` (key_dispatch), `actions` (playback + video), `window` (cast
  bridge + menus), `overlays_cast` (sync), `subtitles_groups` (position
  + special), `overlays` (screenshot), `sidebar/grid` (card). Commits:
  `874df22`, `f18c5a5`, `06b6672`, `ce5216a`, `1bf618a`, `f2d77a6`,
  `4392c8f`, `6fd29be` (plus actions split).
- **`imp::Window` CompositeTemplate stays in a single file.** Accepted
  exception: `gtk::glib::object_subclass!` + `CompositeTemplate` derive
  must own the full template-child enumeration and may not be split
  across submodules without re-introducing parser ambiguity. Defender:
  `src/video/window/imp.rs` is excluded from any future split pass.
- **Screenshot module pins the Stage 5 argv invariant.** Defender:
  doc-comment at `src/video/window/overlays/screenshot.rs:5` cites
  `tests/stage05_argv_terminator.rs::ffmpeg_screenshot_argv_uses_double_dash_separator`.
  The grep regression site moved from `overlays.rs:172` to
  `overlays/screenshot.rs:*` and the test still passes after the split.

## Stage 12 — Tests

- **33 unit + 3 integration = 36 tests.** Defender: the `test` CI job
  (`cargo nextest run --all-features --workspace`, fallback
  `cargo test --all-features --tests --workspace`). Any deletion
  requires an INVARIANT update with rationale.

## Stage 13 — CI gates

- `.github/workflows/ci.yml` runs one job per gate (no combined `rust`
  job, and none of the cargo invocations pass `--locked`):
  - **`shellcheck`** — `shellcheck -e SC1091 -S warning` over tracked
    `*.sh`.
  - **`fmt`** — `cargo fmt --all -- --check`.
  - **`clippy`** — `cargo clippy --workspace --all-targets --all-features
    -- -D warnings`.
  - **`test`** — `cargo nextest run --all-features --workspace` (fallback
    `cargo test --all-features --tests --workspace`).
  - **`deny`** — `cargo deny check advisories bans licenses sources`.
  - **`audit`** — `cargo audit -D warnings`.
  - **`machete`** — `cargo machete --with-metadata`.
  - **`doc`** — `cargo doc --no-deps --all-features --workspace`
    (`RUSTDOCFLAGS=-D warnings`).
  - **`complexity`** — lizard CCN ≤ 25 + jscpd duplication ≤ 5%.
  - **`i18n`** — POT round-trip + `msgfmt -c -v`; no-ops when no `po/`.
  - **`budgets`** — release-binary size cap (≤ 22 MiB) + best-effort
    headless idle-RSS cap (≤ 256 MiB). It does **not** gate workspace
    LOC, `#[allow(dead_code)]`, shell invocations, or component-check.
  - **`appstream-validate`** / **`desktop-validate`** — metainfo +
    `.desktop` validators.
  - **`atspi-smoke`** (`ci:atspi` label) / **`vm-smoke`** (`ci:vm-smoke`
    label) — opt-in.
  - **`semver-checks`** / **`miri`** — disabled (libs-only flags off).
- There is no automated BigLinux `component-check` step in CI today; run
  it locally via `../bigagents-tools` (`node
  scripts/component-check.mjs . --strict`) before publishing shared-crate
  changes.

## How to re-enter

The per-stage working notes that produced these invariants are generated
locally under `tmp/agent_working/` (gitignored; not part of a public
clone). This file is the durable record. Re-audit cadence: whole-pipeline
re-runs quarterly, on `big-rust-components` / `big-media-runtime`
public-API changes (re-check stages 1, 3, 7, 12, 13), or after a security
advisory (re-check stage 4).
