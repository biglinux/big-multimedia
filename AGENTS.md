# Resource lifetime checks

Timeline controllers must obtain their widget from the signal argument. A strong
capture of the drawing area creates a widget → controller → closure → widget
cycle and retains the segment vector and output callbacks after window teardown.

Run the finalization regression in an isolated GTK session:

```sh
/note/bigdesktop/scripts/headless-gate.sh cargo test --locked -p big-player-ui --test timeline_lifetime -- --ignored
```

The test uses qdata destruction to check finalization, with 25 lifetimes at each
of 0, 5 and 25 segments. Weak notifications alone only prove disposal.

An entry and its accept button must not capture each other through validation
or apply callbacks. A shared validation closure must hold weak widget references
when those same widgets keep the closure in their signal handlers.

After building the video player, sweep its dialog lifetimes with:

```sh
/note/bigdesktop/scripts/headless-gate.sh --accessible python scripts/leak-sweep.py scripts/leak-surfaces/video.toml
```

`app-action` checks that the action actually opens a new accessible surface.
The libadwaita breakpoint allowance does not cover retained dialog controls.

Filter apply closures must hold their adjustment/dropdown weakly: those controls
also own the apply handlers. `filter_controls_release_the_backend` checks 75
control finalizations and mpv ownership over 25 cycles in an isolated session.

Transient context menus use `auto_unparent_on_close`: detach from an idle, after
the activated action has finished using its ancestry. The idle retains the parent
until detaching, so closing the owner in that action cannot finalize it too soon.

Widget accessibility relations use `accessible::relate`; gtk-rs 0.11's
`update_relation` leaks its temporary target list and is banned by Clippy.
The `relation_probe` example provides a 5/25-cycle heaptrack workload. Build it
with `CARGO_PROFILE_RELEASE_DEBUG=true CARGO_PROFILE_RELEASE_STRIP=false` and
run `heaptrack --record-only` inside the isolated session. Keep the real
accessible relation: an explicit label alone does not name an empty dropdown.

Credential forms build password rows through `input::password_row::password_row`.
The libadwaita 1.9 buffer creation reference otherwise survives the row, including
its password. The isolated qdata test checks 75 finalizations at lengths 0, 5 and
25; the direct constructor and builder are banned outside the helper.

Keep the Relm4 0.11 patch in `third_party/relm4` until upstream prunes closed
shutdown channels. `component_probe` provides the isolated 5/25-cycle heaptrack
comparison; a standalone player exit hides this process-lifetime registry leak.

Use `gtk::Stack` for audio source pages. libadwaita 1.9 `ViewStack` retains its
transition animation after teardown; Clippy bans the leaking type. Isolated
5-cycle comparisons at 0, 5 and 25 children must leave no animations alive.

Audio shell teardown removes its sleep-timer source even when the countdown is
still armed. Run `closing_audio_releases_the_sleep_timer` with `--ignored` in an
isolated accessible session/home; process exit alone cannot prove cancellation.

Download queue teardown removes its progress source before the receiver disappears.
`dropping_download_panel_removes_progress_source` exercises 25 source lifetimes in
an isolated GTK session; hiding a live panel must not cancel its progress updates.

Application shutdown handlers registered by an audio shell must be disconnected
on teardown. A weak backend capture still retains its Arc allocation until the
handler is released; the audio lifetime test checks the remaining weak count.

The controls bar unparents its seek preview in `Drop`, before GtkStack disposes
its pages. A Widget::destroy handler is too late for a custom child attached to
a GtkStack: GTK 4.22 loops removing a child that is not a stack page. Run the
isolated `embedded_video_finalizes_after_close` test to prove 25 hosted teardowns.

Display CSS registered for a player or control uses
`theme::add_provider_for_widget_lifetime`. The display owns a reference independently
of the local provider variable: remove that registration when its widget dies.
`scoped_providers_finalize_with_their_widgets` checks 25 provider finalizations.

The reusable playlist window is hidden while its controller lives and destroyed
in component shutdown, including when it was never presented. GTK owns a window
reference independently of Relm4; `hidden_playlist_window_finalizes_with_its_controller`
checks 25 qdata finalizations instead of assuming that dropping the controller suffices.

Audio palette and high-contrast slots also remove their display registrations
when the content root is destroyed. Updating a cached slot prevents stacking
within one window, but does not release it when the whole window closes.

The dialog resource gate records the PID before exec and verifies `/proc/PID/exe`;
never locate the player by command-line pattern. Its CSV columns are phase, time,
Anonymous KiB and Pss KiB. Preferences expose a dialog role; appearance/equalizer
expose window roles. Close the actual surface before sampling its retained memory.

Downloads stream into `StagedFile` and publish by rename only on success. The
worker owns partial-file cleanup; the GTK progress tick never deletes files.
Panel teardown cancels active work even when no further tick can run. The transfer
regression covers success, cancellation and HTTP failure at 0, 5 and 25 bytes,
including preservation of a previous destination and removal of staging files.

Media-information file size uses GIO's asynchronous metadata query and a weak
preferences group. Slow or remote filesystems must not block the GTK callback;
closing the dialog while the query runs must not retain its widget tree.

Radio recording directory creation and final-file validation run in a worker.
A generation token rejects preparation completed after stop or a newer request;
the source check rejects preparation for a station that is no longer playing.
The isolated audio lifetime test delivers a late completion after cancellation.

Open URL and Go to Time validation callbacks keep the accept button weakly:
the button's submit handler already owns the entry. The video sweep covers both
paths. On GTK 4.22.4/libadwaita 1.9.3, also apply the isolated diagnostic dialog-focus
patch to distinguish this project cycle from the native focus-tree retention;
do not allow retained entry controls in the stock-library gate.

The sleep-timer start handler must not clone `BigSleepTimerSetupPage`: that
aggregate owns the very button storing the handler. Capture weak controls and
action values. `wired_sleep_timer_pages_finalize` proves 75/75 page finalizations
and start values with 0, 5 and 25 choices; the audio dialog sweep must retain no
controls (native AdwBreakpoint retention is tracked separately).

Colorize's shared apply closure also holds its hue/saturation/mix adjustments
weakly. A weak expander alone does not break adjustment → apply → adjustment.
The filter lifetime test includes the color group and checks 200 finalizations
and the backend's strong reference count across 25 cycles.

Radio cache/search completions use the fallible input sender: component shutdown
is expected while network I/O is pending, not a panic. Radio teardown removes its
search debounce source. The isolated source-lifetime test and rapid hosted audio
open/close run cover these late-delivery paths.

Radio discovery and its disk cache load start on the first map of the radio tab,
not when the hidden component is constructed. Remapping must not refetch. The
isolated `radio_discovery_waits_for_first_map` regression checks hidden, first-map
and remap states; opening Music must not launch Radio Browser discovery.
