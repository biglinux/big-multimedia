//! mpv backend: wraps the system libmpv C API for audio/video playback with render API integration.
//!
//! Provides `MpvBackend` which manages an mpv instance behind a mutex,
//! supporting property observation, audio filter chains, and OpenGL rendering.

mod backend;
mod screenshots;
pub use screenshots::{ScreenshotRequestError, ScreenshotResult};
mod types;

pub use backend::{MpvBackend, MpvError};
pub use types::{
    AudioCacheProfile, AudioEffects, AudioMetadata, EndReason, PlayerEvent, TargetSource,
    TrackMetadata,
};

#[cfg(feature = "video")]
mod dmabuf_feedback;
#[cfg(feature = "video")]
pub(crate) mod egl_dmabuf;
#[cfg(feature = "video")]
pub mod fast_render;
pub mod filters;
#[cfg(feature = "video")]
pub mod frame_layout;
#[cfg(feature = "video")]
pub mod gl_proc_address;
/// GPU-offloaded video renderer: DMA-BUF export (MESA) or GdkGLTexture
/// fallback → `GtkGraphicsOffload` → Wayland subsurface. Used by both the
/// video player and the video converter's edit page.
#[cfg(feature = "video")]
pub mod offload_renderer;
mod playback;
mod properties;
mod properties_core;
#[cfg(feature = "video")]
mod properties_rendering;
#[cfg(feature = "video")]
mod properties_subtitles;
#[cfg(feature = "video")]
mod properties_video;
#[cfg(feature = "video")]
pub use properties_video::VideoParams;
#[cfg(feature = "video")]
pub mod shader_gen;
#[cfg(feature = "video")]
pub mod software_frames;
#[cfg(feature = "video")]
pub mod video_surface;
