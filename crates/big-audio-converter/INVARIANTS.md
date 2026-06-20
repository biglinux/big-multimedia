# INVARIANTS — big-audio-converter

Living document. One paragraph per stage 4–11 finding from the workspace
audit pipeline (stages 4–11). Each entry names the invariant, the test that
defends it (or `(no test yet)`), and the file:line where it is enforced.

## Stage 4 — License + supply chain

- **GPL-3.0-or-later coherence.** Crate license matches `LICENSE` at root.
  Enforced: `Cargo.toml:7`, `LICENSE`. Tested by CI job `supply-chain`
  (`cargo-deny-action`).
- **No vendored binary blobs.** Enforced: repo-wide grep. Tested by CI
  `supply-chain` (cargo-deny `[sources]`).
- **Known unmaintained: `paste` 1.0.15 via lofty.** Ignored explicitly in
  `deny.toml:13` with rationale.

## Stage 5 — Threat model

- **GL symbol loader cannot be LD_PRELOAD-hijacked.** `glGetIntegerv` is
  resolved via `dlopen("libGL.so.1", RTLD_LAZY | RTLD_LOCAL)`, not
  `RTLD_DEFAULT`. Enforced post-Stage-3 in
  `big-converter-ui/src/converter/ui/edit_page/helpers.rs` (cached in
  `OnceLock`). (no test yet — Stage 12 entry
  `path_lookup_is_library_scoped`.)
- **ffprobe argv hardened.** Path argument is preceded by `--` so a
  leading-dash file name cannot be parsed as a flag. Enforced post-Stage-3
  in `big-converter-ui/src/converter/ui/audio_edit_page/playback.rs`.
- **ffmpeg argv hardened.** Path follows explicit `-i`. Enforced same
  file. Comment notes the contract for future maintainers.
- **`/tmp` symlink race in cast transcode — FIXED.** All scratch files
  (`ffmpeg-cmd.log`, `ffmpeg-stderr.log`, extracted subtitle `.mkv`) now
  live inside an owned 0700 `TempDir` created lazily per
  `TranscodeSession`. Defense in depth: cmd-log opens use
  `OpenOptions::create_new(true).mode(0o600)`. Enforced:
  `big-media-runtime/src/cast/transcode/session.rs`. Tested by
  `scratch_path_is_inside_owned_temp_dir_and_outside_tmp_world`.

## Stage 6 — Resource lifecycle

- **Progress poll timer leak (LEAK #1) — FIXED.** `ConverterModel` now
  stores the 200 ms tick's `SourceId` and removes it in `Drop`. Enforced:
  `big-converter-ui/src/converter/ui/audio_converter.rs`.
- **GIO mount missing cancellable (LEAK #2) — FIXED.** Network-URL
  dialog now pairs the mount call with a `gio::Cancellable` cancelled by
  a 30 s `glib::timeout_add_seconds_local_once`. Enforced:
  `big-converter-ui/src/converter/ui/window/dialogs.rs`.
- **Event pump after unload (LEAK #3) — FIXED.** The 50 ms playback pump
  returns `ControlFlow::Break` once the upstream `async_channel::Sender`
  is dropped, removing the GLib source. Enforced:
  `big-converter-ui/src/converter/ui/audio_edit_page/playback.rs`.

## Stage 7 — Performance budgets

- **Cold RSS ≤ 80 MiB** at 1 s. (no measurement yet — runtime gate
  pending baseline capture.)
- **Idle RSS drift ≤ +10 MiB over 60 s.** Same.
- **Idle wakeups ≤ 50 / s.** Same. Reducing the Stage 6 leaks brings
  this down materially.
- **FileChooser → queue p99 ≤ 250 ms.** Same.
- **Start → first ffmpeg stderr p99 ≤ 600 ms.** Same.

## Stage 8 — i18n

- **User-visible strings route through `i18n()` / `ni18n()`.** Current
  coverage 32 %; bare strings + dynamic format! calls bring the gap.
  Enforced by CI job `i18n-coverage` (greps `src/` for bare
  `set_label|set_title|...("...")`). To bypass intentionally, annotate
  with `// i18n-ignore` (e.g. units like `"s"`, `"%"`).
- **Const-array labels (~52 % of strings) are a deferred-by-design gap.**
  Resolution requires moving the arrays into a runtime-initialised structure.

## Stage 9 — Accessibility

- **No color-only state.** Buttons that change color also change label or
  icon.
- **No widget grade D (wrong role).** Audited. 0 found.
- **F-grade widgets (15).** Mostly hidden state switches in `sidebar/build.rs`.
  Fix proposed in Stage 9 artifact: add `.update_property(Label(...))` per
  switch.
- **EQ band Adjustments wrapped in Scale.** (no test yet — Stage 12 entry
  `eq_band_keyboard_focus`.)

## Stage 10 — User cognitive load

- **Destructive actions confirm.** `clear_queue`, `remove_item`,
  `clear_segments` show `adw::AlertDialog` before firing. Enforced:
  `big-converter-ui/src/converter/ui/window/{header_signals,build,queue_signals}.rs`
  and `edit_page/actions.rs`. Tested by
  `destructive_commands_match_invariant_list` (logical pin) — a GTK
  init test for the dialog itself remains deferred (Stage 12).
- **Empty states explain.** Every empty list has a hint sentence pointing
  to the action that populates it. (no test yet.)

## Stage 11 — Agent cognitive load

- **Files ≤ 700 LOC; functions ≤ 80 LOC.** No current violators
  (top file `edit_page/render.rs` is 586 LOC, under budget).
- **Doc anchors on `pub` items.** Currently 0 % coverage on the 31 public
  items. Action: add one-line `///` per item.
- **Module heads ≥ 5 lines.** A few `mod.rs` files still lack it.
- **No ambiguous suffix names.** `Manager`, `Handler`, `Helper`, `Util`,
  `Wrapper`, `Service`: 0 hits. Enforced by code review (greppable).
