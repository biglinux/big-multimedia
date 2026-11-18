# big-os-kit

GTK-free OS contracts for the BigLinux media crates: subprocesses, atomic
files, background writes, the keyring and XDG paths.

## Entry points

- `subprocess::BigSubprocessSpec`: an argv-form command with a timeout and a
  capture policy, built with `BigSubprocessSpec::builder()`.
- `storage`: atomic private writes (`atomic_write_private`) and bounded reads
  of regular files (`read_regular_limited`).
- `background` and `deferred`: the bounded serial lanes that write state off the
  GUI thread, and a JSON document whose save status a window can show.
- `keyring`: Secret Service access through the system `secret-tool`.
- `http_client`, `url`, `xdg_dirs`: blocking HTTP over the system curl, URL
  encoding and XDG base directories.

The crate root's rustdoc lists every module.

## Where it fits

It is the leaf of the workspace and links no GTK. `big-app-kit-core`,
`big-media-data`, `big-player-ui`, `big-media-runtime` and both players depend
on it; `big-app-kit` re-exports `subprocess` and `storage` unchanged. The
workspace map is in the root [README](../../../README.md#architecture).

## Rules for callers

- Call `keyring` and the blocking `http_client` only from a worker, never from
  the GTK main loop.
- Closures sent to `background` must not capture GTK objects; deliver results
  through weak references. [Persistence and recovery](../../../docs/persistence-and-recovery.md)
  states the full contract.
- `atomic_write_private` does not fsync. Use `atomic_write_private_synced` only
  where crash recovery depends on write order.
- Feature `jemalloc` makes jemalloc the allocator of the whole process. Only an
  executable enables it, and it must export the allocator symbols (see the
  players' `build.rs`).

## Validate a change

```sh
cargo test --locked -p big-os-kit --all-features
```

## License

MIT
