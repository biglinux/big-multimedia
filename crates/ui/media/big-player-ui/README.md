# big-player-ui

Relm4 panels the two players share: appearance and audio preference pages, the
cut timeline, and the audio player's radio, podcast, lyrics and download panels.

## Entry points

- `appearance` and `audio_preferences`: preference pages both players mount.
- `cut_timeline`: the cut-marking timeline, drawing and pointer handling.
- `media_list`: the recycling list the radio and podcast panels use.
- Feature `audio-panels`: `playback_bar`, `radio_panel`, `podcast_panel`,
  `lyrics_panel`, `download_panel`, `file_browser`, `smartphone` (the phone
  remote dialog) and the audio trim bar. Only the audio player enables it.

## Where it fits

Each panel is a Relm4 `Component` that owns its widgets and emits typed
intents. The player's shell owns the queue, mpv, the database and the settings,
and pushes display state back. The crate uses `big-media-data`, `big-app-kit`,
the component crates and `big-os-kit`, and never `big-media-runtime`, so no
panel can reach mpv.

Panels follow the resource-lifetime rules in
[CONTRIBUTING](../../../../CONTRIBUTING.md#resource-lifetimes): a closed panel
finalizes, and every GLib source it arms is removed.

## Validate a change

```sh
cargo test --locked -p big-player-ui --all-features
```

The `#[ignore]`d lifetime tests need an isolated Wayland session; CONTRIBUTING
shows how to run them.

## License

MIT
