# Settings and environment

Where each player keeps its preferences, how that file is read, and the
environment variables the players read.

## The settings file

Each player has its own file:

| Player | File |
|---|---|
| Big Audio Player | `~/.config/big-audio-player/settings.conf` |
| Big Video Player | `~/.config/big-video-player/settings.conf` |

`~/.config` stands for `$XDG_CONFIG_HOME` when that is an absolute path. When
neither `XDG_CONFIG_HOME` nor `HOME` is absolute, the player keeps its settings
in memory only and reports that it cannot save them.

Close the player before you edit the file. A running player refuses to save
over a file another program changed after it was loaded.

### Format

- UTF-8 text, one `key=value` per line. Blank lines and lines that start with
  `#` are skipped.
- The key is trimmed; the value is everything after the first `=`, spaces
  included. A key holds 1 to 128 letters, digits, `-` or `_`; a value
  holds at most 65,536 bytes and no control characters.
- The whole file is at most 1 MiB and must be a regular file, not a symlink.
- The loader checks the keys typed in
  [`keys.rs`](../crates/foundation/big-media-data/src/settings/keys.rs): a bool is `true` or
  `false`, an integer fits 32 bits, a number is finite, and `render-mode`,
  `tv-resolution` and `noise-voice-sensitivity` take only the values listed.
  Spaces around such a value are ignored and dropped on the next save. Other
  keys keep their value as written; a bool among them reads as `true` only
  when it is exactly `true`.
- An unknown key that follows these rules is kept, and written back on the
  next save.
- One bad line, a bad value for a known key or a key given twice rejects the
  whole file. The player then runs with the defaults, shows that its settings
  cannot be saved, and leaves the file untouched.

On save the player rewrites the file atomically with mode 0600. It writes every
key it holds, defaults included, sorted by key; comments are not kept.

## Keys

These tables list every key in the catalog the players share
([`keys.rs`](../crates/foundation/big-media-data/src/settings/keys.rs)): its type
and the default written into a new file; a unit test keeps them equal. They list what the code reads and writes, not what each
value does on screen; the Notes column repeats only what the code states. Each
player reads the keys it uses from its own file and keeps the rest.

Restore Default Settings in the video player writes every default except the
keys in `Settings::KEPT_ON_RESTORE`
([`store.rs`](../crates/foundation/big-media-data/src/settings/store.rs)): the
library folders, keyboard shortcuts, volume, shuffle, voice and equalizer
levels, the folders the browsers left off in, the one-time playlist hint,
dialog sizes, and the library grouping and column widths. The running player takes
the new values at once, except `render-mode`, `gpu-dumb-mode`,
`custom-mpv-options` and `audio-output`, which apply when the player reopens.

### Volume and audio output

| Key | Type | Default | Notes |
|---|---|---|---|
| `volume` | number | `1` | The video player's volume as a fraction of 100%, saved half a second after it stops changing and at close. It starts at this value, kept within 0 and `volume-max`. |
| `volume-max` | integer | `150` | Percent; the players keep it within 100 to 200. |
| `audio-volume` | integer | `100` | The audio player's volume in percent, saved and restored like `volume`. |
| `audio-output` | string | empty |  |
| `audio-channels` | string | empty |  |
| `preferred-audio-lang` | string | empty |  |
| `audio-normalize` | bool | `false` |  |
| `audio-pitch-correction` | bool | `true` |  |
| `replaygain` | string | `track` |  |
| `replaygain-preamp` | number | `0.0` |  |
| `gapless-audio` | bool | `false` |  |

### Audio effects

| Key | Type | Default | Notes |
|---|---|---|---|
| `eq-preset` | string | `flat` |  |
| `eq-bands` | string | empty | Ten gains in dB separated by `;`, 31 Hz first, each kept within ±12; a missing gain is 0. |
| `noise-strength` | number | `0.0` |  |
| `noise-model` | integer | `0` | `0` DeepFilterNet3, `1` DPDFNet-2. |
| `noise-mono` | bool | `false` |  |
| `noise-voice-only` | bool | `false` | Keep only what the denoiser hears as a voice. |
| `noise-voice-sensitivity` | `1`, `2`, `3`, `4`, `5` | `3` |  |

### Playback

