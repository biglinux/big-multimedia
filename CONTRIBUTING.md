# Contributing

## Build and check

The toolchain is pinned in `rust-toolchain.toml`. Native dependencies: GTK 4.22,
libadwaita 1.9, libmpv and SQLite development files.

```sh
cargo build --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked --all-features
scripts/check-media-surface-imports.sh
```

Tests that create widgets need a Wayland display. Run them in an isolated
session, a headless compositor such as `kwin_wayland --virtual` with its own
D-Bus session bus, so no window reaches your desktop. Lifetime regressions are
`#[ignore]`d and run one at a time with `-- --ignored`.

Translations are one catalog for the whole suite in `crates/media-runtime/po`;
regenerate the template with `crates/media-runtime/po/regen-pot.sh`.

`clippy.toml` bans GTK, libadwaita and gtk-rs calls that leak or misbehave in
the pinned versions, each with its replacement. `docs/native-library-limits.md`
lists the native defects behind those bans and what the project still inherits.

## Resource lifetimes

A widget that is closed must finalize, and every GLib source it armed must be
removed. Process exit proves neither: check finalization with qdata destruction
in an isolated session. Weak notifications alone only prove disposal.

### Closures and captures

Timeline controllers obtain their widget from the signal argument. A strong
capture of the drawing area creates a widget → controller → closure → widget
cycle and retains the segment vector and output callbacks after window teardown.
`crates/ui/media/big-player-ui/tests/timeline_lifetime.rs` checks 25 lifetimes
at each of 0, 5 and 25 segments:

```sh
cargo test --locked -p big-player-ui --test timeline_lifetime -- --ignored
```

An entry and its accept button must not capture each other through validation
or apply callbacks. A shared validation closure holds weak widget references
when those same widgets keep the closure in their signal handlers. Open URL and
Go to Time validation callbacks keep the accept button weakly: the button's
submit handler already owns the entry.

Filter apply closures hold their adjustment or dropdown weakly: those controls
also own the apply handlers. Colorize's shared apply closure also holds its
hue, saturation and mix adjustments weakly; a weak expander alone does not
break adjustment → apply → adjustment. `filter_controls_release_the_backend`
checks 200 control finalizations and mpv ownership over 25 cycles.

The sleep-timer start handler must not clone `BigSleepTimerSetupPage`: that
aggregate owns the very button storing the handler. Capture weak controls and
action values. `wired_sleep_timer_pages_finalize` checks page finalization and
start values with 0, 5 and 25 choices.

The video sidebar keeps its list filter in the state that filter reads, so the
filter holds that state weakly. `closed_sidebar_releases_its_state` checks 25
sidebar closes; the test drops its own window reference too, since `destroy()`
only releases GTK's toplevel reference.

Application shutdown handlers registered by an audio shell are disconnected on
teardown. A weak backend capture still retains its `Arc` allocation until the
handler is released; the audio lifetime test checks the remaining weak count.

### GLib sources and late completions

Audio shell teardown removes its sleep-timer source even while the countdown is
armed: `closing_audio_releases_the_sleep_timer`.

Download queue teardown removes its progress source before the receiver
disappears: `dropping_download_panel_removes_progress_source` exercises 25
source lifetimes. Hiding a live panel must not cancel its progress updates.

Radio cache and search completions use the fallible input sender: component
shutdown while network I/O is pending is expected, not a panic. Radio teardown
removes its search debounce source.

Radio discovery and its disk cache load start on the first map of the radio
tab, not when the hidden component is constructed, and remapping does not
refetch. `radio_discovery_waits_for_first_map` checks hidden, first-map and
remap states; opening Music must not start Radio Browser discovery.

Radio recording directory creation and final-file validation run in a worker.
A generation token rejects preparation completed after stop or a newer
request; the source check rejects preparation for a station that is no longer
playing.

### Widgets GTK owns independently

Transient context menus use `big_gtk_kit::popover::auto_unparent_on_close`,
which detaches from an idle after the activated action has finished using its
ancestry. The idle retains the parent until detaching, so closing the owner in
that action cannot finalize it too soon.

The controls bar unparents its seek preview in `Drop`, before GtkStack disposes
its pages. A `destroy` handler is too late for a custom child of a GtkStack:
GTK 4.22 loops removing a child that is not a stack page.
`embedded_video_finalizes_after_close` checks 25 hosted teardowns.

Display CSS registered for a player or control uses
`big_gtk_kit::css::add_provider_for_widget_lifetime`. The display owns a
reference independently of the local provider variable, so the registration is
removed when its widget dies. Audio palette and high-contrast slots also remove
their registrations when the content root is destroyed: updating a cached slot
prevents stacking within one window but does not release it when the window
closes.

The reusable playlist window is hidden while its controller lives and
destroyed in component shutdown, including when it was never presented. GTK
owns a window reference independently of Relm4;
`hidden_playlist_window_finalizes_with_its_controller` checks 25 finalizations.

### Library workarounds

- Accessibility relations go through `big_gtk_kit::accessible::relate`; gtk-rs
  0.11's `update_relation` leaks its temporary target list. Keep the real
  relation: an explicit label alone does not name an empty dropdown.
- Password rows go through `big_gtk_kit::password::password_row`; libadwaita
  1.9 otherwise keeps the buffer, and the password in it, alive after the row.
- Audio source pages use `gtk::Stack`; libadwaita 1.9 `ViewStack` retains its
  transition animation after teardown.
- GMenu sections go through `big_gtk_kit::menu::append_shared_section`: GTK
  4.22's menu tracker keeps every section it shows, so a section is built once
  per process.
- `third_party/relm4` carries a Relm4 0.11 patch that prunes closed shutdown
  channels. Keep it until upstream does the same; a standalone player exit hides
  this process-lifetime registry leak.

### Blocking work and files

Downloads stream into `StagedFile` and publish by rename only on success. The
worker owns partial-file cleanup; the GTK progress tick never deletes files,
and panel teardown cancels active work even when no further tick can run.

Media-information file size uses GIO's asynchronous metadata query and a weak
preferences group: slow or remote filesystems must not block the GTK callback,
and closing the dialog while the query runs must not retain its widget tree.

## Measuring leaks

`scripts/leak-sweep.py` opens and closes the video player's dialogs through
AT-SPI actions and fails on GObjects retained per cycle. It extends the sweep
of gtk4-leak-guard; point `GTK4_LEAK_GUARD` at a checkout and run it inside an
isolated accessible session after building the player:

```sh
cargo build --locked -p big-video-player
GTK4_LEAK_GUARD=../gtk4-leak-guard python3 scripts/leak-sweep.py scripts/leak-surfaces/video.toml
```

`app-action` steps check that the action opens a new accessible surface. The
breakpoint allowance in `video.toml` covers the libadwaita defect only; never
widen it to hide retained dialog controls.

`scripts/leak-perf-dialogs.sh` measures memory growth across dialog open/close
cycles. It records the player's PID before exec and checks `/proc/PID/exe`
rather than finding the player by command line, and closes the actual surface
before sampling. Its CSV columns are phase, time, Anonymous KiB and Pss KiB.

`relation_probe` and `component_probe` in `big-relm4-components` are 5/25-cycle
heaptrack workloads. Build them with
`CARGO_PROFILE_RELEASE_DEBUG=true CARGO_PROFILE_RELEASE_STRIP=false` and run
`heaptrack --record-only` inside the isolated session. The players export
jemalloc, which bypasses heaptrack's preload: a capture with zero allocation
calls is invalid. Attribute allocations with a separate build that drops
big-os-kit's `jemalloc` feature, and measure real memory and CPU with the
production allocator.
