# big-audio-effects

Reusable audio-effect contracts for the BigLinux media players.

This crate centralizes the small, hard-to-change pieces of an audio processing
chain so that `big-audio-player` and `big-video-player` produce identical
results from identical user input.

The values exported here are part of a public on-disk format: changing them
invalidates user presets and breaks PipeWire graphs.

## Modules

- `noise_reduction` — LADSPA plugin paths, control-port ordering and
  noise-reduction model identifiers.
- `equalizer` — the canonical 10-band equalizer center frequencies and labels.

## License

MIT
