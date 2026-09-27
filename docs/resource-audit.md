# Memory and system-resource audit — 26–27 September 2026

This records the defects reproduced and corrected in Big Multimedia, and the
native-library limits that remain on this workstation. Baseline: `a492ec6`.
Every correction has its own commit; nothing was pushed or submitted upstream.
Raw evidence and diagnostic sources are in `~/.cache/big-multimedia-audit/`.
Upstream drafts and suggested patches are in `~/relatos-upstream/`.

## Results in the project

| Owner / defect | Evidence before → after | Commit |
| --- | --- | --- |
| Timeline controller captures its DrawingArea and segment state | segment Rc count 6 → 1; 75 widget finalizations per run at 0/5/25 segments | `a2ea256` |
| Open Disc validation captures its entry/dropdown | retained controls every close → none in three sweeps and 25 cycles | `9b5ea70` |
| Filter preset and adjustment callbacks | backend Arc count 4 → 1; control qdata finalization | `7a3d451` |
| Transient popover detached before its action finished | ancestry regression failed → 25 cycles pass, with deferred unparent | `7336dbf` |
| gtk-rs accessibility relation GList | 25−5 heaptrack retained 480 B / 20 lists → that site absent in three diffs | `05261a5` |
| PasswordEntryRow buffer creation reference | 0/75 → 75/75 buffers finalized at 0/5/25 characters, three repeats | `2acb169` |
| Relm4 global shutdown senders | 25−5 retained 20 channels and queue blocks (10,880 B) → growth removed in three diffs | `8af5020` |
| Audio sidebar AdwViewStack animation | 5 animations + targets retained per five lifetimes → zero with GtkStack, nine paired runs | `504c942` |
| Audio sleep timer survives its owner | armed Source still live → destroyed after component teardown | `208c1b0` |
| Download progress timer survives its panel | Source remains → 25 owned-source lifetimes pass | `a2c8a38` |
| Per-window application shutdown signal retains a weak backend block | backend weak count 1 → 0 | `af42920` |
| Seek preview attached as an arbitrary GtkStack child | hosted close spins inside GtkStack disposal → 25 hosted teardowns / 50 finalizations | `284d713` |
| Display CSS registrations outlive widgets | four video CssProviders per window → zero growth | `d0e3de0` |
| Hidden reusable playlist window never destroyed | 0/1 finalization → 25/25, three repeats | `8c294a7` |
| Audio palette/high-contrast providers remain on display | two CssProviders per window → zero growth in three hosted sweeps | `da6ccf9` |
| Downloads overwrite the destination before success and clean up on GTK thread | cancellation erased previous content → previous destination preserved; staging cleaned by worker at 0/5/25 bytes, success/cancel/HTTP failure | `9ef97d2` |
| Media-info synchronous file metadata | GTK callback performs stat → GIO async query; real 960,044-byte fixture displays 937.5 KB | `39f0cad` |
| Radio recording filesystem work in GTK callbacks | directory creation and final-file check moved to worker; late prepared result after Stop rejected in three isolated runs | `22d16c3` |
| Open URL / Go to Time entry ↔ accept-button cycle | three entries retained per three closes → zero with native focus defect isolated, three paired sweeps | `5cd95a9` |
| Sleep timer start handler captures its entire page | 0/75 → 75/75 pages finalized, three repeats at 0/5/25 choices; start values preserved | `5e4a2b8` |
| Colorize shared closure captures three adjustments | three adjustments per panel / backend Arc count 4 → no retained controls, Arc count 1; expanded test finalizes 200 controls | `6438652` |
| Radio completion arrives after shutdown; debounce survives | repeated closed-channel panics → zero across three five-cycle hosted runs; pending Source destroyed | `b6464af` |
| Hidden radio tab starts discovery on every audio window | query generation 1 before first map → 0; first map starts once, remap does not restart; three isolated runs | `4c61f05` |

Validation tooling was also corrected separately: stale translation expectations
(`76d2a52`), dialog/PID/memory observation (`bd7373f`), and two reviewed lifetime-lint
false positives (`fa9a302`). The dialog gate records its own exec PID, checks the
loaded executable, measures Anonymous/Pss, and closes the actual accessible role.
All five of its configured journeys completed five cycles after that correction.

## CPU, memory, file descriptors, threads and writes

