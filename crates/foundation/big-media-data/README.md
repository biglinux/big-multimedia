# big-media-data

GTK-free data services of the media players: the SQLite music library, the
settings store, and the radio, podcast, lyrics and metadata clients.

## Entry points

- `settings::Settings`: each player's `settings.conf`, opened with
  `Settings::for_audio()` or `Settings::for_video()` and read through the typed
  keys in `settings::keys`. [Settings and environment](../../../docs/settings.md)
  documents the file and every key.
- `db` (feature `db`): the SQLite library database, its migrations and queries;
  `db::open_database()` opens it.
- `library` (feature `library`): folder scanning, cover art, MusicBrainz
  metadata, LRC lyrics, podcast feeds, and the account credentials in the
  keyring (`library::accounts`).
- `radio` (feature `radio`): the Radio Browser API client and server discovery.
- `media_probe`: ffprobe's JSON report on a media file.

All three features are on by default.

## Where it fits

`big-player-ui` uses it for its panels, and `big-media-runtime` re-exports it
as `config`, `db`, `error`, `i18n`, `library` and `settings`, so the players
reach it through the runtime. It builds on `big-os-kit` for files, HTTP and the
keyring, and links neither GTK nor mpv.

## Rules for callers

- Database, scan, network, keyring and ffprobe calls block: run them on a
  worker, never on the GTK main loop.
- `Settings` reads from memory and saves on the `big-os-kit` background lane,
  without fsync. A settings file that fails to parse is never overwritten.

## Validate a change

```sh
cargo test --locked -p big-media-data --all-features
```

## License

MIT
