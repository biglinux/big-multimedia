//! Chromecast network plumbing: Avahi discovery, MIME detection, local file/
//! pipe HTTP server, local IP lookup, and the cast session loop.

mod avahi;
mod cast_session;
mod http;
mod local_ip;
mod mime;

pub(in crate::cast::chromecast) use avahi::avahi_browse_chromecasts;
pub(in crate::cast::chromecast) use cast_session::cast_session_loop;
pub(in crate::cast::chromecast) use http::{start_file_server, start_pipe_server};
pub use local_ip::get_local_ip;
pub(in crate::cast::chromecast) use mime::guess_mime;

#[cfg(test)]
mod tests;
