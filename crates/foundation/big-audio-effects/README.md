# big-audio-effects

Reusable audio-effect contracts for BigLinux Rust media tooling.

This crate centralizes the small, hard-to-change pieces of an audio processing
chain so that media players (`big-audio-player`, `big-video-player`), converters
(`big-audio-converter`, `big-video-converter`) and PipeWire-side tooling produce
identical results from identical user input.

The values exported here are part of a public on-disk format: changing them
invalidates user presets and breaks PipeWire graphs.

## Modules

- `noise_reduction` — LADSPA plugin paths, control-port ordering and
  noise-reduction model identifiers.
- `equalizer` — the canonical 10-band equalizer center frequencies, labels and
  presets.
- `dynamics` — derived gate and compressor curves plus FFmpeg filter-string
  formatters.

## Example

```rust
use big_audio_effects::dynamics::ffmpeg_acompressor_filter;

let filter = ffmpeg_acompressor_filter(0.5);
assert!(filter.starts_with("acompressor="));
```

## License

MIT
