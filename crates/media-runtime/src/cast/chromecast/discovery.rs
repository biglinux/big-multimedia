use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;

use log::info;

use super::CastSessionController;
use super::network::avahi_browse_chromecasts;

impl CastSessionController {
    /// Discovers Cast devices through the system Avahi daemon.
    pub fn start_discovery(&self, _timeout_secs: u64) {
        if self.discovering.load(Ordering::Relaxed) {
            return;
        }
        self.discovering.store(true, Ordering::Relaxed);

        let devices = Arc::clone(&self.devices);
        let discovering = Arc::clone(&self.discovering);

        thread::spawn(move || {
            let avahi_devices = avahi_browse_chromecasts();
            if !avahi_devices.is_empty() {
                let mut current_devices = devices
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for device in avahi_devices {
                    if !current_devices
                        .iter()
                        .any(|candidate| candidate.host == device.host)
                    {
                        info!(
                            "Avahi discovered Cast device: {} ({})",
                            device.name, device.host
                        );
                        current_devices.push(device);
                    }
                }
            }

            discovering.store(false, Ordering::Relaxed);
            info!("Cast discovery finished");
        });
    }
}
