# big-media-components

Reusable media UI components for BigLinux Rust applications.

Current modules:

- `bytes`: shared byte-size formatting for converters, browsers, metadata, and
  download progress.
- `media_metadata_grid`: compact resolution/codec/size/duration metadata grid.
- `time`: shared media duration/time formatting for players, editors, queues,
  and metadata.
- `video_aux_controls`: playback speed, volume, audio track, and subtitle
  track popover controls.
- `video_crop_controls`: crop spinbuttons plus reset/grid controls.
- `video_transport`: play/pause, seekbar, time labels, skip/step buttons, and
  fullscreen controls.
- `surface`: canonical fullscreen/OSD/overlay/idle-chrome surface API for
  camera, video, and image viewers.

This crate depends on `big-relm4-components` for shared BigLinux UI primitives
such as the custom tooltip helper.
