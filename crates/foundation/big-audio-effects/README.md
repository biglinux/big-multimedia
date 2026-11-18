# big-audio-effects

Reusable audio-effect contracts for the BigLinux media players.

This crate centralizes the small, hard-to-change pieces of an audio processing
chain so that `big-audio-player` and `big-video-player` produce identical
results from identical user input.

Saved settings hold equalizer gains by band index and the noise-reduction
model by its index in `MODELS`, so reordering either changes what a user's
saved settings mean. The plugin paths, labels and control indexes must match
the installed LADSPA plugins: mpv's `af` chain loads them by these names.

## Modules

- `noise_reduction` — LADSPA plugin paths, control-port ordering and
  noise-reduction model identifiers.
- `equalizer` — the canonical 10-band equalizer center frequencies and labels.

## Validate a change

```sh
cargo test --locked -p big-audio-effects
```

## License

MIT
