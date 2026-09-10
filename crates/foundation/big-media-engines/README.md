# big-media-engines

Pure-Rust media parsers used by BigLinux apps.

Display-free, panic-free parsers for the media container formats used by the
BigLinux media stack. The crate forbids `unsafe` and performs no I/O.

## Modules

- `subtitle` — SubRip (`.srt`) cues; see `subtitle::parse_srt`.
- `playlist` — M3U / M3U8 playlists; see `playlist::parse_m3u`.

## Design

Every parser accepts an explicit `ParseLimits` value so hostile inputs cannot
drive the process out of memory. All errors are reported as typed enums; no
parser ever panics on untrusted input.

## Example

```rust
use big_media_engines::ParseLimits;
use big_media_engines::subtitle::{self, SubtitleFormat};

let bytes = b"1\n00:00:01,000 --> 00:00:02,000\nHello\n";
let cues = subtitle::parse(SubtitleFormat::SubRip, bytes, ParseLimits::SUBTITLE)
    .expect("valid SRT");
assert_eq!(cues.len(), 1);
```

## License

MIT
