# big-mpv-render

Reusable mpv render/offload primitives for BigLinux Rust applications.

This crate currently stores the render-path policy as code. Move the actual
`FastRenderContext`/`OffloadRenderer` API here only after the player and video
converter editor have a stable shared interface.
