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
