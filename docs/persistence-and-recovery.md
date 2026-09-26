# Persistence, credentials and recoverable operations

## Owners and thread boundaries

`big-os-kit::background` has a bounded, serial lane for short disk transactions
(64 queued tasks) and a separate serial account/service lane (8 tasks). Admission
uses `try_send`. A widget disappearing does not discard an admitted write. GTK
objects never enter the worker closure; completions use weak widget references.
The existing `Settings` store snapshots its in-memory state under a short lock,
then releases it before filesystem operations. A separate disk lock serializes
transactions. A newer local generation is never cleared by an older completion.

Independent JSON state uses `deferred::Document` with the exact reviewed bytes.
The same cooperating-writer lock covers compare and publication. Invalid source,
external conflict, disk error and publication without confirmed directory sync
remain observable states. Retry does not implicitly rebase or overwrite an
external revision. Resolve a conflict by reviewing/reloading the application;
export unsaved preferences to a new file before restarting when they must be kept.
A failed save is not made successful merely because the worker finished.

The UI shows Saving/Saved/Failed/DurabilityPending and Retry on the relevant
preferences/state surfaces. Main-store settings, generic JSON state, IPTV provider
state, favorites, recents, plugin live control values, preference/profile exports
and playlist exports no longer perform their durable disk write in the GTK
callback. Snapshot construction and widget updates remain on GTK. Initial loading,
media streaming and every unrelated filesystem operation are not claimed to be
asynchronous. User document exports use staged no-replace publication rather than
a universal private-mode overwrite policy.

Normal window closing and requested quit/suspend/poweroff drain admitted jobs.
Scans own a drain guard until terminal completion, not merely until the UI
requests cancellation. Forced termination, kernel calls stuck in I/O, panic=abort
and power loss are not successful-save guarantees. Hosts bypassing these window/
quit paths must honor the same drain contract. Error states remain errors.

## Secret Service account protocol

The bridge uses the system `secret-tool` from libsecret on a service worker.
Nothing is installed automatically. The default persistent collection is used;
locked/unavailable/denied/cancelled/timeout is an error, never permission to save
plaintext. `secret-tool` cannot reliably distinguish a missing item from a locked
or failed lookup by exit code alone. Helper output/error is never logged, and the
secret goes on stdin, not argv or lookup attributes. Payload is capped at 7,900
UTF-8 bytes, below the helper's current 8,192-byte input buffer.

Last.fm and ListenBrainz use a fresh random candidate ID per update. A small
private journal stores only provider, identifiers, source-revision hash, intended
payload hash and existing listening-history consent. It is durably written before
calling the service. Store is followed by exact readback; only then is the account
JSON replaced by a versioned reference. The old keyring item is cleaned only after
the reference/readback/durability checks. Legacy plaintext is not erased early.
No claim is made that replacing the file securely erases old filesystem blocks or
user backups.

Protect/Check/Configure/Recover actions are available in audio preferences. A
credential editor uses password rows, explicit Save, reviewed account revision
and a separate worker. Existing consent is preserved; protecting credentials does
not implement or silently enable unavailable scrobbling features. A pending
transaction cannot be replaced by a different edit. Recover replays that exact
transaction; if the helper failed before creating its candidate, manually reenter
exactly the intended credentials. A different payload is refused. Old-item
cleanup uncertainty retains the journal and requires recovery/explicit inspection,
not speculative deletion. Keyring and JSON are not one ACID resource.

This bridge covers Last.fm and ListenBrainz account credentials, not arbitrary
IPTV provider URLs or all credentials anywhere in the desktop.

## LAN remote

One nonblocking server thread owns at most 16 accepted sockets and 4 per IP.
Headers are limited to 8,192 bytes/48 fields and an absolute 3-second deadline
from acceptance. A slow drip never resets it. Responses have a 2-second write
budget and bounded chunks; one slow peer cannot hold a per-client thread.
Only complete HTTP/1.1 frames with required Host and unambiguous framing are
accepted; bodies, transfer encoding, duplicate headers and Expect are refused.
One response closes the connection. Additional/pipelined data are not dispatched.

Exact random token, POST, the custom command header and origin restrictions remain
mandatory. Authenticated commands are limited to 8/IP/second, 24 globally/second;
rate history holds at most 64 peers. UI command admission is separately bounded
and checks the same revoked flag again on GTK. Turning off/closing the dialog
revokes immediately and wakes the loop; is_stopped is true only after listener and
accepted clients close. A successful HTTP admission is not confirmed playback.
HTTP remains plaintext, IPv4, opt-in and for a trusted LAN. This is not TLS,
Internet remote access, pairing or universal denial-of-service protection.

## Unavailable files and scanning

Library menu → Review unavailable files scans a fixed ID snapshot in bounded
pages: at most 2,048 examined records and 100 review entries per page. New imports
are included only after Restart review. Only NotFound entries are selectable;
permission failures, nonregular paths and remote items are shown but not removable.
There is no automatic selection. The dialog shows ratings/playlist impact, asks
for confirmation with Cancel as default, and rechecks the filesystem plus the
exact row and playlist/queue relations inside the removal transaction.

Removal deletes only selected database records/links, never media files and never
the current in-memory playback queue. A restored file, altered rating/relation,
cancellation or query error cancels the selection before commit. An ambiguous
commit result is reported as unconfirmed and requires checking again. A filesystem
can change after the final check; this is not an atomic transaction spanning
SQLite and removable devices.

At most two scans are admitted globally. Directory iteration, DB-lock waits and
metadata/album-art probes check cancellation. Probes have supervised deadlines;
metadata reads occur outside short 64-record write batches. Earlier committed
batches survive a later failure/cancellation and progress counts only confirmed
imports. Closing the scan dialog or owning shell requests cancellation. Failed
or cancelled work is not a completed scan. Native non-UTF-8 library paths are
refused without destructive replacement; a lossless schema migration is not
included in this patch.

Playlist file imports are bounded and parsed before locking the DB, create each
playlist atomically, and report partial folder imports. Read errors do not replace
existing queues/dialog data with empty lists. Confirmed document exports are
staged and never overwrite a chosen existing file.

## Verification in this repository

Run `python3 scripts/audit-final/run.py --output NEW_LOG_DIR --mechanisms --browser`.
The runner uses real exit codes; missing tools are BLOCKED, not PASS. The new
`pending_tasks_checks.py` and its negative-mutation tests are textual integration
checks, not execution of Rust code. Auxiliary curl/FFmpeg/POSIX tests run real
installed tools with synthetic data, not the Rust wrappers. Browser tests use
real web assets and simulated transport, not this Rust listener.

The Rust tests for deferred state, accounts, the remote server and unavailable
records run with `cargo test`; the scripts above certify no GTK appearance,
Orca, keyring or receiver behavior.

### Close and system-action confirmation

Normal close drains admitted writes, exports and scans without blocking GTK. A
progress dialog allows the user to keep the window open; that choice does not
cancel operations. After work ends, the live weak registry of settings and
independent documents is checked. Failed or unconfirmed local edits require an
explicit decision: **Keep open** is the default; continuing without saved changes
does not mark any document durable. When no confirmation window exists, a system
action is refused instead of silently discarding pending state. Forced process
termination and a host that bypasses these hooks remain outside the guarantee.
