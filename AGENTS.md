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
