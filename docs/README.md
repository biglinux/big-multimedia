# Documentation

Guides for people who run, configure or change the Big Multimedia players.

## Using and configuring the players

- [Settings and environment](settings.md): where `settings.conf` lives, its
  format, every key with its default, and the environment variables.
- Command-line options: [Big Video Player](../crates/big-video-player/README.md#command-line)
  and [Big Audio Player](../crates/big-audio-player/README.md#command-line).

## Changing the code

- [CONTRIBUTING](../CONTRIBUTING.md): build, checks and resource-lifetime rules.
- [Persistence and recovery](persistence-and-recovery.md): save lanes, account
  credentials, the LAN remote and library scans.
- [Native library limits](native-library-limits.md): GTK and libadwaita defects
  and the `clippy.toml` bans that keep them out.
- [Media runtime architecture](../crates/media-runtime/ARCHITECTURE.md): the
  engine's module map and subprocess rules.

`screenshots/` holds the images the root README shows.