A long-lived host opened and destroyed the embedded player, then stayed alive.
The old video teardown consumed **99.49%, 99.49%, 99.52% of one CPU** over three
60-second idle intervals after close. With the seek-preview fix the same journey
consumed **0%, 0.0167%, 0%**. The empty host control consumed 0%. These runs used
the same machine and OpenGL renderer and did not walk AT-SPI during the interval.
`write_bytes` did not change during any interval; no child remained at its end.

Before the lazy-radio change, the open idle audio/video windows measured 1.85%
and 0.82% CPU respectively in single 60-second observations. These single readings
are context, not an established before/after performance claim. Context-switch
counts, descriptors, threads, Pss and Anonymous are retained in `idle-summary.json`.

The three initial 25-cycle audio-host sweeps had no growing GObject types. Anonymous
slopes were 45.69, 85.55 and 80.10 KiB/cycle, below the existing dialog gate's
512 KiB/cycle threshold. This is not proof that all memory is freed. Descriptor
counts returned to 37–39 after those runs, and thread counts were 15–17. The native
video-host sweeps consistently retained exactly three GMenu and one AdwBreakpoint
per cycle; their causes and isolated native fixes are below.

The executable exports jemalloc allocation functions. Its default configuration
already uses zero dirty/muzzy decay; the actual override is `_RJEM_MALLOC_CONF`.
The 25-cycle diagnostic sweeps explicitly retained those settings, also setting
`MALLOC_CONF` and `MALLOC_TRIM_THRESHOLD_=0`. Pss was recorded but never used to
attribute shared-page fluctuations to a leak.

After deferring radio discovery, three further 25-cycle hosted audio sweeps again
had no GObject growth. Anonymous slopes were **47.00, 49.77 and 41.64 KiB/cycle**;
final descriptor counts were **37, 37, 36** and thread counts **14, 14, 14**.
Per-run writes were recorded, not treated as zero: audio state persistence still
occurs on open/close. These runs do not establish a substantial RAM reduction from
the lazy-radio change; its proven benefit is avoiding discovery work for a hidden tab.

The full-host heaptrack captures initially recorded **zero allocation calls**:
executable-defined jemalloc symbols bypassed LD_PRELOAD. Those captures are invalid
and were discarded as evidence. Separate release-debug host binaries were built
with the players' existing jemalloc feature temporarily disabled, then the original
manifests restored byte-for-byte. Production keeps jemalloc. Short/long pairs use
the same diagnostic allocator, OpenGL renderer and `MALLOC_TRIM_THRESHOLD_=0`.

Three final 25−5-cycle diffs reported:

| Diagnostic workload | Additional live bytes reported by heaptrack, decimal KB |
| --- | --- |
| Audio host | 61.20 / 64.28 / 61.89 |
| Video host, stock GTK/adwaita | 218.75 / 217.00 / 216.19 |
| Plain native-window control, no player | 57.43 / 57.82 / 56.75 |

About 58–62 KB in each workload comes from `wl_display_read_events` in
libwayland-client. The remaining video stacks include GMenu hash tables/attributes
and breakpoint signal closures, consistent with the native censuses. Audio's
remaining differences include native thread-local storage and small GTK allocations;
no repeatable growing project-owned channel site was identified. These numbers
are allocator-specific attribution evidence, not production footprint comparisons
and not a claim that the native residual is fixed. The plain-window control still
retains it, so no speculative project workaround or upstream patch was invented.
The captures and full stacks are `heap-final-{audio,video,control}-{1,2,3}-diff.txt`.

Video hosted teardown also emits GTK's `Invalid inhibitor cookie` warning. The
native Wayland backend removes a window-only inhibitor in `window_removed` before
the component's later Drop attempts `uninhibit`. Source inspection confirms cleanup
has already happened; this warning is recorded, not counted as a surviving inhibitor.

## Native libraries: still relevant on the installed system

| Native defect | Isolation / result | Draft patch in `~/relatos-upstream/` |
| --- | --- | --- |
| GTK 4.22.4 menu tracker fails to release unmerged section links | reftrace names section creation; native C qdata probe 0/75 → 75/75 in three runs; patched GTK removes the three GMenu/window in the real host | `gtk4-menu-tracker-section-reference.patch` and companion `.md` / repro `.c` |
| libadwaita 1.9.3 breakpoint array lacks an element destructor | one breakpoint per video host window; three per ordinary adaptive dialog; four per preferences dialog | `libadwaita-1-breakpoint-bin-leak.patch` |
| libadwaita ViewStack animation remains alive | direct sidebar replaced; native preferences windows/dialogs still construct their own ViewStack | `libadwaita-3-view-stack-animation-leak.patch` |
| Native dialog focus restoration retains a closed sheet subtree | stock-library URL/time sweep still retains controls after the project cycle fix; same executable with the dialog-host patch retains none | `libadwaita-1.9.3-dialog-focus.patch`; underlying GTK proposal `gtk-4.22.4-pending-focus.patch` |

