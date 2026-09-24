# big-media-engines

Display-free media helpers shared by the BigLinux media players. The crate
forbids `unsafe` and has no GTK dependency.

## Modules

- `media_files` — audio/video extension sets and path predicates.
- `queue` — the play-queue model (order, shuffle, repeat).
- `radio_browser` (feature `radio-browser`) — Radio-Browser API response parser.

Playlist and subtitle file parsing lives with its users in `big-media-runtime`
(`media::playlist_scan`, `media::iptv`).

## License

MIT
