# Converter Screen Contract

## Converter Job Recovery

- User goal: see and control conversions that are still owned by `big-convertd`
  after closing or reopening the converter UI.
- Domain objects: daemon job snapshot, per-file conversion detail, progress row,
  restart command, cleanup acknowledgement.
- Source of truth: `big-convertd` `List`/`Attach`/`Restart`/`AckCleanup`
  responses over `big_os_kit::microdaemon`.
- Startup state: the window starts with local settings loaded, then checks the
  daemon asynchronously. It must not claim a recovered job exists until `List`
  returns one.
- Active job: show the progress page reconstructed from daemon details and keep
  polling `Attach`.
- Interrupted or failed job: show the daemon snapshot on the progress page and
  offer a dialog action to restart through `Restart`; the daemon decides whether
  a persisted restart spec is safe.
- Terminal job cleanup: when the user leaves the progress page after a terminal
  daemon job, send `AckCleanup`. Failure is logged and the visible local page
  still returns to the queue.
- Non-goals: reconstructing editable queue settings from daemon history,
  restarting destructive conversions that the daemon marks non-restartable,
  or duplicating daemon journal policy in the UI.
