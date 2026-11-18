# Contributing

## Build and check

The toolchain is pinned in `rust-toolchain.toml`. Native dependencies: GTK 4.22,
libadwaita 1.9, libmpv and SQLite development files.

```sh
cargo build --workspace --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --locked --all-features
scripts/check-media-surface-imports.sh
scripts/check-no-shell-invocations.sh
```

`check-no-shell-invocations.sh` refuses `sh -c` and `bash -c` in every crate's
`src/`: subprocesses take an argv array, never a shell command string.

Tests that create widgets need a Wayland display. Run them in an isolated
session, a headless compositor such as `kwin_wayland --virtual` with its own
D-Bus session bus, so no window reaches your desktop. Lifetime regressions are
`#[ignore]`d and run one at a time with `-- --ignored`, inside that session.
They open the settings, library and cache of whatever home they get, so point
`HOME` and the XDG directories at a scratch directory and keep cargo's own:

```sh
scratch="$(mktemp -d)"
mkdir "$scratch/home"
isolated_home=(
	CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
	HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data"
	XDG_CACHE_HOME="$scratch/cache" XDG_STATE_HOME="$scratch/state"
)
env "${isolated_home[@]}" cargo test --locked -p big-video-player embedded_video_finalizes_after_close -- --ignored
env "${isolated_home[@]}" cargo test --locked -p big-audio-player closing_audio_releases_the_sleep_timer -- --ignored
env "${isolated_home[@]}" cargo test --locked -p big-player-ui --features audio-panels radio_discovery_waits_for_first_map -- --ignored
env "${isolated_home[@]}" cargo test --locked -p big-player-ui --features audio-panels radio_teardown_cancels_pending_search -- --ignored
```

### Sound server and cast daemon tests

Two runtime tests need services of their own. Never run them in your desktop
session: they must not reach your sound devices or your session bus. Both use
`isolated_home` from above and a bus whose only activatable service is the one
in `$scratch/services`:

```sh
mkdir "$scratch/services"
cat >"$scratch/bus.conf" <<EOF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir=$scratch</listen>
  <auth>EXTERNAL</auth>
  <servicedir>$scratch/services</servicedir>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
EOF
```

`native_sound_server_volume_tracks_external_changes` plays through mpv's
`pulse` and `pipewire` outputs and changes the stream with `pactl`; it needs
FFmpeg. It runs against a private PipeWire, WirePlumber and pipewire-pulse
under a scratch `XDG_RUNTIME_DIR`. WirePlumber's `policy` profile loads no
ALSA, Bluetooth or camera monitor, so the only sink is a null sink; the script
stops before the test when `pactl` sees any other sink:

```sh
mkdir -m 700 "$scratch/runtime"
mkdir -p "$scratch/config/pipewire/pipewire.conf.d"
cat >"$scratch/config/pipewire/pipewire.conf.d/null-sink.conf" <<'EOF'
context.objects = [
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = test-null-sink
            media.class = Audio/Sink
            audio.position = [ FL FR ]
            monitor.channel-volumes = true
        }
    }
]
EOF
cat >"$scratch/audio.sh" <<'EOF'
pipewire &
wireplumber --profile policy &
pipewire-pulse &
trap 'kill $(jobs -p)' EXIT
for _ in {1..50}; do pactl info >/dev/null 2>&1 && break; sleep 0.2; done
pactl list short sinks |
	awk '$2 != "test-null-sink" && $2 != "auto_null" { bad = 1 } END { exit bad || NR == 0 }'
BIG_MEDIA_TEST_AUDIO_ISOLATED=1 cargo test --locked -p big-media-runtime --features ui-core \
	--lib native_sound_server_volume_tracks_external_changes -- --ignored
EOF
env -u PULSE_SERVER -u PIPEWIRE_REMOTE -u DISPLAY -u WAYLAND_DISPLAY "${isolated_home[@]}" \
	XDG_RUNTIME_DIR="$scratch/runtime" PIPEWIRE_RUNTIME_DIR="$scratch/runtime" \
	dbus-run-session --config-file "$scratch/bus.conf" -- bash -e "$scratch/audio.sh"
```

`daemon_exposes_player_contract` checks that D-Bus activates the BigNetScreen
daemon you name, not the installed one. Set `daemon` to the absolute path of a
`bignetscreend` build, without spaces, and give the bus a service file for it:

```sh
daemon=/path/to/bignetscreend
cat >"$scratch/services/br.com.biglinux.BigNetScreen.Service.service" <<EOF
[D-BUS Service]
Name=br.com.biglinux.BigNetScreen.Service
Exec=$daemon
EOF
env "${isolated_home[@]}" BIG_MEDIA_TEST_BNS_DAEMON="$daemon" \
	dbus-run-session --config-file "$scratch/bus.conf" -- \
	cargo test --locked -p big-media-runtime --features ui-core --lib daemon_exposes_player_contract -- --ignored
```

The tray icon test plays the tray host on a session bus of its own:

```sh
dbus-run-session -- cargo test --locked -p big-app-kit tray -- --ignored
```

`undo_restores_latest_trashing` trashes and restores through GVfs, which
must see the scratch home instead of your trash, so the bus activates only
GVfs:

```sh
mkdir -p -m 700 "$scratch/runtime"
ln -s /usr/share/dbus-1/services/org.gtk.vfs.*.service "$scratch/services/"
env -u DISPLAY -u WAYLAND_DISPLAY "${isolated_home[@]}" XDG_RUNTIME_DIR="$scratch/runtime" \
	dbus-run-session --config-file "$scratch/bus.conf" -- \
	cargo test --locked -p big-media-runtime --features ui-core --lib undo_restores_latest_trashing -- --ignored
```

### FAT and exFAT publication

FAT and exFAT refuse hard links, so `StagedFile::publish` falls back to a
no-replace rename there. `fat_publish_without_hard_links` checks that against a
real mount and fails unless `BIG_OS_KIT_FAT_DIR` names a directory on vfat or
exfat. A memory stick mounted with `udisksctl mount -b DEVICE` will do; make an
empty directory on it for the run and remove it afterwards. The video move and
podcast download tests write under `TMPDIR`, so the same directory runs them on
that filesystem; the FIFO test is skipped because FAT has no FIFOs:

```sh
dir=/run/media/$USER/LABEL/big-multimedia-test
BIG_OS_KIT_FAT_DIR="$dir" cargo test --locked -p big-os-kit --lib fat_publish_without_hard_links -- --ignored
TMPDIR="$dir" cargo test --locked -p big-video-player --lib fileops -- --skip cross_device_move_hides_partial_copies
TMPDIR="$dir" cargo test --locked -p big-player-ui --features audio-panels --lib downloads_publish_only_complete_files
```

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
submit handler already owns the entry. A dialog that asks for one name uses
`big_relm4_components::layout::name_prompt::prompt_name`;
`closed_prompts_finalize_their_entry_and_buttons` checks 50 closes.

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
removed when its widget dies. CSS rewritten while the widget lives (palettes,
high contrast, the idle background) goes in a
`big_relm4_components::theme::CssSlot`, which registers once on that kit
function and replaces its rules in place: a cached provider prevents stacking
within one window but is not released when the window closes.

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
- Dialogs and alerts come from `big_gtk_kit::dialog::Dialog` and
  `big_gtk_kit::alert::Alert`, which present a closed one again: libadwaita
  1.9 keeps three breakpoints for every dialog it presents. A handler
  connected through their deref, other than an alert's `response`, stays for
  the next opening: connect `closed` through their own method and other
  signals on the content, as `focus_when_shown` does for an entry.
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