| Key | Type | Default | Notes |
|---|---|---|---|
| `resume-playback` | bool | `true` |  |
| `loop-mode` | string | `no` |  |
| `idle-mode` | string | `yes` |  |
| `keep-open` | string | `yes` |  |
| `keep-open-pause` | bool | `false` |  |
| `stop-screensaver` | string | `yes` |  |
| `cache` | bool | `false` |  |
| `cache-secs` | integer | `30` |  |
| `cache-pause` | bool | `true` |  |
| `cache-on-disk` | bool | `false` |  |
| `demuxer-readahead-secs` | number | `1.0` |  |
| `seekbar-thumbnail` | bool | `true` |  |
| `sleep-timer-action` | string | `pause` |  |
| `sleep-timer-minutes` | integer | `30` |  |
| `tv-resolution` | `0`, `720`, `1080`, `1440`, `2160` | `0` | Frame height sent to a TV; `0` keeps the file's own. |

### Video and rendering

| Key | Type | Default | Notes |
|---|---|---|---|
| `hwdec` | string | `auto-safe` |  |
| `deinterlace` | bool | `false` |  |
| `auto-window-resize` | bool | `true` |  |
| `render-mode` | `auto`, `gl`, `sw` | `auto` |  |
| `render-profile` | string | `default` |  |
| `gpu-dumb-mode` | bool | `false` |  |
| `custom-mpv-options` | string | empty |  |
| `glsl-shaders` | string | empty |  |
| `builtin-upscaler` | string | empty |  |
| `shader-fsr-sharpness` | number | `0` | `0` reads as `0.2`. |
| `shader-cas-sharpening` | number | `0` |  |
| `anime4k-preset` | string | empty |  |
| `neural-upscaler` | string | empty |  |
| `tone-mapping` | string | `auto` |  |
| `target-peak` | integer | `0` |  |
| `hdr-compute-peak` | bool | `false` |  |
| `gamut-mapping-mode` | string | `auto` |  |
| `icc-profile` | string | empty |  |
| `icc-intent` | integer | `0` |  |

### Subtitles

| Key | Type | Default | Notes |
|---|---|---|---|
| `preferred-sub-lang` | string | empty |  |
| `sub-auto` | string | `fuzzy` |  |
| `sub-font` | string | empty |  |
| `subtitle-font-size` | integer | `36` |  |
| `sub-bold` | bool | `false` |  |
| `sub-color` | string | `#FFFFFFFF` |  |
| `sub-border-style` | string | `outline-and-shadow` |  |
| `sub-border-size` | number | `2.5` |  |
| `sub-border-color` | string | `#FF000000` |  |
| `sub-shadow-color` | string | `#80000000` |  |
| `sub-shadow-offset` | number | `0.0` |  |
| `sub-gauss` | number | `0.0` |  |
| `sub-gray` | bool | `false` |  |
| `sub-margin-y` | integer | `22` |  |
| `sub-scale-with-window` | bool | `true` |  |
| `sub-ass-scale-with-window` | bool | `false` |  |
| `sub-filter-sdh` | bool | `false` |  |
| `sub-filter-sdh-harder` | bool | `false` |  |

### Screenshots

| Key | Type | Default | Notes |
|---|---|---|---|
| `screenshot-png-compression` | integer | `7` |  |
| `screenshot-directory` | string | empty |  |
| `screenshot-tag-colorspace` | bool | `true` |  |
| `screenshot-high-bit-depth` | bool | `true` |  |

### Playlist, library and files

| Key | Type | Default | Notes |
|---|---|---|---|
| `playlist-view` | string | `list` |  |
| `playlist-sort` | string | `name` |  |
| `playlist-sort-desc` | bool | `false` |  |
| `playlist-shuffle` | bool | `false` |  |
| `playlist-folder-recursive` | bool | `false` |  |
| `playlist-last-folder` | string | empty | The folder the video Files tab reopens at start. Empty, or a folder that no longer exists, opens the places. |
| `playlist-reorder-hint-seen` | bool | `false` | The one-time "drag to reorder" line was closed. |
| `playlist-column-widths` | string | empty |  |
| `video-library-folders` | string | empty | A JSON list; empty means Videos, Downloads and the home folder. |
| `files-last-dir` | string | empty |  |
| `library-grouping` | string | empty |  |
| `radio-api-servers` | string | `de2.api.radio-browser.info,nl1.api.radio-browser.info,at1.api.radio-browser.info,de1.api.radio-browser.info` | Comma-separated Radio Browser hosts; empty finds them through DNS. |

