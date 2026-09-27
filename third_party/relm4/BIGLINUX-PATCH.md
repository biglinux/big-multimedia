# relm4 0.11.0, patched

A copy of the relm4 0.11.0 library from crates.io, with one change in
`src/runtime_util.rs`: before a component's shutdown sender is added to the
global `SHUTDOWN_SENDERS` list, the senders of components that have already
shut down are dropped. Upstream only drains that list in `shutdown_all`, so each
component launched during a session kept its channel until the process ended —
measured here with `component_probe`: 25 minus 5 component lifetimes retained
20 channel allocations (10,240 bytes) plus 20 queue blocks (640 bytes).
The 5/25-cycle heaptrack comparison is the regression workload. Pruning occurs
at the next component launch; this bounds retention rather than promising an
empty registry immediately after the final component closes.

Remove this directory and the `[patch.crates-io]` entry in the workspace
`Cargo.toml` when relm4 prunes the list itself.
