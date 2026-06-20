# INVARIANTS — big-video-converter

Twin of `big-audio-converter/INVARIANTS.md` — 47 / 52 source files are
byte-identical; the invariants below diverge only where noted with **(video-only)**.

## Stage 4 — License + supply chain

- **GPL-3.0-or-later coherence.** `Cargo.toml:7` ↔ `LICENSE`. Tested by CI
  `supply-chain`.
- **No vendored binary blobs.**
- **Known unmaintained: `paste` 1.0.15.** Ignored in `deny.toml:13`.

## Stage 5 — Threat model

- **GL symbol loader cannot be LD_PRELOAD-hijacked.** `glGetIntegerv`
  resolved via `dlopen("libGL.so.1", RTLD_LAZY | RTLD_LOCAL)`, cached.
  Enforced post-Stage-3 in
  `big-converter-ui/src/converter/ui/edit_page/helpers.rs`.
- **ffprobe argv hardened with `--` separator.**
  `big-converter-ui/src/converter/ui/audio_edit_page/playback.rs`.
- **ffmpeg argv hardened with explicit `-i`.** Same.
- **`/tmp` symlink race in cast transcode — FIXED.** Owned 0700 `TempDir`
  per `TranscodeSession`; cmd-log opens use `create_new` + mode 0600.
  Enforced: `big-media-runtime/src/cast/transcode/session.rs`. Tested by
  `scratch_path_is_inside_owned_temp_dir_and_outside_tmp_world`.

## Stage 6 — Resource lifecycle

- **Progress poll timer leak — FIXED.** `ConverterModel` stores the tick
  `SourceId` and removes it in `Drop`.
- **GIO mount missing cancellable — FIXED.** Mount paired with a
  `Cancellable` cancelled by a 30 s timeout.
- **Event pump after unload — FIXED.** Pump returns
  `ControlFlow::Break` once the `async_channel::Sender` is dropped.
- **(video-only) Render-loop event pump.** `edit_page/render.rs:409`
  50 ms timeout — same shape as the audio pump; still to be fixed in a
  follow-up (deferred).

## Stage 7 — Performance budgets

- **Cold RSS ≤ 110 MiB** (heavier than audio twin by ~20 MiB for GL).
- **Idle RSS drift ≤ +15 MiB / 60 s.**
- **Idle wakeups ≤ 60 / s.**
- **FileChooser → queue p99 ≤ 250 ms.**
- **Start → first ffmpeg stderr p99 ≤ 700 ms.**
- **(video-only) Edit page → first frame p99 ≤ 900 ms.**
- **(video-only) Crop apply → preview p99 ≤ 200 ms.**

## Stage 8 — i18n

- **`i18n()` routing required for user-visible strings.** Coverage 32 %.
  CI job `i18n-coverage` greps for bare set_*. Bypass: `// i18n-ignore`.

## Stage 9 — Accessibility

- **No color-only state.**
- **No grade-D (wrong role) widgets.**
- **(video-only) DrawingArea preview is intentionally non-focusable;** all
  interactive controls live in the overlay button strip which is
  Tab-navigable.

## Stage 10 — User cognitive load

- **Destructive actions confirm** (clear_queue, remove_item, clear_segments).
  Enforced in
  `big-converter-ui/src/converter/ui/window/{header_signals,build,queue_signals}.rs`
  and `edit_page/actions.rs`.
- **Empty states explain.**

## Stage 11 — Agent cognitive load

- **Files ≤ 700 LOC; functions ≤ 80 LOC.** No current violators
  (`edit_page/render.rs` is 587 LOC, under budget).
- **Doc anchors on pub items.**
- **Module heads ≥ 5 lines.**
- **No ambiguous suffix names.**
