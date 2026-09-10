//! Chromecast / Cast support — mDNS discovery, CASTV2 protocol, and local HTTP server
//! for streaming media to Google Cast-compatible devices.

mod casting;
mod control;
mod discovery;
mod network;
mod state;
mod transcoding;
mod types;

pub use state::CastSessionController;
pub use types::{CastCommand, DiscoveredCastDevice};

pub use network::get_local_ip;