GTK 4.22.4 and libadwaita 1.9.3 were built in the audit cache. The menu, breakpoint,
ViewStack and dialog-host patches were tested using `LD_LIBRARY_PATH` **only inside
isolated sessions**. No system library was installed or replaced. Process maps
identify the loaded cache libraries. The underlying GTK pending-focus patch was
not applied in this audit; the tested focus fix is in libadwaita.

Clippy bans new `Menu::append_section` calls. Eight existing calls preserve current
menu grouping under explicit, documented exceptions; they still require the native
GTK fix. The ban prevents expansion of this known defect, not its disappearance.
Accessibility relations, password construction and ViewStack have their own bans
and project replacements. The Relm4 registry patch is vendored locally and prunes
on the next launch, leaving at most the final closed sender until then or exit.

Do not widen the sweep allowance to conceal retained controls. The maintained
video gate permits only three native AdwBreakpoint objects per cycle; it can still
fail on the installed native focus bug. A clean patched-library run does not mean
the workstation's stock libraries are fixed.

## Reproduce and read the evidence

Build the players, then run the maintained video sweep in isolation:

```sh
cargo build --locked -p big-audio-player -p big-video-player --bins
/note/bigdesktop/scripts/headless-gate.sh --accessible python scripts/leak-sweep.py scripts/leak-surfaces/video.toml
```

Run ignored lifetime tests individually in an isolated GTK session/home. Their
names are recorded in `AGENTS.md`; they prove finalization or Source destruction,
not just successful window close. `relation_probe` and `component_probe` are the
small release-debug heaptrack workloads for the two non-widget ownership defects.

The final native gate (`native-closure/results.tsv`) passed all 11 checks:
static review, formatting, gettext, both players/all targets, foundation/data/render
policy/runtime/player tests, optional features, and Clippy. Additional audio-panel
tests and Clippy passed. `leak_lint.py crates` scanned 448 files with zero errors
and zero warnings. The modified sweep wrapper passes Ruff; the shell gate passes
bash syntax, ShellCheck and shfmt. The initial gate had one stale translation-label
failure; it was fixed and the complete final gate rerun after the I/O changes.

The pre-existing `scripts/review-check.py` still has three Ruff findings and a
formatting mismatch; they are unrelated to its corrected translation expectations.
Its own static checks pass. No broad reformatting was mixed into this audit.

The nine video panel journeys passed three complete three-cycle repetitions with
stock libraries after the colorize/sleep-page fixes. The native patch set produced
clean sweeps for five audio dialogs and nine video dialogs (three cycles each),
including preferences, appearance, sleep timer, URL, disc, time, IPTV and media
information. These checks do not include the additional menu-only dialogs below.

## Coverage limits

Source review and sweeps cover the main audio/video hosts, common dialogs and the
video settings panel pages. An action that opens an inline panel must be measured
as a panel, not accepted as a missing dialog. The cache's `panel_sweep.py` verifies
its visible title and its disappearance on close. Initially using the window-only
driver for these actions produced errors; those were observer failures, not passes.

The local-media sweep cannot exercise yt-dlp quality because that action is disabled
for a local file. The action-only driver did not expose cast/smartphone/playlist-management
menu entries. A separate inspector could open Main Menu, but the subsequent
menu sweep failed to resolve its accessible tree consistently; those attempts
are recorded as observer failures, not successful coverage. Music Folders and
About were also not certified by that menu sweep. External devices, network streams, converter
handoffs, production desktop activation and hardware decoding were not certified
by this audit. No cold-page-cache/renderer A/B startup claim is made. Settings writes
remain on their existing background/coalescing path; their durability contract was
not weakened merely to lower a counter.

This report distinguishes project fixes from native-library retention and bounded
process state. It does not claim every allocation, renderer cache or asynchronous
external workload has been eliminated.