### Video player controls and colors

| Key | Type | Default | Notes |
|---|---|---|---|
| `ui-bg-color` | string | `#2A2A30` |  |
| `ui-bg-opacity` | number | `0.90` |  |
| `ui-button-custom` | bool | `false` |  |
| `ui-button-radius` | integer | `18` |  |
| `ui-button-size` | integer | `36` |  |
| `ui-color-source` | string | `gradient` |  |
| `ui-controls-margin-bottom` | integer | `0` |  |
| `ui-controls-margin-side` | integer | `0` |  |
| `ui-controls-padding-bottom` | integer | `0` |  |
| `ui-controls-padding-left` | integer | `0` |  |
| `ui-controls-padding-right` | integer | `0` |  |
| `ui-controls-padding-top` | integer | `0` |  |
| `ui-controls-radius` | integer | `0` |  |
| `ui-dialog-color` | string | `#2A2A30` |  |
| `ui-font-color` | string | `#FFFFFF` |  |
| `ui-icon-color` | string | `#FFFFFF` |  |
| `ui-menu-opacity` | number | `0.95` |  |
| `ui-osd-shadow` | bool | `false` |  |
| `ui-show-audio-track` | bool | `true` |  |
| `ui-show-fullscreen` | bool | `true` |  |
| `ui-show-menu` | bool | `true` |  |
| `ui-show-next` | bool | `true` |  |
| `ui-show-open` | bool | `true` |  |
| `ui-show-pin` | bool | `true` |  |
| `ui-show-play` | bool | `true` |  |
| `ui-show-playlist` | bool | `true` |  |
| `ui-show-prev` | bool | `true` |  |
| `ui-show-speed` | bool | `true` |  |
| `ui-show-subtitle` | bool | `true` |  |
| `ui-show-tv` | bool | `true` |  |
| `ui-show-voice` | bool | `true` |  |
| `ui-show-volume` | bool | `true` |  |
| `ui-window-btn-custom-size` | bool | `false` |  |
| `ui-window-radius` | integer | `12` |  |
| `ui-window-radius-custom` | bool | `false` |  |
| `ui-zone-bottom-center` | string | `prev,play,next` |  |
| `ui-zone-bottom-left` | string | `volume,speed,voice` |  |
| `ui-zone-bottom-right` | string | `subtitle,audio-track,tv,fullscreen` |  |
| `ui-zone-top-left` | string | `pin,open,playlist` |  |
| `ui-zone-top-right` | string | `menu` |  |

### Audio player appearance

| Key | Type | Default | Notes |
|---|---|---|---|
| `audio-ui-color-source` | string | `gtk-default` |  |
| `audio-ui-bg-color` | string | `#2A2A30` |  |
| `audio-ui-font-color` | string | `#FFFFFF` |  |
| `audio-ui-icon-color` | string | `#FFFFFF` |  |
| `audio-ui-dialog-color` | string | `#2A2A30` |  |
| `audio-high-contrast` | bool | `false` |  |
| `audio-album-art-bg` | bool | `true` |  |

### Dialog sizes

| Key | Type | Default | Notes |
|---|---|---|---|
| `appearance-window-width` | integer | `600` |  |
| `appearance-window-height` | integer | `700` |  |
| `audio-appearance-window-width` | integer | `500` |  |
| `audio-appearance-window-height` | integer | `400` |  |

### Desktop integration and shortcuts

| Key | Type | Default | Notes |
|---|---|---|---|
| `custom-keybindings` | string | empty |  |
| `taskbar-progress` | bool | `true` |  |
| `tray-icon` | bool | `false` |  |

## Environment variables

| Variable | Read by | Effect |
|---|---|---|
| `BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD` | video player, when it creates the video surface | `1`, `true`, `yes` or `on` (any case) draws video through a GL area; `0`, `false`, `no`, `off` or empty first tries GTK graphics offload, falling back to the GL area. The video player sets it to `1` when it is unset. Sessions on the proprietary NVIDIA driver never use offload. |
| `BIG_MEDIA_DB_APP_ID` | audio player, each time it opens the music library | Names the folder of the library database, `~/.config/<id>/library.db`. Unset or empty means `big-audio-player`. No shipped program sets it. |

To try graphics offload in the video player:

```sh
BIG_MEDIA_DISABLE_GRAPHICS_OFFLOAD=0 big-video-player
```
